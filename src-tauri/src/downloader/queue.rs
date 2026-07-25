use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex as AsyncMutex, Semaphore};

use crate::db::Db;
use crate::error::AppError;
use crate::logging::log_event;
use crate::models::{DownloadJob, GalleryMode, JobStatus, MediaType};

use super::gallery_dl;
use super::ytdlp;
use super::ytdlp_binary;

const MAX_CONCURRENT_DOWNLOADS: usize = 3;

/// Upper bound on redownload attempts when the output has no audio track
/// (see `ytdlp::output_has_audio_stream`). yt-dlp issue #15891: TikTok's CDN
/// can intermittently serve a video-only file under a format id whose
/// metadata still claims `acodec=aac`; maintainers confirmed re-downloading
/// commonly gets a different, correct file, so a couple of retries is a real
/// fix here, not just a delay.
const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;

#[derive(Clone, serde::Serialize)]
struct JobProgressEvent {
    job_id: String,
    progress_percent: f64,
    speed_bytes_per_sec: Option<i64>,
    eta_seconds: Option<i64>,
}

#[derive(Clone, serde::Serialize)]
struct JobStatusChangedEvent {
    job_id: String,
    status: String,
    error_message: Option<String>,
    /// Only set when `status = completed` — lets the frontend show the
    /// output path immediately without a separate `list_history` round-trip
    /// (that command is added later, in User Story 3).
    output_file_path: Option<String>,
}

/// Tracks the cancel signal for a job currently running (or paused) so
/// `pause_job`/`cancel_job` (T034) can stop the underlying yt-dlp process.
/// yt-dlp cannot be paused mid-flight in a cross-platform way (no portable
/// SIGSTOP-equivalent on Windows), so "pause" is implemented as: stop the
/// process, keep `status = paused`, and `resume_job` re-invokes yt-dlp with
/// `--continue` against the same partial `.part` file.
struct RunningJob {
    cancel_tx: watch::Sender<bool>,
}

pub struct DownloadQueue {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    semaphore: Arc<Semaphore>,
}

impl DownloadQueue {
    pub fn new(db: Arc<Db>, app: AppHandle) -> Self {
        Self {
            db,
            app,
            running: Arc::new(AsyncMutex::new(HashMap::new())),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DOWNLOADS)),
        }
    }

    /// Persists the job as `queued` and spawns its execution in the
    /// background. Returns immediately; progress/completion are reported via
    /// the `job:progress` / `job:status_changed` events (contracts/tauri-commands.md).
    pub async fn enqueue(&self, job: DownloadJob) -> Result<(), AppError> {
        self.db.insert_job(&job)?;
        self.spawn_run(job).await;
        Ok(())
    }

    async fn spawn_run(&self, job: DownloadJob) {
        let db = Arc::clone(&self.db);
        let app = self.app.clone();
        let semaphore = Arc::clone(&self.semaphore);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        {
            let mut running = self.running.lock().await;
            running.insert(job.id.clone(), RunningJob { cancel_tx });
        }

        let job_id = job.id.clone();
        let running_registry = Arc::clone(&self.running);

        tokio::spawn(async move {
            let _permit = semaphore.acquire().await;
            let result = run_job(&app, &db, &job, cancel_rx).await;
            if let Err(err) = result {
                log_event(
                    &app,
                    "ERROR",
                    format!(
                        "Job {} failed ({} — {}): {}",
                        job.id, job.platform, job.source_url, err.message
                    ),
                );
                let _ = db.update_job_status(&job.id, JobStatus::Failed, Some(&err.message));
                emit_status_changed(&app, &job.id, JobStatus::Failed, Some(err.message), None);
            }
            running_registry.lock().await.remove(&job_id);
        });
    }

    pub async fn cancel(&self, job_id: &str) -> Result<(), AppError> {
        let mut running = self.running.lock().await;
        if let Some(handle) = running.remove(job_id) {
            let _ = handle.cancel_tx.send(true);
        }
        self.db.update_job_status(job_id, JobStatus::Canceled, None)?;
        emit_status_changed(&self.app, job_id, JobStatus::Canceled, None, None);
        Ok(())
    }

    pub async fn pause(&self, job_id: &str) -> Result<(), AppError> {
        let mut running = self.running.lock().await;
        if let Some(handle) = running.remove(job_id) {
            let _ = handle.cancel_tx.send(true);
        }
        self.db.update_job_status(job_id, JobStatus::Paused, None)?;
        emit_status_changed(&self.app, job_id, JobStatus::Paused, None, None);
        Ok(())
    }

    pub async fn resume(&self, job_id: &str) -> Result<(), AppError> {
        let job = self
            .db
            .get_job(job_id)?
            .ok_or_else(|| AppError::not_found("Job"))?;
        if job.status != JobStatus::Paused {
            return Err(AppError::new(
                "INVALID_JOB_STATE",
                "Only a paused job can be resumed",
            ));
        }
        self.db
            .update_job_status(job_id, JobStatus::Queued, None)?;
        emit_status_changed(&self.app, job_id, JobStatus::Queued, None, None);
        self.spawn_run(job).await;
        Ok(())
    }

    /// Creates a brand-new job that repeats a failed/canceled one, keeping
    /// `retried_from_job_id` pointing at the original (data-model.md §1) —
    /// the old row is left untouched in history, matching FR-006.
    pub async fn retry(&self, job_id: &str) -> Result<DownloadJob, AppError> {
        let original = self
            .db
            .get_job(job_id)?
            .ok_or_else(|| AppError::not_found("Job"))?;

        let now = Utc::now().to_rfc3339();
        let retried = DownloadJob {
            id: uuid::Uuid::new_v4().to_string(),
            source_url: original.source_url,
            platform: original.platform,
            media_type: original.media_type,
            audio_quality: original.audio_quality,
            video_quality: original.video_quality,
            gallery_mode: original.gallery_mode,
            selected_gallery_indices: original.selected_gallery_indices,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: original.output_directory,
            output_file_path: None,
            is_playlist_item: original.is_playlist_item,
            parent_playlist_id: original.parent_playlist_id,
            retried_from_job_id: Some(job_id.to_string()),
            created_at: now.clone(),
            updated_at: now,
        };

        self.enqueue(retried.clone()).await?;
        Ok(retried)
    }
}

