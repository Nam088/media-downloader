use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, State};

use crate::downloader::gallery_dl;
use crate::downloader::gallery_dl::GalleryDump;
use crate::downloader::spotiflac;
use crate::downloader::spotiflac::MusicPreview;
use crate::downloader::ytdlp;
use crate::downloader::ytdlp::YtDlpChild;
use crate::error::AppError;
use crate::logging::log_event;
use crate::models::{
    AudioFormatOption, ChapterPreview, GalleryItemPreview, MediaSource, PlaylistEntryPreview,
    SubtitleTrackPreview, VideoQualityOption,
};
use crate::platform::{detect_platform, is_music_url};

/// Ba tier mà mọi preview nhạc chào ra (FR-003). Worker không probe trước
/// được tier nào provider thật sự có — `allow_fallback` của module tự hạ khi
/// tải — nên danh sách này là hằng, không phải kết quả đo.
pub const MUSIC_TIERS: [&str; 3] = ["flac16", "flac24", "mp3_320"];

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
pub async fn cancel_preview_media(
    previews: State<'_, ActivePreviews>,
    source_url: String,
) -> Result<bool, AppError> {
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
        map.get(source_url)
            .map(|cached| cached.playlist_entry_urls.clone())
    }
}

#[tauri::command]
pub async fn preview_media(
    app: AppHandle,
    cache: State<'_, PreviewCache>,
    previews: State<'_, ActivePreviews>,
    source_url: String,
) -> Result<MediaSource, AppError> {
    // Engine nhạc đã được thử và hụt. Giữ lại vì lỗi mà yt-dlp trả về sau đó
    // cho một link Spotify ("cần đăng nhập/DRM") đúng về mặt kỹ thuật nhưng
    // chỉ đường sai hoàn toàn: thứ hỏng là engine nhạc, không phải quyền truy
    // cập của người dùng.
    let mut music_engine_failed = false;

    // Link nhạc lossless (Spotify/Tidal/Apple Music/Pandora) đi qua engine
    // SpotiFLAC TRƯỚC khi yt-dlp được hỏi: yt-dlp không lỗi "sạch" với các
    // link này (nó trả một preview rỗng thay vì UNSUPPORTED_PLATFORM, nên cơ
    // chế `looks_empty_handed` bên dưới không bắt được đáng tin cậy —
    // research.md R2 của specs/006). Worker lỗi thì rơi tiếp xuống chuỗi
    // yt-dlp → gallery-dl hiện hành, nên một link nhạc mà SpotiFLAC bó tay
    // vẫn còn đường tải thường.
    if is_music_url(&source_url) {
        match try_music_preview(&app, &previews, &source_url).await {
            Some((source, entry_urls)) => {
                cache.store(source.clone(), entry_urls);
                return Ok(source);
            }
            None => {
                log_event(
                    &app,
                    "WARN",
                    format!(
                        "preview_media: spotiflac preview failed for {source_url}, falling back to yt-dlp"
                    ),
                );
                music_engine_failed = true;
            }
        }
    }

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
                if music_engine_failed {
                    return Err(music_engine_unavailable(&source_url));
                }
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
            //
            // A yt-dlp "playlist" result is normally excluded from this check
            // (a real, populated playlist has nothing gallery-dl would do
            // better), but a playlist with *zero* entries is exactly as
            // empty-handed as a non-playlist result with no video — confirmed
            // on Imgur's "gallery" URL form, which yt-dlp's own imgur
            // extractor recognizes but resolves to `"entries": [],
            // "playlist_count": 0"`, even though the link genuinely has
            // images gallery-dl can see just fine.
            let looks_empty_handed = yt_dlp_source.available_video_qualities.is_empty()
                && (!yt_dlp_source.is_playlist
                    || yt_dlp_source.playlist_item_count.unwrap_or(0) == 0);
            if looks_empty_handed {
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
                None => {
                    log_event(
                        &app,
                        "INFO",
                        format!(
                            "preview_media: neither yt-dlp nor gallery-dl support {source_url}"
                        ),
                    );
                    if music_engine_failed {
                        return Err(music_engine_unavailable(&source_url));
                    }
                    return Err(unsupported_after_all_engines(&source_url));
                }
            }
        }
        Err(err) => return Err(err),
    };

    cache.store(source.clone(), playlist_entry_urls);
    Ok(source)
}

