use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use crate::commands::media::PreviewCache;
use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;
use crate::models::{DownloadJob, GalleryMode, JobStatus, MediaType, OutputOptions};
use crate::platform::detect_platform;

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MediaTypeInput {
    Audio,
    Video,
    Gallery,
    Music,
}

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum GalleryModeInput {
    Files,
    AudioOnly,
    ImagesOnly,
    Slideshow,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistScope {
    SingleItem,
    EntirePlaylist,
}

#[derive(Debug, Deserialize)]
pub struct CreateJobInput {
    pub source_url: String,
    pub media_type: MediaTypeInput,
    pub audio_quality: Option<String>,
    pub video_quality: Option<String>,
    #[serde(default)]
    pub gallery_mode: Option<GalleryModeInput>,
    /// A user-picked subset of the preview's `gallery_items`, as 0-based
    /// indices into that array, to actually download — `None`/omitted means
    /// everything. See `models::DownloadJob.selected_gallery_indices`.
    #[serde(default)]
    pub selected_gallery_indices: Option<Vec<u32>>,
    pub output_directory: String,
    #[serde(default)]
    pub playlist_scope: Option<PlaylistScope>,
    /// This job's own display title (e.g. `MediaSource.title`), shown in the
    /// queue instead of the raw `source_url` when available. See
    /// `models::DownloadJob.title`.
    #[serde(default)]
    pub title: Option<String>,
    /// Output format/metadata choices (`specs/003-media-output`). Optional on
    /// purpose: a caller that omits it gets `OutputOptions::default()`, which
    /// is byte-for-byte today's behaviour, so the frontend can adopt the
    /// picker without a lockstep change.
    #[serde(default)]
    pub output_options: Option<OutputOptions>,
}

/// Validates the requested quality against the most recent `preview_media`
/// result for this exact `source_url` (data-model.md "Ràng buộc toàn vẹn dữ
/// liệu chung") — the backend never trusts a quality string the frontend
/// sends without cross-checking it against the real format list (FR-019).
///
/// Playlist previews intentionally return empty `available_*` lists (see
/// `commands::media`, flat-playlist mode doesn't fetch per-video formats), so
/// there is nothing real to validate against yet — skip the check and let
/// `queue::build_ytdlp_args` fall back to yt-dlp's own "best" selection for
/// each fanned-out entry (T033).
fn validate_quality(
    preview: &crate::models::MediaSource,
    input: &CreateJobInput,
) -> Result<(), AppError> {
    if preview.is_playlist {
        return Ok(());
    }

    match input.media_type {
        MediaTypeInput::Audio => match input.audio_quality.as_deref() {
            Some(quality) => {
                let matches = preview.available_audio_formats.iter().any(|f| {
                    f.bitrate_kbps
                        .is_some_and(|b| format!("{b}kbps") == quality)
                });
                if !matches {
                    return Err(AppError::invalid_quality_option());
                }
            }
            None => {
                // Only acceptable when the source never exposed a real
                // bitrate at all (e.g. TikTok's pre-muxed formats — see
                // commands::media::extract_format_options) — the frontend
                // then omits `audio_quality` rather than inventing a number,
                // and queue::build_ytdlp_args already treats `None` as "use
                // yt-dlp's own best available" (FR-019).
                let has_bitrate_option = preview
                    .available_audio_formats
                    .iter()
                    .any(|f| f.bitrate_kbps.is_some());
                if has_bitrate_option || preview.available_audio_formats.is_empty() {
                    return Err(AppError::invalid_quality_option());
                }
            }
        },
        MediaTypeInput::Video => {
            let quality = input
                .video_quality
                .as_deref()
                .ok_or_else(AppError::invalid_quality_option)?;
            if !preview
                .available_video_qualities
                .iter()
                .any(|opt| opt.label == quality)
            {
                return Err(AppError::invalid_quality_option());
            }
        }
        MediaTypeInput::Gallery => {
            // A gallery job only makes sense against a preview that was
            // actually resolved by gallery-dl (`commands::media`'s
            // yt-dlp/gallery-dl routing) — never trust the frontend to send
            // `media_type: gallery` for a link whose cached preview is a
            // regular yt-dlp result (FR-019).
            if !preview.is_gallery || preview.gallery_items.is_empty() {
                return Err(AppError::invalid_quality_option());
            }
            if input.gallery_mode.is_none() {
                return Err(AppError::new(
                    "MISSING_QUALITY",
                    "gallery_mode is required for gallery downloads",
                ));
            }
            // Never trust frontend-supplied indices without bounds-checking
            // them against this exact preview's real item count (FR-019) —
            // same integrity rule as audio/video quality above.
            if let Some(selected) = &input.selected_gallery_indices {
                let item_count = preview.gallery_items.len() as u32;
                if selected.is_empty() || !selected.iter().all(|index| *index < item_count) {
                    return Err(AppError::invalid_quality_option());
                }
            }
        }
        MediaTypeInput::Music => {
            // Chỉ chấp nhận cho một preview mà chính engine SpotiFLAC đã trả
            // về (cùng luật với Gallery: không tin `media_type` frontend gửi
            // cho một link mà preview cache nói khác — FR-019).
            if !preview.is_music {
                return Err(AppError::invalid_quality_option());
            }
            let tier = input
                .audio_quality
                .as_deref()
                .ok_or_else(AppError::invalid_quality_option)?;
            if !preview.available_music_tiers.iter().any(|t| t == tier) {
                return Err(AppError::invalid_quality_option());
            }
            // Các lựa chọn đầu ra video/phụ đề/cắt đoạn không có nghĩa với
            // đường tải nhạc (worker tự tag + nhúng bìa; ffmpeg chỉ đụng vào
            // ở bước transcode MP3 320) — nhận chúng rồi âm thầm bỏ qua là
            // hứa một thứ không làm.
            if let Some(options) = &input.output_options {
                let has_foreign_options = !options.subtitles.languages.is_empty()
                    || options.segment != crate::models::SegmentMode::Whole;
                if has_foreign_options {
                    return Err(AppError::invalid_quality_option());
                }
            }
        }
    }
    Ok(())
}

struct NewJobArgs {
    source_url: String,
    platform: String,
    media_type: MediaTypeInput,
    audio_quality: Option<String>,
    video_quality: Option<String>,
    gallery_mode: Option<GalleryModeInput>,
    selected_gallery_indices: Option<Vec<u32>>,
    output_directory: String,
    is_playlist_item: bool,
    parent_playlist_id: Option<String>,
    title: Option<String>,
    playlist_title: Option<String>,
    output_options: OutputOptions,
}

fn new_job(args: NewJobArgs) -> DownloadJob {
    let now = Utc::now().to_rfc3339();
    DownloadJob {
        id: uuid::Uuid::new_v4().to_string(),
        source_url: args.source_url,
        platform: args.platform,
        media_type: match args.media_type {
            MediaTypeInput::Audio => MediaType::Audio,
            MediaTypeInput::Video => MediaType::Video,
            MediaTypeInput::Gallery => MediaType::Gallery,
            MediaTypeInput::Music => MediaType::Music,
        },
        audio_quality: args.audio_quality,
        video_quality: args.video_quality,
        gallery_mode: args.gallery_mode.map(|mode| match mode {
            GalleryModeInput::Files => GalleryMode::Files,
            GalleryModeInput::AudioOnly => GalleryMode::AudioOnly,
            GalleryModeInput::ImagesOnly => GalleryMode::ImagesOnly,
            GalleryModeInput::Slideshow => GalleryMode::Slideshow,
        }),
        selected_gallery_indices: args.selected_gallery_indices,
        status: JobStatus::Queued,
        progress_percent: 0.0,
        speed_bytes_per_sec: None,
        eta_seconds: None,
        error_message: None,
        output_directory: args.output_directory,
        output_file_path: None,
        is_playlist_item: args.is_playlist_item,
        parent_playlist_id: args.parent_playlist_id,
        retried_from_job_id: None,
        created_at: now.clone(),
        updated_at: now,
        title: args.title,
        playlist_title: args.playlist_title,
        // Chỗ giữ chỗ: `DownloadQueue::enqueue` ghi đè bằng vị trí cuối hàng
        // đợi thật sự (`next_queue_position`). Để nguyên 0.0 ở đây thì mọi job
        // mới đều hoà nhau và đứng trước toàn bộ hàng đợi hiện có.
        queue_position: 0.0,
        retry_count: 0,
        next_retry_at: None,
        output_options: args.output_options,
    }
}

/// Returns one job per created download. A single link normally yields
/// exactly one job; confirming `playlist_scope: "entire_playlist"` (FR-013)
/// yields one job per playlist entry, all sharing `parent_playlist_id`.
#[tauri::command]
pub async fn create_download_job(
    queue: State<'_, DownloadQueue>,
    cache: State<'_, PreviewCache>,
    input: CreateJobInput,
) -> Result<Vec<DownloadJob>, AppError> {
    let preview = cache.get(&input.source_url).ok_or_else(|| {
        AppError::new(
            "PREVIEW_REQUIRED",
            "Call preview_media before creating a job",
        )
    })?;

    validate_quality(&preview, &input)?;

    if preview.is_playlist && input.playlist_scope == Some(PlaylistScope::EntirePlaylist) {
        let entry_urls = cache
            .get_playlist_entry_urls(&input.source_url)
            .filter(|urls| !urls.is_empty())
            .ok_or_else(|| {
                AppError::new(
                    "PLAYLIST_ENTRIES_UNAVAILABLE",
                    "Could not read this playlist's items — try previewing the link again",
                )
            })?;

        let parent_playlist_id = uuid::Uuid::new_v4().to_string();
        let mut jobs = Vec::with_capacity(entry_urls.len());
        for entry_url in entry_urls {
            let platform = detect_platform(&entry_url)
                .map(|p| p.to_string())
                .unwrap_or_else(|| preview.platform.clone());
            let mut job = new_job(NewJobArgs {
                source_url: entry_url,
                platform,
                media_type: input.media_type,
                // A flat-playlist preview has no per-entry format list to
                // validate against (see `validate_quality`'s early return
                // for playlists), but the user can still pick a general
                // quality preference in the form — passed through as-is to
                // every fanned-out entry, exactly like a single-item job,
                // rather than always forcing yt-dlp's own "best" regardless
                // of what was actually selected.
                audio_quality: input.audio_quality.clone(),
                video_quality: input.video_quality.clone(),
                gallery_mode: None,
                selected_gallery_indices: None,
                output_directory: input.output_directory.clone(),
                is_playlist_item: true,
                parent_playlist_id: Some(parent_playlist_id.clone()),
                // A flat-playlist entry URL list has no per-entry title (see
                // `extract_playlist_entry_urls`'s doc comment). Only the
                // playlist's own title is known here, shared across every
                // fanned-out entry as the group header.
                title: None,
                playlist_title: Some(preview.title.clone()),
                // FR-232: một bộ lựa chọn đầu ra áp cho toàn bộ lô fan-out.
                output_options: input.output_options.clone().unwrap_or_default(),
            });
            queue.enqueue(&mut job).await?;
            jobs.push(job);
        }
        return Ok(jobs);
    }

    // `preview.platform` was already resolved by `preview_media` (yt-dlp's
    // own extractor, not just our 6-domain shortlist — see
    // `commands::media::resolve_platform_label`); re-deriving it here via
    // the restrictive `detect_platform` would wrongly reject every link
    // outside that shortlist even though the preview above just proved
    // yt-dlp can handle it.
    let mut job = new_job(NewJobArgs {
        source_url: input.source_url,
        platform: preview.platform.clone(),
        media_type: input.media_type,
        audio_quality: input.audio_quality,
        video_quality: input.video_quality,
        gallery_mode: input.gallery_mode,
        selected_gallery_indices: input.selected_gallery_indices,
        output_directory: input.output_directory,
        is_playlist_item: false,
        parent_playlist_id: None,
        title: input.title,
        playlist_title: None,
        // Bỏ trống nghĩa là "không nêu lựa chọn nào", và `default()` đúng bằng
        // hành vi hiện tại — nên giao diện chưa cập nhật vẫn chạy y như cũ.
        output_options: input.output_options.unwrap_or_default(),
    });
    queue.enqueue(&mut job).await?;
    Ok(vec![job])
}

/// One playlist video the user picked, with its own independently-chosen
/// media type/quality — see `create_playlist_download_jobs`.
#[derive(Debug, Deserialize)]
pub struct PlaylistItemJobInput {
    pub source_url: String,
    pub media_type: MediaTypeInput,
    pub audio_quality: Option<String>,
    pub video_quality: Option<String>,
    /// This video's own title, from `MediaSource.playlist_entries[].title`.
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreatePlaylistJobsInput {
    pub output_directory: String,
    pub items: Vec<PlaylistItemJobInput>,
    /// The playlist's own title (`MediaSource.title`), shared across every
    /// job created from this submission as the queue's group header.
    #[serde(default)]
    pub playlist_title: Option<String>,
    /// One set of output choices applied to every picked video (FR-232).
    /// Omitted means `OutputOptions::default()`, i.e. today's behaviour.
    #[serde(default)]
    pub output_options: Option<OutputOptions>,
}

/// The detailed playlist download flow: one job per user-picked video, each
/// with its own media type and quality (so a single playlist can mix, e.g.,
/// "video for these 3, audio for that one"), all sharing one
/// `parent_playlist_id`. This is the sibling of `create_download_job`'s
/// `playlist_scope: "entire_playlist"` path (which applies one media
/// type/quality uniformly to every entry) — that simpler path is kept for
/// whichever caller still wants "just grab the whole thing the same way".
///
/// No `PreviewCache` lookup/`validate_quality` here: a flat-playlist preview
/// never had a real per-entry format list to validate against in the first
/// place (see `validate_quality`'s own early return for playlists) — each
/// item's quality is already just a generic, unvalidated label exactly like
/// a single playlist-item job already was.
#[tauri::command]
pub async fn create_playlist_download_jobs(
    queue: State<'_, DownloadQueue>,
    input: CreatePlaylistJobsInput,
) -> Result<Vec<DownloadJob>, AppError> {
    if input.items.is_empty() {
        return Err(AppError::new(
            "MISSING_QUALITY",
            "Select at least one video to download",
        ));
    }

    let parent_playlist_id = uuid::Uuid::new_v4().to_string();
    let mut jobs = Vec::with_capacity(input.items.len());
    for item in input.items {
        let platform = detect_platform(&item.source_url)
            .map(|p| p.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let mut job = new_job(NewJobArgs {
            source_url: item.source_url,
            platform,
            media_type: item.media_type,
            audio_quality: item.audio_quality,
            video_quality: item.video_quality,
            gallery_mode: None,
            selected_gallery_indices: None,
            output_directory: input.output_directory.clone(),
            is_playlist_item: true,
            parent_playlist_id: Some(parent_playlist_id.clone()),
            title: item.title,
            playlist_title: input.playlist_title.clone(),
            output_options: input.output_options.clone().unwrap_or_default(),
        });
        queue.enqueue(&mut job).await?;
        jobs.push(job);
    }
    Ok(jobs)
}

#[tauri::command]
pub async fn pause_job(queue: State<'_, DownloadQueue>, job_id: String) -> Result<(), AppError> {
    queue.pause(&job_id).await
}

#[tauri::command]
pub async fn resume_job(queue: State<'_, DownloadQueue>, job_id: String) -> Result<(), AppError> {
    queue.resume(&job_id).await
}

#[tauri::command]
pub async fn cancel_job(queue: State<'_, DownloadQueue>, job_id: String) -> Result<(), AppError> {
    queue.cancel(&job_id).await
}

#[tauri::command]
pub async fn retry_job(
    queue: State<'_, DownloadQueue>,
    job_id: String,
) -> Result<DownloadJob, AppError> {
    queue.retry(&job_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{MediaSource, SegmentMode, SubtitleOptions, TrimRange};

    /// Preview đúng hình dạng mà `try_music_preview` trả về cho một track đơn.
    fn music_preview() -> MediaSource {
        MediaSource {
            source_url: "https://open.spotify.com/track/x".to_string(),
            title: "Someone – Song".to_string(),
            thumbnail_url: None,
            duration_seconds: Some(215),
            platform: "spotify".to_string(),
            is_playlist: false,
            playlist_item_count: None,
            available_video_qualities: Vec::new(),
            available_audio_formats: Vec::new(),
            is_gallery: false,
            gallery_items: Vec::new(),
            is_music: true,
            available_music_tiers: vec![
                "flac16".to_string(),
                "flac24".to_string(),
                "mp3_320".to_string(),
            ],
            playlist_entries: Vec::new(),
            subtitles: None,
            chapters: None,
        }
    }

    fn music_input(audio_quality: Option<&str>, output_options: Option<OutputOptions>) -> CreateJobInput {
        CreateJobInput {
            source_url: "https://open.spotify.com/track/x".to_string(),
            media_type: MediaTypeInput::Music,
            audio_quality: audio_quality.map(str::to_string),
            video_quality: None,
            gallery_mode: None,
            selected_gallery_indices: None,
            output_directory: "/tmp".to_string(),
            playlist_scope: None,
            title: None,
            output_options,
        }
    }

    #[test]
    fn a_tier_from_the_preview_is_accepted() {
        for tier in ["flac16", "flac24", "mp3_320"] {
            assert!(
                validate_quality(&music_preview(), &music_input(Some(tier), None)).is_ok(),
                "{tier} nằm trong danh sách tier của preview"
            );
        }
    }

    #[test]
    fn a_tier_the_preview_never_offered_is_rejected() {
        // FR-019 áp cho engine mới: backend không tin chuỗi chất lượng frontend
        // gửi mà không đối chiếu preview thật của chính link đó.
        let err = validate_quality(&music_preview(), &music_input(Some("flac999"), None))
            .expect_err("tier lạ phải bị từ chối");
        assert_eq!(err.code, "INVALID_QUALITY_OPTION");

        let err = validate_quality(&music_preview(), &music_input(None, None))
            .expect_err("job nhạc không có tier là không hợp lệ");
        assert_eq!(err.code, "INVALID_QUALITY_OPTION");
    }

    #[test]
    fn a_music_job_against_a_non_music_preview_is_rejected() {
        // Cùng luật với gallery: `media_type` do frontend gửi không được phép
        // mâu thuẫn với engine đã thật sự resolve link này.
        let mut preview = music_preview();
        preview.is_music = false;
        preview.available_music_tiers.clear();

        let err = validate_quality(&preview, &music_input(Some("flac16"), None))
            .expect_err("preview yt-dlp không dựng được job nhạc");
        assert_eq!(err.code, "INVALID_QUALITY_OPTION");
    }

    #[test]
    fn output_options_that_do_not_apply_to_music_are_rejected_instead_of_ignored() {
        // Nhận rồi âm thầm bỏ qua là hứa một thứ không làm: worker tự tag và
        // nhúng bìa, không có chỗ nào cho phụ đề hay cắt đoạn.
        let with_subtitles = OutputOptions {
            subtitles: SubtitleOptions {
                languages: vec!["vi".to_string()],
                ..Default::default()
            },
            ..Default::default()
        };
        let err = validate_quality(
            &music_preview(),
            &music_input(Some("flac16"), Some(with_subtitles)),
        )
        .expect_err("phụ đề không có nghĩa với một file nhạc");
        assert_eq!(err.code, "INVALID_QUALITY_OPTION");

        let with_trim = OutputOptions {
            segment: SegmentMode::Trim(TrimRange {
                start_seconds: Some(30.0),
                end_seconds: Some(60.0),
                accurate_cut: false,
            }),
            ..Default::default()
        };
        let err = validate_quality(
            &music_preview(),
            &music_input(Some("flac16"), Some(with_trim)),
        )
        .expect_err("cắt đoạn không đi qua đường tải nhạc");
        assert_eq!(err.code, "INVALID_QUALITY_OPTION");
    }

    #[test]
    fn plain_output_options_still_pass_through_for_music() {
        assert!(validate_quality(
            &music_preview(),
            &music_input(Some("flac24"), Some(OutputOptions::default()))
        )
        .is_ok());
    }
}