async fn run_job(
    app: &AppHandle,
    db: &Arc<Db>,
    job: &DownloadJob,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<(), AppError> {
    if job.media_type == MediaType::Gallery {
        return run_gallery_job(app, db, job, cancel_rx).await;
    }

    db.update_job_status(&job.id, JobStatus::Downloading, None)?;
    emit_status_changed(app, &job.id, JobStatus::Downloading, None, None);

    let output_template = format!("{}/%(title)s.%(ext)s", job.output_directory);
    let extra_args = build_ytdlp_args(job)?;

    let mut output_path: Option<String> = None;
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        let job_id_for_progress = job.id.clone();
        let app_for_progress = app.clone();
        let db_for_progress = Arc::clone(db);

        let download_fut = ytdlp::run_download(
            app,
            &job.source_url,
            &output_template,
            extra_args.clone(),
            move |update| {
                let _ = db_for_progress.update_job_progress(
                    &job_id_for_progress,
                    update.percent,
                    update.speed_bytes_per_sec,
                    update.eta_seconds,
                );
                emit_progress(&app_for_progress, &job_id_for_progress, &update);
            },
        );

        let download_result = tokio::select! {
            result = download_fut => result,
            _ = cancel_rx.changed() => {
                // Job was paused/canceled; the caller already updated status
                // and emitted the event, so just stop here without treating
                // this as a failure.
                return Ok(());
            }
        };

        // TikTok's CDN can intermittently fail a request outright, not just
        // silently drop audio (yt-dlp issue #15891/#15642 — confirmed by
        // maintainers as server-side inconsistency, never fixed upstream).
        // A fresh attempt often just works, same as a human hitting "retry".
        let path = match download_result {
            Ok(path) => path,
            Err(err) => {
                if attempt < MAX_DOWNLOAD_ATTEMPTS {
                    log_event(
                        app,
                        "WARN",
                        format!(
                            "Job {} attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS} failed, retrying: {}",
                            job.id, err.message
                        ),
                    );
                    continue;
                }
                return Err(err);
            }
        };

        // Only muxed video jobs are at risk of a *silent* audio loss: an
        // audio-only extraction (`-x`) fails loudly instead (ffmpeg can't
        // extract a stream that isn't there), which the branch above already
        // retries.
        if job.media_type == MediaType::Video && !ytdlp::output_has_audio_stream(&path).await {
            if attempt < MAX_DOWNLOAD_ATTEMPTS {
                log_event(
                    app,
                    "WARN",
                    format!("Job {} attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS}: downloaded video had no audio track, retrying", job.id),
                );
                let _ = tokio::fs::remove_file(&path).await;
                continue;
            }
            // Last attempt still missing audio. Per the community workaround
            // documented on the same yt-dlp issues (TikTok can consistently
            // serve a video-only file for a given format on videos where
            // download is disabled, so retrying the identical request isn't
            // guaranteed to help): separately fetch just the best audio
            // track and mux it onto the otherwise-good video, rather than
            // discarding a mostly-fine download over its audio track alone.
            match recover_missing_audio(app, &job.source_url, &path).await {
                Ok(fixed_path) => {
                    output_path = Some(fixed_path);
                    break;
                }
                Err(_) => {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "The source served a video with no audio track after multiple attempts, and no separate audio track could be recovered. Please try again.",
                    ));
                }
            }
        }

        output_path = Some(path);
        break;
    }
    let output_path = output_path.expect("loop always returns via `?`/cancel or sets output_path");

    let metadata = tokio::fs::metadata(&output_path).await.ok();
    let file_size = metadata.map(|m| m.len() as i64).unwrap_or(0);
    let file_format = std::path::Path::new(&output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();

    db.insert_downloaded_file(&job.id, &output_path, &file_format, file_size)?;
    db.set_job_output_file(&job.id, &output_path)?;
    db.update_job_status(&job.id, JobStatus::Completed, None)?;
    emit_status_changed(
        app,
        &job.id,
        JobStatus::Completed,
        None,
        Some(output_path),
    );

    Ok(())
}

