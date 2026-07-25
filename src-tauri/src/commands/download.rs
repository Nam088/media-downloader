use chrono::Utc;
use serde::Deserialize;
use tauri::State;

use crate::commands::media::PreviewCache;
use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;
use crate::models::{DownloadJob, GalleryMode, JobStatus, MediaType};
use crate::platform::detect_platform;

#[derive(Debug, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum MediaTypeInput {
    Audio,
    Video,
    Gallery,
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
    pub output_directory: String,
    #[serde(default)]
    pub playlist_scope: Option<PlaylistScope>,
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
fn validate_quality(preview: &crate::models::MediaSource, input: &CreateJobInput) -> Result<(), AppError> {
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
            if !preview.available_video_qualities.iter().any(|opt| opt.label == quality) {
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
    output_directory: String,
    is_playlist_item: bool,
    parent_playlist_id: Option<String>,
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
        },
        audio_quality: args.audio_quality,
        video_quality: args.video_quality,
        gallery_mode: args.gallery_mode.map(|mode| match mode {
            GalleryModeInput::Files => GalleryMode::Files,
            GalleryModeInput::AudioOnly => GalleryMode::AudioOnly,
            GalleryModeInput::ImagesOnly => GalleryMode::ImagesOnly,
            GalleryModeInput::Slideshow => GalleryMode::Slideshow,
        }),
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
    let preview = cache
        .get(&input.source_url)
        .ok_or_else(|| AppError::new("PREVIEW_REQUIRED", "Call preview_media before creating a job"))?;

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
            let job = new_job(NewJobArgs {
                source_url: entry_url,
                platform,
                media_type: input.media_type,
                audio_quality: None,
                video_quality: None,
                gallery_mode: None,
                output_directory: input.output_directory.clone(),
                is_playlist_item: true,
                parent_playlist_id: Some(parent_playlist_id.clone()),
            });
            queue.enqueue(job.clone()).await?;
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
    let job = new_job(NewJobArgs {
        source_url: input.source_url,
        platform: preview.platform.clone(),
        media_type: input.media_type,
        audio_quality: input.audio_quality,
        video_quality: input.video_quality,
        gallery_mode: input.gallery_mode,
        output_directory: input.output_directory,
        is_playlist_item: false,
        parent_playlist_id: None,
    });
    queue.enqueue(job.clone()).await?;
    Ok(vec![job])
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