/// Chạy preview của spotiflac-worker cho một link nhạc và dựng `MediaSource`
/// từ kết quả. Trả `None` cho MỌI thất bại (worker chưa build, module lỗi,
/// mạng đứt…) — người gọi còn nguyên chuỗi yt-dlp → gallery-dl để thử tiếp,
/// nên một lần SpotiFLAC hụt chân không bao giờ tự nó là fatal.
async fn try_music_preview(
    app: &AppHandle,
    previews: &ActivePreviews,
    source_url: &str,
) -> Option<(MediaSource, Vec<String>)> {
    let result = spotiflac::run_music_preview(app, source_url, |child| {
        previews.insert(source_url.to_string(), child.child);
    })
    .await;
    previews.remove(source_url);

    match result {
        Ok(preview) => Some(build_music_media_source(source_url, &preview)),
        Err(err) => {
            log_event(
                app,
                "WARN",
                format!("preview_media: spotiflac worker failed for {source_url}: {err}"),
            );
            None
        }
    }
}

/// `MediaSource` cho một preview nhạc. Track đơn hiện "Artist – Title"; album/
/// playlist/artist thành preview playlist với mỗi track một entry — đi tiếp
/// qua đúng cơ chế fan-out mỗi-bài-một-job hiện có (research.md R7).
fn build_music_media_source(
    source_url: &str,
    preview: &MusicPreview,
) -> (MediaSource, Vec<String>) {
    let platform = detect_platform(source_url)
        .map(str::to_string)
        .unwrap_or_else(|| "unknown".to_string());
    let is_collection = preview.kind != "track" && preview.tracks.len() > 1
        || matches!(preview.kind.as_str(), "album" | "playlist" | "artist");

    let playlist_entries: Vec<PlaylistEntryPreview> = if is_collection {
        preview
            .tracks
            .iter()
            .map(|track| PlaylistEntryPreview {
                url: track.url.clone(),
                title: format!("{} – {}", track.artist, track.title),
                duration_seconds: track.duration_seconds,
                thumbnail_url: track.thumbnail_url.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };
    let entry_urls: Vec<String> = playlist_entries.iter().map(|e| e.url.clone()).collect();

    let title = if is_collection {
        preview.title.clone()
    } else {
        format!("{} – {}", preview.artist, preview.title)
    };
    let duration_seconds = if is_collection {
        None
    } else {
        preview.tracks.first().and_then(|t| t.duration_seconds)
    };

    let source = MediaSource {
        source_url: source_url.to_string(),
        title,
        thumbnail_url: preview
            .thumbnail_url
            .clone()
            .or_else(|| preview.tracks.first().and_then(|t| t.thumbnail_url.clone())),
        duration_seconds,
        platform,
        is_playlist: is_collection,
        playlist_item_count: is_collection.then_some(preview.tracks.len() as i64),
        available_video_qualities: Vec::new(),
        available_audio_formats: Vec::new(),
        is_gallery: false,
        gallery_items: Vec::new(),
        is_music: true,
        available_music_tiers: MUSIC_TIERS.iter().map(|t| t.to_string()).collect(),
        playlist_entries,
        // Nhạc không có khái niệm phụ đề/chương — "chưa kiểm tra" để giao diện
        // ẩn hẳn hai ô chọn đó, cùng quy ước với gallery.
        subtitles: None,
        chapters: None,
    };
    (source, entry_urls)
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
    let result = gallery_dl::dump_gallery_json(app, source_url, |child| {
        previews.insert(source_url.to_string(), child);
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
            // URL just isn't a gallery" — log it (visible in the app's own
            // Logs page, not just a dev-only terminal) so that distinction
            // is diagnosable.
            log_event(
                app,
                "WARN",
                format!("preview_media: gallery-dl fallback failed for {source_url}: {err}"),
            );
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

/// Tên file suy ra từ URL, dùng khi nguồn không cung cấp tiêu đề — điển hình
/// là link trỏ thẳng tới một file media hoặc một manifest HLS, nơi extractor
/// generic của yt-dlp thường không trả về `title` nào cả (FR-130). Đọc từ
/// *path* đã phân tích chứ không phải chuỗi thô, nên token trong query string
/// hay fragment không bao giờ lọt vào tiêu đề.
fn filename_from_url(source_url: &str) -> String {
    url::Url::parse(source_url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back().map(decode_path_segment))
        })
        .filter(|segment| !segment.trim().is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

/// Giải mã percent-encoding của một path segment để hiển thị: `url` trả về
/// segment vẫn còn mã hoá, nên `holiday%202026.mp4` sẽ hiện nguyên xi cho
/// người dùng nếu không giải mã.
///
/// Giải mã là lựa chọn có chủ đích: chuỗi này chỉ dùng để *hiển thị* (nó
/// thành `DownloadJob.title`, không bao giờ thành đường dẫn xuất — yt-dlp tự
/// dựng tên file từ template `-o` của nó), nên lý do thường thấy để giữ
/// nguyên escape (chống chèn dấu phân cách/đi ngược thư mục) không áp dụng ở
/// đây. Rủi ro còn lại là escape giải mã ra thứ không in được, nên bản giải
/// mã chỉ được dùng khi nó là UTF-8 hợp lệ và không chứa ký tự điều khiển;
/// ngược lại giữ nguyên segment thô.
fn decode_path_segment(segment: &str) -> String {
    if !segment.contains('%') {
        return segment.to_string();
    }

    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                decoded.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }

    match String::from_utf8(decoded) {
        Ok(text) if !text.chars().any(char::is_control) => text,
        _ => segment.to_string(),
    }
}

/// Lỗi cuối cùng khi mọi engine đều bó tay. Liệt kê tên engine đã thử để
/// người dùng biết vấn đề nằm ở link chứ không phải ở một cấu hình nào họ
/// quên bật — thông báo "nền tảng không được hỗ trợ" cũ đọc như thể ứng dụng
/// có một danh sách cho phép cố định, điều nó không hề có (FR-131).
/// Link nhạc mà engine SpotiFLAC không chạy được, và không engine nào khác
/// đọc nổi link đó.
///
/// Tách khỏi `access_denied`/`unsupported_after_all_engines` vì hai thông báo
/// kia chỉ sai đường: chúng nói về link hoặc về quyền của người dùng, trong
/// khi thứ hỏng nằm ở phía ứng dụng (worker chưa được đóng gói, bundle hỏng,
/// môi trường thiếu thứ gì đó). Người đọc phải mở Logs xem dòng WARN của
/// engine nhạc chứ không phải đi tìm tài khoản Spotify.
fn music_engine_unavailable(source_url: &str) -> AppError {
    AppError::new(
        "MUSIC_ENGINE_UNAVAILABLE",
        format!(
            "The lossless music engine could not read {source_url}. Check the Logs tab for the SpotiFLAC error — the bundled worker may be missing or failed to start."
        ),
    )
}

fn unsupported_after_all_engines(source_url: &str) -> AppError {
    AppError::new(
        "UNSUPPORTED_ALL_ENGINES",
        format!(
            "No engine could read {source_url}. Tried: yt-dlp (including its generic extractor for direct file and HLS links), then gallery-dl."
        ),
    )
}

fn build_media_source(source_url: &str, platform: &str, raw: &serde_json::Value) -> MediaSource {
    let is_playlist = raw.get("_type").and_then(|v| v.as_str()) == Some("playlist");

    let playlist_item_count = if is_playlist {
        raw.get("playlist_count")
            .and_then(|v| v.as_i64())
            .or_else(|| {
                raw.get("entries")
                    .and_then(|e| e.as_array())
                    .map(|a| a.len() as i64)
            })
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

    let playlist_entries = if is_playlist {
        extract_playlist_entries(raw)
    } else {
        Vec::new()
    };

    // `None` = chưa kiểm tra. Một preview playlist phẳng không hề lấy metadata
    // của từng video (đó là cả điểm của `--flat-playlist`), nên nó không biết
    // gì về phụ đề hay chương — và "không biết" phải khác hẳn "đã kiểm tra,
    // không có cái nào", vốn là thứ duy nhất được phép vô hiệu hoá ô chọn kèm
    // giải thích (FR-221/FR-225).
    let (subtitles, chapters) = if is_playlist {
        (None, None)
    } else {
        (Some(extract_subtitles(raw)), Some(extract_chapters(raw)))
    };

    MediaSource {
        source_url: source_url.to_string(),
        // A whitespace-only title is exactly as useless as a missing one, so
        // both fall through to the URL-derived name (FR-130).
        title: raw
            .get("title")
            .and_then(|v| v.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| filename_from_url(source_url)),
        thumbnail_url: raw
            .get("thumbnail")
            .and_then(|v| v.as_str())
            .map(String::from),
        duration_seconds: raw
            .get("duration")
            .and_then(|v| v.as_f64())
            .map(|d| d as i64),
        platform: platform.to_string(),
        is_playlist,
        playlist_item_count,
        available_video_qualities,
        available_audio_formats,
        is_gallery: false,
        gallery_items: Vec::new(),
        is_music: false,
        available_music_tiers: Vec::new(),
        playlist_entries,
        subtitles,
        chapters,
    }
}

/// Ngôn ngữ phụ đề nguồn thật sự có, đọc thẳng từ JSON của yt-dlp (FR-217).
///
/// yt-dlp trả về hai bản đồ tách bạch — `subtitles` (do người tạo nội dung
/// cung cấp) và `automatic_captions` (máy sinh) — nên sự phân biệt mà FR-217
/// đòi hỏi là dữ liệu có sẵn, không phải thứ ta suy đoán.
///
/// Một ngôn ngữ đã có bản do người tạo cung cấp thì bản tự động của cùng ngôn
/// ngữ ấy bị bỏ qua: nó luôn là bản kém hơn, và trên YouTube danh sách tự động
/// còn kèm cả trăm ngôn ngữ dịch máy — liệt kê hết sẽ chôn vùi đúng những lựa
/// chọn tốt mà người dùng đang tìm.
fn extract_subtitles(raw: &serde_json::Value) -> Vec<SubtitleTrackPreview> {
    // Không phải một ngôn ngữ: YouTube trả về bản ghi chat trực tiếp của một
    // video đã phát xong dưới dạng một "phụ đề" tên `live_chat`.
    const LIVE_CHAT_PSEUDO_LANGUAGE: &str = "live_chat";

    let mut tracks: Vec<SubtitleTrackPreview> = Vec::new();
    for (key, auto_generated) in [("subtitles", false), ("automatic_captions", true)] {
        let Some(map) = raw.get(key).and_then(|value| value.as_object()) else {
            continue;
        };
        // Thứ tự trong JSON không có gì bảo đảm; sắp xếp để cùng một link luôn
        // cho ra cùng một danh sách, không nhảy chỗ giữa hai lần xem trước.
        let mut languages: Vec<_> = map.iter().collect();
        languages.sort_by(|left, right| left.0.cmp(right.0));

        for (language, entries) in languages {
            if language == LIVE_CHAT_PSEUDO_LANGUAGE {
                continue;
            }
            // Một mã ngôn ngữ trỏ tới danh sách rỗng nghĩa là không có file
            // phụ đề nào đằng sau nó — hiện nó ra là mời người dùng chọn một
            // thứ sẽ về tay không.
            if entries.as_array().is_none_or(|list| list.is_empty()) {
                continue;
            }
            if tracks.iter().any(|track| &track.language == language) {
                continue;
            }
            tracks.push(SubtitleTrackPreview {
                language: language.clone(),
                label: subtitle_label(entries),
                auto_generated,
            });
        }
    }
    tracks
}

/// Tên đọc được của một ngôn ngữ, nếu nguồn có kèm (`"name": "Vietnamese"`).
fn subtitle_label(entries: &serde_json::Value) -> Option<String> {
    entries
        .as_array()?
        .iter()
        .find_map(|entry| entry.get("name").and_then(|name| name.as_str()))
        .filter(|name| !name.trim().is_empty())
        .map(str::to_string)
}

/// Danh sách chương của nội dung (FR-225). yt-dlp trả `chapters: null` cho
/// video không có chương, và đó là "đã kiểm tra, không có" — người gọi bọc kết
/// quả này trong `Some` đúng vì thế.
fn extract_chapters(raw: &serde_json::Value) -> Vec<ChapterPreview> {
    raw.get("chapters")
        .and_then(|value| value.as_array())
        .map(|chapters| {
            chapters
                .iter()
                .map(|chapter| ChapterPreview {
                    title: chapter
                        .get("title")
                        .and_then(|value| value.as_str())
                        .filter(|title| !title.trim().is_empty())
                        .map(str::to_string),
                    start_seconds: chapter.get("start_time").and_then(|value| value.as_f64()),
                    end_seconds: chapter.get("end_time").and_then(|value| value.as_f64()),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Best-effort thumbnail for a flat-playlist entry: yt-dlp's own `thumbnail`
/// field is often absent at flat-playlist depth (confirmed empirically — a
/// real YouTube playlist entry had it `null`), falling back to the first of
/// its `thumbnails` array (present instead, smallest-first) when so.
fn extract_entry_thumbnail(entry: &serde_json::Value) -> Option<String> {
    entry
        .get("thumbnail")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| {
            entry
                .get("thumbnails")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|t| t.get("url"))
                .and_then(|v| v.as_str())
                .map(String::from)
        })
}

fn extract_playlist_entries(raw: &serde_json::Value) -> Vec<crate::models::PlaylistEntryPreview> {
    raw.get("entries")
        .and_then(|v| v.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| {
                    let url = entry
                        .get("webpage_url")
                        .or_else(|| entry.get("url"))
                        .and_then(|v| v.as_str())?;
                    Some(crate::models::PlaylistEntryPreview {
                        url: url.to_string(),
                        title: entry
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Untitled")
                            .to_string(),
                        duration_seconds: entry
                            .get("duration")
                            .and_then(|v| v.as_f64())
                            .map(|d| d as i64),
                        thumbnail_url: extract_entry_thumbnail(entry),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
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

    let thumbnail_url = gallery_items
        .iter()
        .find(|item| !item.is_audio)
        .map(|item| item.url.clone());

    MediaSource {
        source_url: source_url.to_string(),
        title: dump.title.clone().unwrap_or_else(|| {
            format!(
                "{} post",
                dump.category
                    .clone()
                    .unwrap_or_else(|| platform.to_string())
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
        is_music: false,
        available_music_tiers: Vec::new(),
        playlist_entries: Vec::new(),
        // gallery-dl không có khái niệm phụ đề hay chương, nên đây là "chưa
        // kiểm tra" chứ không phải "không có" — giao diện nói "không rõ" thay
        // vì hiện một ô chọn rỗng.
        subtitles: None,
        chapters: None,
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

fn extract_format_options(
    raw: &serde_json::Value,
) -> (Vec<VideoQualityOption>, Vec<AudioFormatOption>) {
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
        let vcodec = format
            .get("vcodec")
            .and_then(|v| v.as_str())
            .unwrap_or("none");
        let acodec = format
            .get("acodec")
            .and_then(|v| v.as_str())
            .unwrap_or("none");
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
                    if bitrate_kbps > 0 && !audio_options.iter().any(|(b, _, _)| *b == bitrate_kbps)
                    {
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

    /// Shorthand for the shape yt-dlp's generic extractor actually returns for
    /// a link that points straight at a media file or an HLS manifest: a
    /// `video` result with essentially no metadata attached — no `title`, no
    /// `thumbnail`, no `duration`.
    fn generic_dump_without_title(webpage_url: &str) -> serde_json::Value {
        json!({
            "_type": "video",
            "webpage_url": webpage_url,
        })
    }

    #[test]
    fn direct_media_urls_get_a_title_from_the_filename() {
        // yt-dlp trả về metadata rất nghèo cho link file trực tiếp: thường chỉ
        // có `_type: video` và không có `title`. Rơi về "Untitled" cho toàn bộ
        // nhóm này khiến hàng đợi thành một dãy mục không phân biệt được.
        let url = "https://cdn.example.com/clips/holiday-2026.mp4";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "holiday-2026.mp4");
    }

    #[test]
    fn hls_manifest_urls_get_a_title_from_the_manifest_filename() {
        let url = "https://stream.example.com/live/session-42/master.m3u8";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "master.m3u8");
    }

    #[test]
    fn a_query_string_never_leaks_into_the_derived_title() {
        // CDN links routinely carry a signed token. Deriving the name from the
        // raw string instead of the parsed path would put that token — often
        // hundreds of characters of it — straight into the queue row.
        let url = "https://cdn.example.com/clip.mp4?token=abc123&expires=1799999999";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "clip.mp4");
    }

    #[test]
    fn a_fragment_never_leaks_into_the_derived_title() {
        let url = "https://cdn.example.com/clip.mp4#t=30";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "clip.mp4");
    }

    #[test]
    fn percent_escapes_in_the_filename_are_decoded_for_display() {
        // `holiday%202026.mp4` is what the URL carries; showing that verbatim
        // in the queue is needless noise when the real name is readable.
        let url = "https://cdn.example.com/clips/holiday%202026%20%28final%29.mp4";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "holiday 2026 (final).mp4");
    }

    #[test]
    fn multibyte_percent_escapes_survive_decoding_intact() {
        // Three separate escapes that only mean anything when the decoded
        // *bytes* are reassembled before being read back as UTF-8.
        let url = "https://cdn.example.com/videos/ph%E1%BB%9F.mp4";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "phở.mp4");
    }

    #[test]
    fn escapes_that_do_not_decode_to_clean_text_are_left_alone() {
        // `%FF` is not valid UTF-8 on its own and `%0A` is a newline; neither
        // belongs in a queue row, so the raw segment is kept rather than
        // silently mangled into replacement characters or a line break.
        assert_eq!(
            filename_from_url("https://cdn.example.com/a/%FF.mp4"),
            "%FF.mp4",
            "invalid UTF-8 should leave the segment untouched"
        );
        assert_eq!(
            filename_from_url("https://cdn.example.com/a/two%0Alines.mp4"),
            "two%0Alines.mp4",
            "a control character should leave the segment untouched"
        );
    }

    #[test]
    fn a_trailing_slash_falls_back_instead_of_producing_an_empty_title() {
        let url = "https://example.com/videos/";

        let source = build_media_source(url, "generic", &generic_dump_without_title(url));

        assert_eq!(source.title, "Untitled");
    }

    #[test]
    fn a_url_with_no_path_at_all_falls_back() {
        assert_eq!(filename_from_url("https://example.com"), "Untitled");
        assert_eq!(filename_from_url("https://example.com/"), "Untitled");
        // Not a hierarchical URL at all — there are no path segments to read.
        assert_eq!(filename_from_url("mailto:someone@example.com"), "Untitled");
        assert_eq!(filename_from_url("not a url"), "Untitled");
    }

    #[test]
    fn a_real_title_from_the_extractor_is_kept_as_is() {
        // The fallback must stay invisible to ordinary platform links, which
        // do carry a title — deriving from the URL there would replace a good
        // name ("Never Gonna Give You Up") with a meaningless id.
        let raw = json!({
            "_type": "video",
            "title": "Never Gonna Give You Up",
            "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        });

        let source = build_media_source(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "youtube",
            &raw,
        );

        assert_eq!(source.title, "Never Gonna Give You Up");
    }

    #[test]
    fn a_blank_title_is_treated_the_same_as_a_missing_one() {
        // A whitespace-only title is exactly as useless as no title, and some
        // extractors do emit one.
        let raw = json!({
            "_type": "video",
            "title": "   ",
        });

        let source = build_media_source("https://cdn.example.com/clips/beach.mp4", "generic", &raw);

        assert_eq!(source.title, "beach.mp4");
    }

    #[test]
    fn a_music_link_whose_engine_failed_does_not_get_blamed_on_the_user() {
        // yt-dlp trả "requires_login" cho mọi link Spotify, nên nếu để nguyên
        // lỗi của nó thì một worker chưa build được sẽ hiện ra thành "nội dung
        // riêng tư / cần đăng nhập" — người dùng đi tìm tài khoản Spotify
        // trong khi thứ hỏng nằm hoàn toàn ở phía ứng dụng.
        let error = music_engine_unavailable("https://open.spotify.com/track/x");

        assert_eq!(error.code, "MUSIC_ENGINE_UNAVAILABLE");
        assert!(
            error.message.contains("Logs"),
            "phải chỉ người dùng tới chỗ đọc được lý do thật, got: {}",
            error.message
        );
        assert!(
            !error.message.to_lowercase().contains("login"),
            "không được nói gì về đăng nhập, got: {}",
            error.message
        );
        assert!(error.message.contains("https://open.spotify.com/track/x"));
    }

    #[test]
    fn unrecognised_urls_report_every_engine_that_was_tried() {
        let error = unsupported_after_all_engines("https://example.com/nope");

        assert_eq!(error.code, "UNSUPPORTED_ALL_ENGINES");
        assert!(
            error.message.contains("yt-dlp"),
            "message should name yt-dlp, got: {}",
            error.message
        );
        assert!(
            error.message.contains("gallery-dl"),
            "message should name gallery-dl, got: {}",
            error.message
        );
        assert!(
            error.message.contains("https://example.com/nope"),
            "message should quote the link that failed, got: {}",
            error.message
        );
    }

    // ---- specs/003-media-output: phụ đề & chương (FR-217, FR-221, FR-225) --

    /// Đúng hình dạng `yt-dlp --dump-single-json` trả về cho một video có cả
    /// phụ đề người tạo cung cấp lẫn phụ đề máy sinh: hai bản đồ tách bạch,
    /// mỗi mã ngôn ngữ trỏ tới danh sách các file phụ đề của nó.
    fn dump_with_subtitles() -> serde_json::Value {
        json!({
            "_type": "video",
            "title": "Bài giảng",
            "subtitles": {
                "vi": [{ "ext": "vtt", "url": "https://x/vi.vtt", "name": "Vietnamese" }],
                "en": [{ "ext": "vtt", "url": "https://x/en.vtt", "name": "English" }],
                // Bản ghi chat của một buổi phát trực tiếp — yt-dlp xếp nó
                // chung chỗ với phụ đề, nhưng nó không phải một ngôn ngữ.
                "live_chat": [{ "ext": "json", "url": "https://x/chat.json" }],
            },
            "automatic_captions": {
                // Cùng ngôn ngữ với một bản do người tạo cung cấp ở trên.
                "en": [{ "ext": "vtt", "url": "https://x/en-auto.vtt" }],
                "ja": [{ "ext": "vtt", "url": "https://x/ja-auto.vtt" }],
                // Mã ngôn ngữ không có file nào đằng sau.
                "ko": [],
            }
        })
    }

    #[test]
    fn subtitle_languages_come_from_the_source_and_say_which_are_automatic() {
        // FR-217: danh sách phải là ngôn ngữ nguồn THẬT SỰ có, và phải phân
        // biệt được phụ đề người tạo cung cấp với phụ đề máy sinh — hai thứ ấy
        // nằm ở hai trường khác nhau trong JSON, không phải thứ ta suy ra.
        let source = build_media_source("https://x/1", "youtube", &dump_with_subtitles());
        let subtitles = source.subtitles.expect("preview yt-dlp luôn có kiểm tra");

        let languages: Vec<_> = subtitles
            .iter()
            .map(|track| (track.language.as_str(), track.auto_generated))
            .collect();
        assert_eq!(languages, vec![("en", false), ("vi", false), ("ja", true)]);
        assert_eq!(subtitles[0].label.as_deref(), Some("English"));
    }

    #[test]
    fn the_live_chat_transcript_is_not_offered_as_a_subtitle_language() {
        let source = build_media_source("https://x/1", "youtube", &dump_with_subtitles());
        let subtitles = source.subtitles.unwrap();

        assert!(
            !subtitles.iter().any(|track| track.language == "live_chat"),
            "một bản ghi chat không phải ngôn ngữ phụ đề: {subtitles:?}"
        );
    }

    #[test]
    fn a_language_with_no_actual_subtitle_file_behind_it_is_not_offered() {
        // Chọn nó chỉ dẫn tới một lần tải về tay không mà không có lỗi nào.
        let source = build_media_source("https://x/1", "youtube", &dump_with_subtitles());
        assert!(!source
            .subtitles
            .unwrap()
            .iter()
            .any(|track| track.language == "ko"));
    }

    #[test]
    fn a_source_with_no_subtitles_says_so_instead_of_saying_nothing() {
        // FR-221 sống hay chết ở chỗ này: "đã kiểm tra, không có phụ đề nào"
        // (`Some([])`) phải khác hẳn "chưa kiểm tra" (`None`). Gộp cả hai vào
        // một danh sách rỗng thì giao diện không còn cách nào phân biệt giữa
        // "nguồn này không có phụ đề" và "đang tải, chờ chút" — và ô chọn cứ
        // thế quay mãi.
        let raw = json!({ "_type": "video", "title": "Không phụ đề" });
        let source = build_media_source("https://x/1", "youtube", &raw);

        assert_eq!(source.subtitles, Some(Vec::new()));
        assert_eq!(source.chapters, Some(Vec::new()));
    }

    #[test]
    fn a_flat_playlist_preview_admits_it_never_looked() {
        // `--flat-playlist` cố tình không lấy metadata từng video, nên nó
        // không biết gì về phụ đề hay chương của chúng.
        let raw = json!({
            "_type": "playlist",
            "title": "Danh sách",
            "playlist_count": 2,
            "entries": [
                { "webpage_url": "https://x/a", "title": "A" },
                { "webpage_url": "https://x/b", "title": "B" },
            ]
        });
        let source = build_media_source("https://x/list", "youtube", &raw);

        assert_eq!(source.subtitles, None);
        assert_eq!(source.chapters, None);
    }

    #[test]
    fn chapters_come_back_with_their_names_and_timestamps() {
        // FR-225: giao diện cần đếm được số chương để hiện ra và mở khoá tuỳ
        // chọn tách chương.
        let raw = json!({
            "_type": "video",
            "title": "Podcast",
            "chapters": [
                { "start_time": 0.0, "end_time": 61.0, "title": "Mở đầu" },
                { "start_time": 61.0, "end_time": 900.5, "title": "Nội dung chính" },
                // Chương không tên: giữ lại (nó vẫn là một file kết quả) nhưng
                // KHÔNG bịa tên cho nó (FR-211).
                { "start_time": 900.5, "end_time": 1200.0 },
            ]
        });

        let chapters = build_media_source("https://x/1", "youtube", &raw)
            .chapters
            .expect("preview yt-dlp luôn có kiểm tra");

        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title.as_deref(), Some("Mở đầu"));
        assert_eq!(chapters[1].start_seconds, Some(61.0));
        assert_eq!(chapters[1].end_seconds, Some(900.5));
        assert_eq!(chapters[2].title, None);
    }

    #[test]
    fn a_gallery_preview_reports_neither_subtitles_nor_chapters_as_unknown() {
        // gallery-dl không có khái niệm nào tương ứng, nên đây là "chưa kiểm
        // tra" — không phải "đã kiểm tra và không có".
        let dump = GalleryDump {
            entries: vec![gallery_dl::GalleryEntry {
                url: "https://x/1.jpg".to_string(),
                filename: Some("1".to_string()),
                extension: Some("jpg".to_string()),
            }],
            title: Some("Bài đăng".to_string()),
            category: Some("tiktok".to_string()),
            queue_url: None,
        };

        let source = build_gallery_media_source("https://x/post", "tiktok", &dump);

        assert_eq!(source.subtitles, None);
        assert_eq!(source.chapters, None);
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