/// Fallback per-image duration in `GalleryMode::Slideshow`, used only when
/// the audio track's actual length can't be probed (see
/// `probe_audio_duration_secs`) — normally each image's display time is the
/// audio's total length divided evenly across the image count, clamped to
/// `[MIN_SLIDESHOW_IMAGE_DURATION_SECS, MAX_SLIDESHOW_IMAGE_DURATION_SECS]`,
/// so the slideshow's pacing actually matches the music instead of a fixed
/// 3s/image cadence that runs short or leaves dead air.
const DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 3.0;
const MIN_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 1.5;
const MAX_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 8.0;

/// gallery-dl-backed equivalent of the yt-dlp path above (`MediaType::Gallery`
/// jobs only) — see `research.md` §2's gallery-dl amendment. Downloads every
/// file gallery-dl finds into a job-exclusive subfolder (gallery-dl's `-D`
/// flag has no per-job namespacing of its own, so a shared folder like the
/// user's chosen Downloads directory would otherwise mix unrelated files
/// together), then applies `job.gallery_mode`.
async fn run_gallery_job(
    app: &AppHandle,
    db: &Arc<Db>,
    job: &DownloadJob,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<(), AppError> {
    db.update_job_status(&job.id, JobStatus::Downloading, None)?;
    emit_status_changed(app, &job.id, JobStatus::Downloading, None, None);

    let gallery_mode = job.gallery_mode.clone().ok_or_else(|| {
        AppError::new("MISSING_QUALITY", "gallery_mode is required for gallery downloads")
    })?;

    // Re-dump (cheap: `--no-download`) rather than trusting the cached
    // preview — gives an accurate `total_files` for progress percent and a
    // human-readable folder name, using the exact same data the download
    // itself is about to act on.
    //
    // Retried like the yt-dlp path (`MAX_DOWNLOAD_ATTEMPTS`): TikTok's
    // bot-detection can 403 a gallery-dl request outright (confirmed live —
    // same platform-side flakiness already documented for yt-dlp, issues
    // #15891/#15642). Oddly, gallery-dl's `--dump-json` mode treats that as
    // *non-fatal* — it logs the error but still exits 0 with an empty `[]`
    // — while an actual download of the same blocked URL exits with a real
    // error. So both an outright `Err` here AND a successful-but-empty dump
    // are treated as a failed attempt worth retrying.
    let mut dump: Option<gallery_dl::GalleryDump> = None;
    let mut last_dump_err: Option<AppError> = None;
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        match gallery_dl::dump_gallery_json(app, &job.source_url, |_child| {}).await {
            Ok(d) if !d.entries.is_empty() => {
                dump = Some(d);
                break;
            }
            Ok(d) if attempt == MAX_DOWNLOAD_ATTEMPTS => dump = Some(d),
            Ok(_) => {
                log_event(
                    app,
                    "WARN",
                    format!(
                        "Gallery job {} dump attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS} found nothing, retrying",
                        job.id
                    ),
                );
                continue;
            }
            Err(err) if attempt == MAX_DOWNLOAD_ATTEMPTS => last_dump_err = Some(err),
            Err(err) => {
                log_event(
                    app,
                    "WARN",
                    format!(
                        "Gallery job {} dump attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS} errored, retrying: {}",
                        job.id, err.message
                    ),
                );
                continue;
            }
        }
    }
    let dump = dump.ok_or_else(|| {
        last_dump_err.unwrap_or_else(|| {
            AppError::new(
                "DOWNLOAD_FAILED",
                "gallery-dl found nothing for this link after multiple attempts — the source may be blocking automated requests. Please try again",
            )
        })
    })?;
    // Narrow to the user's selection (checkbox grid in the gallery preview),
    // if one was made, via gallery-dl's own `--range` (item numbers in its
    // own 1-based crawl order). Matched by *ordinal position*, not URL — see
    // `models::DownloadJob.selected_gallery_indices`'s doc comment for why:
    // a site's own item order for a given, unchanged post is stable across
    // separate crawls even when its per-item URLs aren't (TikTok serves
    // fresh, short-lived, signed CDN URLs every crawl, but the same items in
    // the same order). The audio track's own index is always included
    // regardless of what was selected — this only ever restricts which
    // *images* get fetched; whether audio ends up kept is entirely
    // `gallery_mode`'s call (`AudioOnly`/`Slideshow` need it,
    // `Files`/`ImagesOnly` keep or drop it after the fact).
    let resolved_indices: Option<Vec<usize>> = job.selected_gallery_indices.as_ref().map(|selected| {
        dump.entries
            .iter()
            .enumerate()
            .filter(|(i, entry)| {
                let is_audio = entry.extension.as_deref().map(gallery_dl::is_audio_extension).unwrap_or(false);
                is_audio || selected.contains(&(*i as u32))
            })
            .map(|(i, _)| i)
            .collect()
    });
    // Nothing usable to narrow to (an empty selection would otherwise
    // silently download zero images), or the selection already covers
    // everything — either way, no `--range` restriction at all.
    let resolved_indices =
        resolved_indices.filter(|indices| !indices.is_empty() && indices.len() < dump.entries.len());
    let range: Option<String> = resolved_indices.as_ref().map(|indices| {
        indices
            .iter()
            .map(|i| (i + 1).to_string()) // gallery-dl's --range is 1-based
            .collect::<Vec<_>>()
            .join(",")
    });
    let total_files = resolved_indices.map(|indices| indices.len()).unwrap_or(dump.entries.len()).max(1) as u32;

    let folder_label = dump
        .title
        .clone()
        .unwrap_or_else(|| format!("{} gallery", job.platform));
    let job_dir = format!(
        "{}/{} ({})",
        job.output_directory,
        sanitize_path_component(&folder_label),
        &job.id[..8],
    );
    tokio::fs::create_dir_all(&job_dir).await.map_err(AppError::internal)?;

    let mut downloaded_files: Option<Vec<String>> = None;
    for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS {
        let job_id_for_progress = job.id.clone();
        let app_for_progress = app.clone();
        let db_for_progress = Arc::clone(db);

        let download_fut = gallery_dl::run_gallery_download(
            app,
            &job.source_url,
            range.as_deref(),
            &job_dir,
            total_files,
            move |update| {
                let percent = (update.completed_files as f64 / update.total_files as f64) * 100.0;
                let _ = db_for_progress.update_job_progress(&job_id_for_progress, percent, None, None);
                emit_progress(
                    &app_for_progress,
                    &job_id_for_progress,
                    &ytdlp::ProgressUpdate {
                        percent,
                        speed_bytes_per_sec: None,
                        eta_seconds: None,
                    },
                );
            },
        );

        let download_result = tokio::select! {
            result = download_fut => result,
            _ = cancel_rx.changed() => return Ok(()),
        };

        match download_result {
            Ok(files) => {
                downloaded_files = Some(files);
                break;
            }
            Err(err) => {
                if attempt < MAX_DOWNLOAD_ATTEMPTS {
                    log_event(
                        app,
                        "WARN",
                        format!(
                            "Gallery job {} attempt {attempt}/{MAX_DOWNLOAD_ATTEMPTS} failed, retrying: {}",
                            job.id, err.message
                        ),
                    );
                    continue;
                }
                return Err(err);
            }
        }
    }
    let downloaded_files = downloaded_files.expect("loop always returns via `?`/cancel or sets downloaded_files");

    let (audio_paths, image_paths): (Vec<String>, Vec<String>) =
        downloaded_files.into_iter().partition(|path| gallery_dl::is_audio_file_path(path));

    let output_path = match gallery_mode {
        GalleryMode::Files => job_dir.clone(),
        GalleryMode::AudioOnly => {
            for image_path in &image_paths {
                let _ = tokio::fs::remove_file(image_path).await;
            }
            match audio_paths.as_slice() {
                [] => {
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "No audio track was found for this gallery post",
                    ))
                }
                [single] => single.clone(),
                _ => job_dir.clone(),
            }
        }
        GalleryMode::ImagesOnly => {
            for audio_path in &audio_paths {
                let _ = tokio::fs::remove_file(audio_path).await;
            }
            match image_paths.as_slice() {
                [] => {
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "No images were found for this gallery post",
                    ))
                }
                [single] => single.clone(),
                _ => job_dir.clone(),
            }
        }
        GalleryMode::Slideshow => {
            if image_paths.is_empty() || audio_paths.is_empty() {
                return Err(AppError::new(
                    "DOWNLOAD_FAILED",
                    "Slideshow mode needs at least one image and one audio track",
                ));
            }
            let merged_path = merge_gallery_slideshow(&job_dir, &image_paths, &audio_paths[0]).await?;
            for path in image_paths.iter().chain(audio_paths.iter()) {
                let _ = tokio::fs::remove_file(path).await;
            }
            merged_path
        }
    };

    let metadata = tokio::fs::metadata(&output_path).await.ok();
    let file_size = metadata.map(|m| m.len() as i64).unwrap_or(0);
    let file_format = std::path::Path::new(&output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();

    db.insert_downloaded_file(&job.id, &output_path, &file_format, file_size)?;
    db.set_job_output_file(&job.id, &output_path)?;
    db.update_job_status(&job.id, JobStatus::Completed, None)?;
    emit_status_changed(app, &job.id, JobStatus::Completed, None, Some(output_path));

    Ok(())
}

