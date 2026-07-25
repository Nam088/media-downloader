use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, State};

use crate::downloader::gallery_dl;
use crate::downloader::gallery_dl::GalleryDump;
use crate::downloader::ytdlp;
use crate::downloader::ytdlp::YtDlpChild;
use crate::error::AppError;
use crate::models::{AudioFormatOption, GalleryItemPreview, MediaSource, VideoQualityOption};
use crate::platform::detect_platform;

/// Tracks the yt-dlp process backing each in-flight `preview_media` call, so
/// `cancel_preview_media` can actually kill it instead of just letting the
/// frontend give up on waiting while yt-dlp keeps running unattended.
#[derive(Default)]
pub struct ActivePreviews(Mutex<HashMap<String, YtDlpChild>>);

impl ActivePreviews {
    fn insert(&self, source_url: String, child: YtDlpChild) {
        let mut map = self.0.lock().expect("active previews mutex poisoned");
        map.insert(source_url, child);
    }

    fn remove(&self, source_url: &str) {
        let mut map = self.0.lock().expect("active previews mutex poisoned");
        map.remove(source_url);
    }

    fn kill(&self, source_url: &str) -> Option<YtDlpChild> {
        let mut map = self.0.lock().expect("active previews mutex poisoned");
        map.remove(source_url)
    }
}

#[tauri::command]
pub async fn cancel_preview_media(previews: State<'_, ActivePreviews>, source_url: String) -> Result<bool, AppError> {
    match previews.kill(&source_url) {
        Some(child) => {
            let mut guard = child.lock().await;
            let _ = guard.start_kill();
            Ok(true)
        }
        None => Ok(false),
    }
}

#[derive(Clone)]
struct CachedPreview {
    source: MediaSource,
    /// Individual video URLs when `source.is_playlist` (T033); empty
    /// otherwise. Not part of `MediaSource` itself since `data-model.md` §2
    /// only exposes `playlist_item_count` to the frontend — the full entry
    /// list is an implementation detail used to fan out `entire_playlist`
    /// jobs server-side.
    playlist_entry_urls: Vec<String>,
}

/// Caches the most recent `preview_media` result per `source_url` so
/// `create_download_job` (T019) can validate that the requested
/// `audio_quality`/`video_quality` actually came from this link's real
/// format list, instead of trusting whatever the frontend sends (FR-019,
/// data-model.md "Ràng buộc toàn vẹn dữ liệu chung").
#[derive(Default)]
pub struct PreviewCache(Mutex<HashMap<String, CachedPreview>>);

impl PreviewCache {
    fn store(&self, source: MediaSource, playlist_entry_urls: Vec<String>) {
        let mut map = self.0.lock().expect("preview cache mutex poisoned");
        map.insert(
            source.source_url.clone(),
            CachedPreview {
                source,
                playlist_entry_urls,
            },
        );
    }

    pub fn get(&self, source_url: &str) -> Option<MediaSource> {
        let map = self.0.lock().expect("preview cache mutex poisoned");
        map.get(source_url).map(|cached| cached.source.clone())
    }

    pub fn get_playlist_entry_urls(&self, source_url: &str) -> Option<Vec<String>> {
        let map = self.0.lock().expect("preview cache mutex poisoned");
        map.get(source_url).map(|cached| cached.playlist_entry_urls.clone())
    }
}

