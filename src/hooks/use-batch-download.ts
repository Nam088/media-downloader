import { useCallback, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { buildJobInput } from "@/lib/build-job-input";
import { audioQualityValue } from "@/lib/generic-quality-options";
import { useQueueStore } from "@/stores/queue-store";
import type { CreateJobInput, DownloadJob, MediaSource, MediaType } from "@/types/download";

/**
 * How many links are previewed at the same time.
 *
 * Every preview spawns its own yt-dlp (or gallery-dl) process, so this is a
 * cap on concurrent *processes*, not just on in-flight requests. Four keeps a
 * 20-link paste from taking twenty sequential round-trips without burying a
 * modest machine under twenty extractor processes — or getting the account
 * rate-limited by the source for hammering it.
 */
const PREVIEW_CONCURRENCY = 4;

export type BatchItemStatus = "pending" | "previewing" | "created" | "error";

export interface BatchItem {
  url: string;
  status: BatchItemStatus;
  /** Filled in once the preview succeeded, so the list stops showing raw URLs. */
  title: string | null;
  /** An `AppError.code` the UI can translate; `null` unless `status === "error"`. */
  errorCode: string | null;
}

/**
 * The batch choice is a media *type*, not a quality: quality still comes from
 * each link's own preview (FR-019), because a fixed tier list would be a lie
 * for any source that doesn't offer it.
 */
export type BatchMediaType = Exclude<MediaType, "gallery">;

export interface RunBatchArgs {
  urls: string[];
  mediaType: BatchMediaType;
  outputDirectory: string;
}

export interface BatchSummary {
  created: number;
  failed: number;
}

function errorCodeOf(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error) {
    return String((error as { code: unknown }).code);
  }
  return "DOWNLOAD_FAILED";
}

/**
 * Preview a list of URLs with bounded concurrency and queue a job for each.
 *
 * Deliberately *not* all-or-nothing: one dead link out of twenty must not cost
 * the user the other nineteen, so every URL carries its own status and its own
 * failure reason.
 */
export function useBatchDownload() {
  const [items, setItems] = useState<BatchItem[]>([]);
  const [running, setRunning] = useState(false);
  // Guards against a second run being kicked off from a stale render before
  // `running` has propagated — `running` drives the UI, this drives the logic.
  const inFlight = useRef(false);

  const patchAt = useCallback((index: number, changes: Partial<BatchItem>) => {
    setItems((current) =>
      current.map((item, position) => (position === index ? { ...item, ...changes } : item)),
    );
  }, []);

  const run = useCallback(
    async ({ urls, mediaType, outputDirectory }: RunBatchArgs): Promise<BatchSummary> => {
      if (inFlight.current || urls.length === 0) {
        return { created: 0, failed: 0 };
      }
      inFlight.current = true;
      setRunning(true);
      setItems(urls.map((url) => ({ url, status: "pending", title: null, errorCode: null })));

      const summary: BatchSummary = { created: 0, failed: 0 };
      // A shared cursor rather than fixed slices: a worker that draws a fast
      // link comes straight back for the next one instead of idling while its
      // own slice's slow link finishes. The read-then-increment pair runs
      // without an intervening `await`, so no two workers can claim the same
      // index.
      let cursor = 0;

      const worker = async () => {
        while (cursor < urls.length) {
          const index = cursor;
          cursor += 1;
          const url = urls[index];

          patchAt(index, { status: "previewing" });
          try {
            const preview = await invoke<MediaSource>("preview_media", { sourceUrl: url });

            const input: CreateJobInput = buildJobInput({
              preview,
              mediaType,
              // Best of what *this* link actually published — the first entry
              // of each list is the backend's own best-first ordering.
              audioQuality:
                preview.available_audio_formats.length > 0
                  ? audioQualityValue(preview.available_audio_formats[0].bitrate_kbps)
                  : null,
              videoQuality: preview.available_video_qualities[0]?.label ?? null,
              outputDirectory,
              // Only consulted for a gallery-backed link, which has no
              // audio/video quality concept at all: "audio" means keep just
              // the backing track, "video" means take the files as they are.
              galleryMode: mediaType === "audio" ? "audio_only" : "files",
            });

            const createdJobs = await invoke<DownloadJob[]>("create_download_job", { input });
            useQueueStore.getState().upsertJobs(createdJobs);
            summary.created += 1;
            patchAt(index, { status: "created", title: preview.title });
          } catch (error) {
            summary.failed += 1;
            patchAt(index, { status: "error", errorCode: errorCodeOf(error) });
          }
        }
      };

      await Promise.all(
        Array.from({ length: Math.min(PREVIEW_CONCURRENCY, urls.length) }, () => worker()),
      );

      inFlight.current = false;
      setRunning(false);
      return summary;
    },
    [patchAt],
  );

  const reset = useCallback(() => setItems([]), []);

  return { items, running, run, reset };
}