/// Strips characters invalid in a filename on at least one of
/// Windows/macOS/Linux, so a post title/caption (which may contain anything)
/// is always safe to use as a folder name.
fn sanitize_path_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    let truncated: String = trimmed.chars().take(80).collect();
    if truncated.is_empty() {
        "gallery".to_string()
    } else {
        truncated
    }
}

/// Crossfade duration between consecutive images in `GalleryMode::Slideshow`
/// — TikTok's own slideshow posts use a horizontal slide, not a hard cut.
const SLIDESHOW_TRANSITION_SECS: f64 = 0.5;

/// Canvas every image is scaled/padded onto — skips the reference
/// implementation's dynamic first-image-dimension detection (which needs
/// `ffprobe`, a binary this project doesn't bundle) in favor of a fixed
/// default that already matches the near-universal aspect ratio of the
/// slideshow posts this targets.
const SLIDESHOW_CANVAS: (u32, u32) = (1080, 1920);

/// Reads the audio track's real duration via ffmpeg's own stderr banner
/// (`  Duration: 00:00:12.34, start: ...`) — no `ffprobe` needed (this
/// project doesn't bundle it). `ffmpeg -i <file>` always prints this line
/// once it's parsed the input, even with no output specified, so this just
/// discards everything ffmpeg would otherwise fail on past that point.
async fn probe_audio_duration_secs(ffmpeg_path: &std::path::Path, audio_path: &str) -> Option<f64> {
    let output = tokio::process::Command::new(ffmpeg_path)
        .args(["-i", audio_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .ok()?;
    parse_ffmpeg_duration_line(&String::from_utf8_lossy(&output.stderr))
}

/// Pure parsing half of `probe_audio_duration_secs`, split out so it's
/// testable without actually spawning ffmpeg.
fn parse_ffmpeg_duration_line(stderr: &str) -> Option<f64> {
    let line = stderr.lines().find(|l| l.trim_start().starts_with("Duration:"))?;
    let hms = line.trim_start().strip_prefix("Duration:")?.split(',').next()?.trim();
    let mut parts = hms.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds).filter(|d| *d > 0.0)
}

/// Divides the audio's real length evenly across the image count, clamped
/// to a sane per-image range — falls back to
/// `DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS` when the audio's duration
/// couldn't be probed at all. `image_count` is assumed `>= 1` (callers only
/// reach this with at least one image — `GalleryMode::Slideshow` requires
/// it).
fn compute_image_duration_secs(probed_audio_secs: Option<f64>, image_count: usize) -> f64 {
    match probed_audio_secs {
        Some(total) => (total / image_count as f64)
            .clamp(MIN_SLIDESHOW_IMAGE_DURATION_SECS, MAX_SLIDESHOW_IMAGE_DURATION_SECS),
        None => DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS,
    }
}

/// ffmpeg `xfade` transition used between every consecutive pair of images —
/// a single consistent right-to-left slide (not alternated per pair), same
/// direction throughout the whole slideshow.
const SLIDESHOW_TRANSITION: &str = "slideleft";

/// Merges downloaded gallery images + one audio track into a single
/// slideshow video via ffmpeg's `xfade` filter, crossfading each image into
/// the next with a horizontal slide (`SLIDESHOW_TRANSITION`, the same
/// direction for every pair) rather than a hard cut, matching TikTok's own
/// slideshow transition style (verified manually against real sample images
/// before wiring in — `xfade` produces a genuine sliding transition, not a
/// plain dissolve).
///
/// The audio track is never trimmed: each image's display time is the
/// audio's own real length (probed via `probe_audio_duration_secs`) divided
/// evenly across the image count — not a fixed per-image duration, which
/// would either run past the music (dead air) or, combined with `-shortest`,
/// silently truncate the audio early to match a shorter slideshow. `-shortest`
/// stays only as a rounding-error safety net now that the two are computed to
/// already match.
async fn merge_gallery_slideshow(
    job_dir: &str,
    image_paths: &[String],
    audio_path: &str,
) -> Result<String, AppError> {
    let (canvas_w, canvas_h) = SLIDESHOW_CANVAS;
    let scale_pad = format!(
        "scale={canvas_w}:{canvas_h}:force_original_aspect_ratio=decrease,pad={canvas_w}:{canvas_h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=25"
    );

    let audio_file_name = std::path::Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::internal("gallery audio track has no filename"))?
        .to_string();
    let output_file_name = "slideshow.mp4";
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path()?;

    let probed_audio_secs = probe_audio_duration_secs(&ffmpeg_path, audio_path).await;
    let image_duration_secs = compute_image_duration_secs(probed_audio_secs, image_paths.len());
    // The transition borrows time from both the clip it leaves and the one
    // it enters, so it must stay well under a single image's own display
    // time — otherwise a very short per-image duration (many images, short
    // audio) would make consecutive transitions overlap each other.
    let transition_secs = SLIDESHOW_TRANSITION_SECS.min(image_duration_secs * 0.3);

    let mut cmd = tokio::process::Command::new(&ffmpeg_path);
    cmd.current_dir(job_dir).arg("-y");

    // Each image needs to stay on-screen for its own display time PLUS the
    // transition it crossfades into the next one with (xfade consumes that
    // much of both clips' tails/heads to blend them) — otherwise the
    // transition would eat into black/nothing past the loop's own duration.
    let clip_duration = image_duration_secs + transition_secs;
    for image_path in image_paths {
        let file_name = std::path::Path::new(image_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::internal("gallery image has no filename"))?;
        cmd.args(["-loop", "1", "-t", &clip_duration.to_string(), "-i", file_name]);
    }
    cmd.args(["-i", &audio_file_name]);

    if image_paths.len() == 1 {
        cmd.args(["-vf", &scale_pad]);
    } else {
        let mut filter = String::new();
        for i in 0..image_paths.len() {
            filter.push_str(&format!("[{i}:v]{scale_pad}[v{i}];"));
        }
        let mut last_label = "v0".to_string();
        let mut offset = image_duration_secs;
        for i in 1..image_paths.len() {
            let out_label = if i == image_paths.len() - 1 {
                "vout".to_string()
            } else {
                format!("vx{i}")
            };
            filter.push_str(&format!(
                "[{last_label}][v{i}]xfade=transition={SLIDESHOW_TRANSITION}:duration={transition_secs}:offset={offset}[{out_label}];"
            ));
            last_label = out_label;
            offset += image_duration_secs;
        }
        filter.pop(); // trailing ';'
        cmd.args(["-filter_complex", &filter, "-map", "[vout]"]);
    }

    let audio_input_index = image_paths.len();
    cmd.args([
        "-map",
        &format!("{audio_input_index}:a"),
        "-c:v",
        "libx264",
        "-r",
        "25",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        output_file_name,
    ]);

    let status = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(AppError::internal)?;

    if !status.success() {
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Failed to merge slideshow images and audio into a video",
        ));
    }

    Ok(format!("{job_dir}/{output_file_name}"))
}