#[tauri::command]
pub async fn preview_media(
    app: AppHandle,
    cache: State<'_, PreviewCache>,
    previews: State<'_, ActivePreviews>,
    source_url: String,
) -> Result<MediaSource, AppError> {
    // Whether a link is supported is yt-dlp's call, not ours: it recognizes
    // ~1,600 working extractors today (`yt-dlp --list-extractors`), and
    // FR-014 requires the architecture to accept new platforms without code
    // changes. `dump_metadata_json` already maps yt-dlp's own "no extractor
    // for this URL" failure to `AppError::unsupported_platform` (see
    // `downloader::ytdlp::classify_ytdlp_error`) — pre-rejecting here on our
    // own hard-coded domain list would silently reject sites yt-dlp actually
    // supports, which is exactly the mistake FR-019 warns against.
    let url_for_registry = source_url.clone();
    let result = ytdlp::dump_metadata_json(&app, &source_url, |child| {
        previews.insert(url_for_registry, child);
    })
    .await;
    // Always deregister once the process has exited, on every path (success,
    // yt-dlp error, or canceled) — otherwise a stale entry would make a later
    // `cancel_preview_media` for the same URL try to kill an already-dead
    // child.
    previews.remove(&source_url);

    let (source, playlist_entry_urls) = match result {
        Ok(raw) => {
            if is_login_required(&raw) {
                return Err(AppError::access_denied(
                    "This content requires login or is private/DRM-protected",
                ));
            }
            let platform = resolve_platform_label(&source_url, &raw);
            let yt_dlp_source = build_media_source(&source_url, &platform, &raw);

            // yt-dlp found no actual video track — on some platforms (e.g. a
            // TikTok slideshow: a multi-image post with a background music
            // track and no video at all) that's not "there's nothing here",
            // it's "yt-dlp can only see the audio track of a post that also
            // has images it has no way to expose" (confirmed empirically:
            // yt-dlp's own tiktok.py extractor reads only `video`/`music`
            // out of the raw API response and discards everything else, see
            // `research.md` §2's gallery-dl amendment). Check whether
            // gallery-dl — a tool actually built for image/gallery posts —
            // has a richer view of the same URL before settling for the
            // audio-only result.
            if yt_dlp_source.available_video_qualities.is_empty() && !yt_dlp_source.is_playlist {
                match try_gallery_dl_preview(&app, &previews, &source_url, &platform).await {
                    Some(gallery_source) => (gallery_source, Vec::new()),
                    None => {
                        let playlist_entry_urls = if yt_dlp_source.is_playlist {
                            extract_playlist_entry_urls(&raw)
                        } else {
                            Vec::new()
                        };
                        (yt_dlp_source, playlist_entry_urls)
                    }
                }
            } else {
                let playlist_entry_urls = if yt_dlp_source.is_playlist {
                    extract_playlist_entry_urls(&raw)
                } else {
                    Vec::new()
                };
                (yt_dlp_source, playlist_entry_urls)
            }
        }
        // yt-dlp has no extractor for this URL at all — this is gallery-dl's
        // primary fallback case (Pixiv, Reddit, Twitter/X image posts, and
        // ~280 other gallery/image-focused sites yt-dlp was never meant to
        // cover). Any other yt-dlp error (access denied, a real network
        // failure) is unlikely to be fixed by trying a different tool, so
        // only unsupported-platform triggers this path.
        Err(err) if err.code == "UNSUPPORTED_PLATFORM" => {
            let platform = detect_platform(&source_url)
                .map(str::to_string)
                .unwrap_or_else(|| "unknown".to_string());
            match try_gallery_dl_preview(&app, &previews, &source_url, &platform).await {
                Some(gallery_source) => (gallery_source, Vec::new()),
                None => return Err(err),
            }
        }
        Err(err) => return Err(err),
    };

    cache.store(source.clone(), playlist_entry_urls);
    Ok(source)
}

/// Runs gallery-dl's own preview (`--dump-json --no-download`) for
/// `source_url` and, if it actually found anything, builds a gallery-backed
/// `MediaSource` from the result. Returns `None` on any failure (unsupported
/// by gallery-dl either, network error, etc.) or an empty result — the
/// caller already has a yt-dlp result (or a yt-dlp error) to fall back to in
/// that case, so a gallery-dl failure here is never fatal on its own.
async fn try_gallery_dl_preview(
    app: &AppHandle,
    previews: &ActivePreviews,
    source_url: &str,
    platform: &str,
) -> Option<MediaSource> {
    let url_for_registry = source_url.to_string();
    let result = gallery_dl::dump_gallery_json(app, source_url, |child| {
        previews.insert(url_for_registry, child);
    })
    .await;
    previews.remove(source_url);

    let dump = match result {
        Ok(dump) => dump,
        Err(err) => {
            // Never fatal on its own (the caller already has a yt-dlp result
            // or error to fall back to), but silently discarding this made a
            // genuine gallery-dl invocation failure (missing/uncached
            // resource, launch failure, etc.) indistinguishable from "this
            // URL just isn't a gallery" — log it so that distinction is at
            // least visible in the terminal running `tauri dev`.
            eprintln!("[preview_media] gallery-dl fallback failed for {source_url}: {err}");
            return None;
        }
    };
    if dump.entries.is_empty() {
        return None;
    }
    Some(build_gallery_media_source(source_url, platform, &dump))
}