/// Last-resort recovery for the documented yt-dlp/TikTok audio-loss bug
/// (yt-dlp issues #15891, #15642): separately fetch just the best audio
/// track for the same URL and mux it onto the already-downloaded (but
/// audio-less) video in place. This mirrors the workaround multiple
/// reporters on those issues converged on independently — after
/// maintainers confirmed TikTok's CDN itself serves inconsistent media
/// (the same format id sometimes has audio, sometimes doesn't, despite
/// identical metadata) and closed both issues with no code fix in yt-dlp —
/// since re-requesting the *exact same* video format isn't guaranteed to
/// get a different result (notably on videos where TikTok disables
/// downloads, where it was reported as consistently silent), but a
/// differently-scoped audio-only request has an independent chance of
/// landing on a working response.
async fn recover_missing_audio(
    app: &AppHandle,
    source_url: &str,
    video_path: &str,
) -> Result<String, AppError> {
    let audio_template = format!("{video_path}.audio-only.%(ext)s");
    let audio_args = vec![
        "--no-playlist".to_string(),
        "-f".to_string(),
        "bestaudio/best".to_string(),
    ];
    let audio_path = ytdlp::run_download(app, source_url, &audio_template, audio_args, |_| {}).await?;

    if !ytdlp::output_has_audio_stream(&audio_path).await {
        let _ = tokio::fs::remove_file(&audio_path).await;
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Recovered audio track has no audio either",
        ));
    }

    let muxed_path = format!("{video_path}.muxed.mp4");
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path()?;
    let status = tokio::process::Command::new(&ffmpeg_path)
        .args([
            "-y",
            "-i",
            video_path,
            "-i",
            &audio_path,
            "-map",
            "0:v:0",
            "-map",
            "1:a:0",
            "-c",
            "copy",
            &muxed_path,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(AppError::internal)?;

    let _ = tokio::fs::remove_file(&audio_path).await;

    if !status.success() {
        let _ = tokio::fs::remove_file(&muxed_path).await;
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Failed to mux recovered audio into video",
        ));
    }

    tokio::fs::rename(&muxed_path, video_path)
        .await
        .map_err(AppError::internal)?;
    Ok(video_path.to_string())
}

fn build_ytdlp_args(job: &DownloadJob) -> Result<Vec<String>, AppError> {
    // `--no-playlist` on every single-item job (audio or video) is a
    // deliberate safety net for FR-013: a URL copied from inside a playlist
    // often still carries a `&list=...` param, and without this flag yt-dlp
    // would silently download the whole playlist instead of just this item.
    // Jobs created for a confirmed `entire_playlist` fan-out (T033) are each
    // their own per-entry URL, so this flag has no effect on them either way.
    let mut args = vec!["--no-playlist".to_string()];

    match job.media_type {
        MediaType::Audio => {
            // Explicit `-f` matters here: without it, yt-dlp's default
            // selector is `bestvideo*+bestaudio/best`, which tries to pick
            // separate "best video" and "best audio" candidates and merge
            // them. On sites like TikTok where every format is already a
            // muxed video+audio stream (no dedicated audio-only track), that
            // default can pick two *different* pre-muxed formats and merge
            // them incorrectly, producing a file with no audio track at all
            // once `-x` extracts from it. `bestaudio/best` tells yt-dlp to
            // just take the single best format that actually has audio
            // (preferring a real audio-only stream when one exists) instead
            // of attempting an unnecessary — and here, broken — merge.
            args.push("-f".into());
            args.push("bestaudio/best".into());
            args.push("-x".into());
            args.push("--audio-format".into());
            args.push("mp3".into());
            args.push("--audio-quality".into());
            args.push(match job.audio_quality.as_deref() {
                Some(quality) => {
                    let bitrate_kbps = parse_leading_number(quality).ok_or_else(|| {
                        AppError::new(
                            "INVALID_QUALITY_OPTION",
                            format!("Cannot parse audio quality: {quality}"),
                        )
                    })?;
                    format!("{bitrate_kbps}K")
                }
                // No quality was validated against a real format list (playlist
                // fan-out items skip that step — see download.rs), so ask
                // yt-dlp for its own best available VBR encoding instead of
                // guessing a bitrate.
                None => "0".to_string(),
            });
        }
        MediaType::Video => {
            let height = job
                .video_quality
                .as_deref()
                .map(|quality| {
                    parse_leading_number(quality).ok_or_else(|| {
                        AppError::new(
                            "INVALID_QUALITY_OPTION",
                            format!("Cannot parse video quality: {quality}"),
                        )
                    })
                })
                .transpose()?;
            args.push("-f".into());
            args.push(video_format_selector(height));
            args.push("--merge-output-format".into());
            args.push("mp4".into());
            // TikTok's audio-loss bug (yt-dlp issues #15891/#15642) was
            // reported far more often on `bytevc1`/h265 formats than h264 —
            // this is the community-confirmed mitigation (`-S "vcodec:avc"`)
            // layered on top of `video_format_selector`'s own avc1-first `-f`
            // chain, so a tied fallback still leans h264 instead of h265.
            args.push("--format-sort".into());
            args.push("vcodec:avc".into());
        }
        // `run_job` branches to `run_gallery_job` (a completely separate,
        // gallery-dl-backed code path) before this function is ever called
        // for a gallery job — this arm only exists so the match stays
        // exhaustive if that invariant is ever broken.
        MediaType::Gallery => {
            return Err(AppError::internal(
                "build_ytdlp_args called for a MediaType::Gallery job",
            ))
        }
    }

    args.push("--continue".into());
    Ok(args)
}