/// Flat-playlist entries reliably carry `webpage_url` (the canonical link)
/// across yt-dlp's extractors; `url` is a fallback for the rare extractor
/// that omits it.
fn extract_playlist_entry_urls(raw: &serde_json::Value) -> Vec<String> {
    raw.get("entries")
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .get("webpage_url")
                        .or_else(|| entry.get("url"))
                        .and_then(|v| v.as_str())
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_login_required(raw: &serde_json::Value) -> bool {
    raw.get("requires_login")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Prefers our own lowercase `snake_case` label for the platforms FR-014
/// names explicitly (so the UI/data model stay stable — `"youtube"`,
/// `"tiktok"`, etc.), and falls back to whatever yt-dlp itself calls the
/// site (`extractor_key`, e.g. `"Bilibili"`, `"Vimeo"`) for the ~1,600 other
/// working extractors it supports, so those links aren't rejected just
/// because they're not one of the 6 platforms this app was built to
/// showcase.
fn resolve_platform_label(source_url: &str, raw: &serde_json::Value) -> String {
    if let Some(known) = detect_platform(source_url) {
        return known.to_string();
    }
    raw.get("extractor_key")
        .or_else(|| raw.get("extractor"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_else(|| "unknown".to_string())
}

fn build_media_source(source_url: &str, platform: &str, raw: &serde_json::Value) -> MediaSource {
    let is_playlist = raw.get("_type").and_then(|v| v.as_str()) == Some("playlist");

    let playlist_item_count = if is_playlist {
        raw.get("playlist_count")
            .and_then(|v| v.as_i64())
            .or_else(|| raw.get("entries").and_then(|e| e.as_array()).map(|a| a.len() as i64))
    } else {
        None
    };

    // Flat-playlist previews don't carry per-format info for each entry —
    // exact quality selection happens when a playlist item becomes its own
    // job (see queue.rs "best" fallback), so these stay empty for playlists
    // rather than presenting options that don't really exist yet (FR-019).
    let (available_video_qualities, available_audio_formats) = if is_playlist {
        (Vec::new(), Vec::new())
    } else {
        extract_format_options(raw)
    };

    MediaSource {
        source_url: source_url.to_string(),
        title: raw
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string(),
        thumbnail_url: raw.get("thumbnail").and_then(|v| v.as_str()).map(String::from),
        duration_seconds: raw.get("duration").and_then(|v| v.as_f64()).map(|d| d as i64),
        platform: platform.to_string(),
        is_playlist,
        playlist_item_count,
        available_video_qualities,
        available_audio_formats,
        is_gallery: false,
        gallery_items: Vec::new(),
    }
}

fn build_gallery_media_source(source_url: &str, platform: &str, dump: &GalleryDump) -> MediaSource {
    let gallery_items: Vec<GalleryItemPreview> = dump
        .entries
        .iter()
        .map(|entry| {
            let is_audio = entry
                .extension
                .as_deref()
                .map(gallery_dl::is_audio_extension)
                .unwrap_or(false);
            GalleryItemPreview {
                url: entry.url.clone(),
                extension: entry.extension.clone(),
                is_audio,
            }
        })
        .collect();

    let thumbnail_url = gallery_items.iter().find(|item| !item.is_audio).map(|item| item.url.clone());

    MediaSource {
        source_url: source_url.to_string(),
        title: dump.title.clone().unwrap_or_else(|| {
            format!(
                "{} post",
                dump.category.clone().unwrap_or_else(|| platform.to_string())
            )
        }),
        thumbnail_url,
        duration_seconds: None,
        platform: platform.to_string(),
        is_playlist: false,
        playlist_item_count: None,
        available_video_qualities: Vec::new(),
        available_audio_formats: Vec::new(),
        is_gallery: true,
        gallery_items,
    }
}

fn format_filesize(format: &serde_json::Value) -> Option<u64> {
    // `filesize` is exact when yt-dlp knows it upfront; `filesize_approx` is
    // its own best estimate otherwise (derived from bitrate × duration).
    // Either way this is yt-dlp's number, never one we compute ourselves.
    format
        .get("filesize")
        .and_then(|v| v.as_u64())
        .or_else(|| format.get("filesize_approx").and_then(|v| v.as_u64()))
}

fn extract_format_options(raw: &serde_json::Value) -> (Vec<VideoQualityOption>, Vec<AudioFormatOption>) {
    let formats = raw
        .get("formats")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    // height -> largest known filesize among video-having formats at that
    // height (adaptive streams are video-only, so this is roughly the video
    // track's own size; combined with `best_audio_filesize` below it
    // approximates the final muxed file size, matching what the format
    // selector in `downloader::queue` will actually pick).
    let mut video_by_height: HashMap<i64, Option<u64>> = HashMap::new();
    let mut audio_options: Vec<(u32, String, Option<u64>)> = Vec::new();
    let mut best_audio_filesize: Option<u64> = None;
    let mut best_audio_bitrate = -1.0f64;
    // Some platforms (TikTok, notably) only serve pre-muxed video+audio
    // without ever reporting a per-format audio bitrate (`abr`) — real audio
    // is still there and still extractable via `-x`, there's just no number
    // to label it with. Track the best candidate for that case separately
    // so audio downloads aren't blocked just because no bitrate was given.
    let mut fallback_audio: Option<(String, Option<u64>)> = None;

    for format in &formats {
        let vcodec = format.get("vcodec").and_then(|v| v.as_str()).unwrap_or("none");
        let acodec = format.get("acodec").and_then(|v| v.as_str()).unwrap_or("none");
        let filesize = format_filesize(format);

        if vcodec != "none" {
            if let Some(height) = format.get("height").and_then(|v| v.as_i64()) {
                let entry = video_by_height.entry(height).or_insert(None);
                if filesize.is_some() && (entry.is_none() || filesize > *entry) {
                    *entry = filesize;
                }
            }
        }

        if acodec != "none" {
            match format.get("abr").and_then(|v| v.as_f64()) {
                Some(abr) => {
                    let bitrate_kbps = abr.round() as u32;
                    if bitrate_kbps > 0 && !audio_options.iter().any(|(b, _, _)| *b == bitrate_kbps) {
                        audio_options.push((bitrate_kbps, acodec.to_string(), filesize));
                    }
                    if abr > best_audio_bitrate {
                        best_audio_bitrate = abr;
                        best_audio_filesize = filesize;
                    }
                }
                // No bitrate reported for this format — remember it as a
                // fallback candidate (prefer whichever has a known filesize,
                // so the estimate shown to the user is real when possible).
                None if fallback_audio.as_ref().is_none_or(|(_, fs)| fs.is_none()) => {
                    fallback_audio = Some((acodec.to_string(), filesize));
                }
                None => {}
            }
        }
    }

    let mut heights: Vec<i64> = video_by_height.keys().copied().collect();
    heights.sort_unstable_by_key(|h| std::cmp::Reverse(*h));
    audio_options.sort_unstable_by_key(|(bitrate, _, _)| std::cmp::Reverse(*bitrate));

    let video_qualities = heights
        .into_iter()
        .map(|h| VideoQualityOption {
            label: format!("{h}p"),
            filesize_bytes: match (video_by_height[&h], best_audio_filesize) {
                (Some(v), Some(a)) => Some(v + a),
                (Some(v), None) => Some(v),
                _ => None,
            },
        })
        .collect();

    let mut audio_formats: Vec<AudioFormatOption> = audio_options
        .into_iter()
        .map(|(bitrate_kbps, codec, filesize_bytes)| AudioFormatOption {
            bitrate_kbps: Some(bitrate_kbps),
            codec,
            filesize_bytes,
        })
        .collect();

    // No format anywhere reported a bitrate (TikTok-style pre-muxed
    // sources), but there was audio — offer the one honest option: extract
    // whatever's there, instead of showing an empty list and blocking the
    // download entirely (FR-004/FR-019 — no fabricated numbers, but also no
    // false "nothing available" when audio genuinely exists).
    if audio_formats.is_empty() {
        if let Some((codec, filesize_bytes)) = fallback_audio {
            audio_formats.push(AudioFormatOption {
                bitrate_kbps: None,
                codec,
                filesize_bytes,
            });
        }
    }

    (video_qualities, audio_formats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn falls_back_to_a_single_best_effort_audio_option_when_no_format_reports_a_bitrate() {
        // Regression test for TikTok-style sources: pre-muxed video+audio
        // formats with no `abr` field at all used to leave
        // available_audio_formats empty, blocking audio downloads entirely
        // even though the audio genuinely exists and is extractable.
        let raw = json!({
            "formats": [
                {
                    "vcodec": "h264",
                    "acodec": "aac",
                    "height": 1080,
                    "filesize": 5_000_000,
                },
                {
                    "vcodec": "h264",
                    "acodec": "aac",
                    "height": 720,
                    "filesize": 3_000_000,
                },
            ]
        });

        let (video_qualities, audio_formats) = extract_format_options(&raw);

        assert_eq!(video_qualities.len(), 2);
        assert_eq!(audio_formats.len(), 1);
        assert_eq!(audio_formats[0].bitrate_kbps, None);
        assert_eq!(audio_formats[0].codec, "aac");
    }

    #[test]
    fn prefers_explicit_bitrates_over_the_fallback_when_both_are_present() {
        let raw = json!({
            "formats": [
                { "vcodec": "none", "acodec": "opus", "abr": 128.0 },
                { "vcodec": "none", "acodec": "opus", "abr": 70.0 },
            ]
        });

        let (_, audio_formats) = extract_format_options(&raw);

        assert_eq!(audio_formats.len(), 2);
        assert!(audio_formats.iter().all(|f| f.bitrate_kbps.is_some()));
    }
}