/// Builds a `-f` format selector that prioritizes H.264 video (`avc1`) +
/// AAC audio (`mp4a`) — the codec pair virtually every player can decode
/// inside an MP4 container. Left unconstrained, yt-dlp's plain "bestvideo"
/// commonly resolves to VP9/AV1 + Opus on sites like YouTube (better
/// compression, but QuickTime, older Windows Media Player, and many TVs/
/// mobile players can't decode VP9 or Opus muxed into `.mp4`), producing a
/// file that "downloads fine" but won't actually play. Falls back to
/// whatever's best if this exact quality has no H.264 rendition (rare, e.g.
/// some 4K/8K sources are AV1-only) so the download still succeeds instead
/// of failing outright — just not with the compatibility guarantee.
fn video_format_selector(height: Option<u32>) -> String {
    match height {
        Some(h) => format!(
            "bestvideo[vcodec^=avc1][height<={h}]+bestaudio[acodec^=mp4a]/\
             best[vcodec^=avc1][height<={h}]/\
             bestvideo[height<={h}]+bestaudio/best[height<={h}]"
        ),
        None => "bestvideo[vcodec^=avc1]+bestaudio[acodec^=mp4a]/\
                 best[vcodec^=avc1]/bestvideo+bestaudio/best"
            .to_string(),
    }
}

/// Extracts the leading integer from labels like `"128kbps"` or `"1080p"`.
fn parse_leading_number(label: &str) -> Option<u32> {
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn emit_progress(app: &AppHandle, job_id: &str, update: &ytdlp::ProgressUpdate) {
    let _ = app.emit(
        "job:progress",
        JobProgressEvent {
            job_id: job_id.to_string(),
            progress_percent: update.percent,
            speed_bytes_per_sec: update.speed_bytes_per_sec,
            eta_seconds: update.eta_seconds,
        },
    );
}

fn emit_status_changed(
    app: &AppHandle,
    job_id: &str,
    status: JobStatus,
    error_message: Option<String>,
    output_file_path: Option<String>,
) {
    let _ = app.emit(
        "job:status_changed",
        JobStatusChangedEvent {
            job_id: job_id.to_string(),
            status: status.as_str().to_string(),
            error_message,
            output_file_path,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_ffmpeg_duration_banner() {
        let stderr = "  Duration: 00:00:17.30, start: 0.025057, bitrate: 64 kb/s\n";
        assert_eq!(parse_ffmpeg_duration_line(stderr), Some(17.3));
    }

    #[test]
    fn parses_hours_and_minutes_correctly() {
        let stderr = "  Duration: 01:02:03.50, start: 0.000000, bitrate: 128 kb/s\n";
        let expected = 1.0 * 3600.0 + 2.0 * 60.0 + 3.5;
        assert_eq!(parse_ffmpeg_duration_line(stderr), Some(expected));
    }

    #[test]
    fn returns_none_when_no_duration_line_is_present() {
        assert_eq!(parse_ffmpeg_duration_line("some unrelated ffmpeg output\n"), None);
    }

    #[test]
    fn returns_none_for_an_unparseable_or_zero_duration() {
        assert_eq!(parse_ffmpeg_duration_line("  Duration: N/A, bitrate: N/A\n"), None);
        assert_eq!(
            parse_ffmpeg_duration_line("  Duration: 00:00:00.00, start: 0, bitrate: 0 kb/s\n"),
            None
        );
    }

    #[test]
    fn image_duration_divides_the_real_audio_length_evenly_instead_of_a_fixed_cadence() {
        // Regression test: the slideshow used to show every image for a
        // fixed 3s regardless of the audio's actual length, which either
        // left dead air past the music or — combined with `-shortest` —
        // silently truncated the audio early to match a shorter video.
        assert_eq!(compute_image_duration_secs(Some(17.3), 4), 17.3 / 4.0);
    }

    #[test]
    fn image_duration_falls_back_to_the_default_when_audio_length_is_unknown() {
        assert_eq!(compute_image_duration_secs(None, 4), DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS);
    }

    #[test]
    fn image_duration_clamps_to_a_sane_range_for_extreme_ratios() {
        // Many images sharing a short track: would otherwise be far too
        // fast to actually look at.
        assert_eq!(compute_image_duration_secs(Some(5.0), 20), MIN_SLIDESHOW_IMAGE_DURATION_SECS);
        // One image against a very long track: would otherwise show a
        // single static image for minutes.
        assert_eq!(compute_image_duration_secs(Some(120.0), 1), MAX_SLIDESHOW_IMAGE_DURATION_SECS);
    }

    fn sample_job(media_type: MediaType, audio_quality: Option<&str>, video_quality: Option<&str>) -> DownloadJob {
        DownloadJob {
            id: "job-1".into(),
            source_url: "https://youtube.com/watch?v=abc".into(),
            platform: "youtube".into(),
            media_type,
            audio_quality: audio_quality.map(String::from),
            video_quality: video_quality.map(String::from),
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: "/tmp".into(),
            output_file_path: None,
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn audio_args_use_selected_bitrate_not_a_hardcoded_constant() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job).unwrap();
        assert!(args.contains(&"128K".to_string()));

        let job_high = sample_job(MediaType::Audio, Some("320kbps"), None);
        let args_high = build_ytdlp_args(&job_high).unwrap();
        assert!(args_high.contains(&"320K".to_string()));
    }

    #[test]
    fn audio_downloads_explicitly_select_bestaudio_instead_of_the_ambiguous_default() {
        // Regression test: without an explicit `-f`, yt-dlp's default
        // `bestvideo*+bestaudio` selector can pick two different pre-muxed
        // formats on sites like TikTok (every format has both video and
        // audio, no dedicated audio-only stream) and merge them incorrectly,
        // producing a file with no audio track once `-x` extracts from it.
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job).unwrap();
        let f_index = args.iter().position(|a| a == "-f").expect("-f flag present");
        assert_eq!(args[f_index + 1], "bestaudio/best");
    }

    #[test]
    fn video_args_select_nearest_available_height_via_format_selector() {
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = build_ytdlp_args(&job).unwrap();
        let format_selector = args
            .iter()
            .find(|a| a.contains("bestvideo"))
            .expect("format selector arg present");
        // `height<=1080` lets yt-dlp itself fall back to the closest lower
        // resolution when the source doesn't have exactly 1080p (US2
        // Acceptance Scenario 2), instead of us hard-coding a fallback list.
        assert!(format_selector.contains("height<=1080"));
    }

    #[test]
    fn video_args_prefer_h264_aac_so_the_mp4_actually_plays() {
        // Regression test: unconstrained "bestvideo" commonly resolves to
        // VP9/AV1 + Opus on YouTube, which plays fine in VLC/browsers but
        // fails to open in QuickTime, older Windows Media Player, and many
        // TVs when muxed into .mp4 — the file "downloads" but won't play.
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = build_ytdlp_args(&job).unwrap();
        let format_selector = args
            .iter()
            .find(|a| a.contains("bestvideo"))
            .expect("format selector arg present");
        assert!(
            format_selector.starts_with("bestvideo[vcodec^=avc1]"),
            "must try H.264 (avc1) first for MP4 player compatibility: {format_selector}"
        );
        assert!(format_selector.contains("acodec^=mp4a"));
        // ...but must still fall back to non-H.264 rather than fail the
        // download outright when this quality has no avc1 rendition.
        assert!(format_selector.ends_with("best[height<=1080]"));
    }

    #[test]
    fn video_args_include_format_sort_preferring_avc_for_tied_fallbacks() {
        // Regression test for yt-dlp issues #15891/#15642: TikTok's
        // audio-loss bug was reported far more often on h265 (`bytevc1`)
        // than h264 formats. `video_format_selector`'s `-f` chain already
        // tries avc1 first, but `--format-sort vcodec:avc` additionally
        // biases any tied fallback candidate towards h264 too.
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = build_ytdlp_args(&job).unwrap();
        let sort_index = args
            .iter()
            .position(|a| a == "--format-sort")
            .expect("--format-sort flag present");
        assert_eq!(args[sort_index + 1], "vcodec:avc");
    }

    #[test]
    fn missing_quality_falls_back_to_best_for_playlist_fanout_items() {
        // Playlist entries (T033) skip per-item quality validation since
        // flat-playlist previews don't fetch per-video formats — `None`
        // means "let yt-dlp pick its best", not an error.
        let audio_job = sample_job(MediaType::Audio, None, None);
        let audio_args = build_ytdlp_args(&audio_job).unwrap();
        assert!(audio_args.contains(&"0".to_string()));

        let video_job = sample_job(MediaType::Video, None, None);
        let video_args = build_ytdlp_args(&video_job).unwrap();
        assert!(video_args
            .iter()
            .any(|a| a.ends_with("bestvideo+bestaudio/best")));
    }

    #[test]
    fn every_single_item_job_disables_implicit_playlist_download() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job).unwrap();
        assert_eq!(args.first(), Some(&"--no-playlist".to_string()));
    }

    #[test]
    fn build_ytdlp_args_refuses_a_gallery_job_defensively() {
        // run_job branches to run_gallery_job before this is ever reached in
        // practice — this just guards the invariant.
        let job = sample_job(MediaType::Gallery, None, None);
        assert!(build_ytdlp_args(&job).is_err());
    }

    #[test]
    fn sanitize_path_component_strips_characters_invalid_as_a_filename() {
        let cleaned = sanitize_path_component("Cool: Post? / Title \\ <weird>");
        assert!(!cleaned.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
    }

    #[test]
    fn sanitize_path_component_falls_back_when_everything_gets_stripped() {
        assert_eq!(sanitize_path_component("////"), "gallery");
    }

    #[test]
    fn sanitize_path_component_truncates_very_long_titles() {
        let long_title = "a".repeat(500);
        assert!(sanitize_path_component(&long_title).len() <= 80);
    }
}
