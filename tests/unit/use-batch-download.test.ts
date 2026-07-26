import { invoke } from "@tauri-apps/api/core";
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useBatchDownload } from "@/hooks/use-batch-download";
import { useQueueStore } from "@/stores/queue-store";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type { CreateJobInput, DownloadJob, MediaSource } from "@/types/download";

/**
 * A preview whose format lists differ per URL, so a test can tell "quality
 * came from this link's own preview" apart from "quality came from a list
 * baked into the hook".
 */
function previewFor(url: string, overrides: Partial<MediaSource> = {}): MediaSource {
  return {
    source_url: url,
    title: `Title for ${url}`,
    thumbnail_url: null,
    duration_seconds: 100,
    platform: "youtube",
    is_playlist: false,
    playlist_item_count: null,
    available_video_qualities: [{ label: "720p", filesize_bytes: null }],
    available_audio_formats: [{ bitrate_kbps: 128, codec: "opus", filesize_bytes: null }],
    is_gallery: false,
    gallery_items: [],
    playlist_entries: [],
    ...overrides,
  };
}

function jobFor(url: string): DownloadJob {
  return {
    id: `job-${url}`,
    source_url: url,
    platform: "youtube",
    media_type: "audio",
    audio_quality: null,
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "queued",
    progress_percent: 0,
    speed_bytes_per_sec: null,
    eta_seconds: null,
    error_message: null,
    output_directory: "/out",
    output_file_path: null,
    is_playlist_item: false,
    parent_playlist_id: null,
    retried_from_job_id: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    title: null,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
  };
}

/** Every `create_download_job` payload the hook sent, in call order. */
function createdInputs(): CreateJobInput[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "create_download_job")
    .map(([, args]) => (args as { input: CreateJobInput }).input);
}

describe("useBatchDownload (FR-101, FR-102, FR-103)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    useQueueStore.setState({ jobs: {} });
  });

  function mockPreviews(previews: Record<string, MediaSource>) {
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        const { sourceUrl } = args as { sourceUrl: string };
        const preview = previews[sourceUrl];
        if (!preview) return Promise.reject({ code: "NOT_FOUND", message: "no such fixture" });
        return Promise.resolve(preview);
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return Promise.resolve([jobFor(input.source_url)]);
      }
      return Promise.resolve(undefined);
    });
  }

  it("queues video jobs when the user picked video, instead of forcing audio on every link", async () => {
    const urls = ["https://a.example/1", "https://b.example/2"];
    mockPreviews({ [urls[0]]: previewFor(urls[0]), [urls[1]]: previewFor(urls[1]) });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "video", outputDirectory: "/out" });
    });

    const inputs = createdInputs();
    expect(inputs.map((input) => input.media_type)).toEqual(["video", "video"]);
    // Video jobs must not carry an audio quality — that would be the old
    // audio-shaped job wearing a "video" label.
    expect(inputs.map((input) => input.audio_quality)).toEqual([null, null]);
    expect(inputs.map((input) => input.video_quality)).toEqual(["720p", "720p"]);
  });

  it("queues audio jobs when the user picked audio", async () => {
    const urls = ["https://a.example/1"];
    mockPreviews({ [urls[0]]: previewFor(urls[0]) });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(createdInputs()).toEqual([
      expect.objectContaining({
        media_type: "audio",
        audio_quality: "128kbps",
        video_quality: null,
      }),
    ]);
  });

  it("takes each link's quality from that link's own preview, never a shared list (FR-019)", async () => {
    const urls = ["https://hd.example/1", "https://sd.example/2"];
    mockPreviews({
      [urls[0]]: previewFor(urls[0], {
        available_video_qualities: [
          { label: "1080p", filesize_bytes: null },
          { label: "360p", filesize_bytes: null },
        ],
      }),
      [urls[1]]: previewFor(urls[1], {
        available_video_qualities: [{ label: "240p", filesize_bytes: null }],
      }),
    });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "video", outputDirectory: "/out" });
    });

    const byUrl = new Map(createdInputs().map((input) => [input.source_url, input.video_quality]));
    expect(byUrl.get(urls[0])).toBe("1080p");
    expect(byUrl.get(urls[1])).toBe("240p");
  });

  it("keeps going when urls fail to preview", async () => {
    // The first four are broken *and* there are exactly four worker slots, so
    // every worker in the pool takes a failure as its first job. Nothing
    // healthy can be reached unless a failure is contained to the one link
    // that caused it: a worker that dies (or a `Promise.all` that rejects) on
    // the first bad link leaves the last four links untouched.
    const broken = Array.from({ length: 4 }, (_, i) => `https://broken.example/${i}`);
    const healthy = Array.from({ length: 4 }, (_, i) => `https://ok.example/${i}`);
    const urls = [...broken, ...healthy];

    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        const { sourceUrl } = args as { sourceUrl: string };
        if (sourceUrl.includes("broken")) {
          return Promise.reject({ code: "UNSUPPORTED_PLATFORM", message: "nope" });
        }
        return Promise.resolve(previewFor(sourceUrl));
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return Promise.resolve([jobFor(input.source_url)]);
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useBatchDownload());
    let summary = { created: 0, failed: 0 };
    await act(async () => {
      summary = await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(createdInputs().map((input) => input.source_url)).toEqual(healthy);
    expect(summary).toEqual({ created: 4, failed: 4 });
    expect(result.current.items.map((item) => item.status)).toEqual([
      "error",
      "error",
      "error",
      "error",
      "created",
      "created",
      "created",
      "created",
    ]);
    expect(result.current.items[0]).toEqual({
      url: broken[0],
      status: "error",
      title: null,
      errorCode: "UNSUPPORTED_PLATFORM",
    });
    expect(result.current.items[4]).toEqual({
      url: healthy[0],
      status: "created",
      title: `Title for ${healthy[0]}`,
      errorCode: null,
    });
  });

  it("records a failure to create the job, not just a failure to preview", async () => {
    const urls = ["https://a.example/1"];
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        return Promise.resolve(previewFor((args as { sourceUrl: string }).sourceUrl));
      }
      if (cmd === "create_download_job") {
        return Promise.reject({ code: "ACCESS_DENIED", message: "denied" });
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(result.current.items[0].status).toBe("error");
    expect(result.current.items[0].errorCode).toBe("ACCESS_DENIED");
  });

  it("previews several links at once but never more than four", async () => {
    let concurrent = 0;
    let peak = 0;
    const urls = Array.from({ length: 12 }, (_, index) => `https://example.com/${index}`);

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        concurrent += 1;
        peak = Math.max(peak, concurrent);
        // A real yield, so overlapping previews genuinely overlap rather than
        // resolving in the microtask queue one after another.
        await new Promise((resolve) => setTimeout(resolve, 5));
        concurrent -= 1;
        return previewFor((args as { sourceUrl: string }).sourceUrl);
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return [jobFor(input.source_url)];
      }
      return undefined;
    });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    // Exactly four: fewer would mean the pool serialised work it could have
    // overlapped, more would mean the process cap isn't holding.
    expect(peak).toBe(4);
    expect(createdInputs()).toHaveLength(12);
  });

  it("caps at the number of urls when there are fewer than four", async () => {
    let concurrent = 0;
    let peak = 0;
    const urls = ["https://a.example/1", "https://b.example/2"];

    vi.mocked(invoke).mockImplementation(async (cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        concurrent += 1;
        peak = Math.max(peak, concurrent);
        await new Promise((resolve) => setTimeout(resolve, 5));
        concurrent -= 1;
        return previewFor((args as { sourceUrl: string }).sourceUrl);
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return [jobFor(input.source_url)];
      }
      return undefined;
    });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(peak).toBe(2);
  });

  it("turns a gallery-backed link into a gallery job whose mode follows the batch choice", async () => {
    const url = "https://tiktok.com/@a/photo/1";
    const galleryPreview = previewFor(url, {
      is_gallery: true,
      available_video_qualities: [],
      available_audio_formats: [],
      gallery_items: [
        { url: "https://cdn/1.jpg", extension: "jpg", is_audio: false },
        { url: "https://cdn/a.mp3", extension: "mp3", is_audio: true },
      ],
    });
    mockPreviews({ [url]: galleryPreview });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls: [url], mediaType: "audio", outputDirectory: "/out" });
    });
    expect(createdInputs()[0]).toMatchObject({ media_type: "gallery", gallery_mode: "audio_only" });

    vi.mocked(invoke).mockClear();
    await act(async () => {
      await result.current.run({ urls: [url], mediaType: "video", outputDirectory: "/out" });
    });
    expect(createdInputs()[0]).toMatchObject({ media_type: "gallery", gallery_mode: "files" });
  });

  it("puts the created jobs into the queue store so they show up immediately", async () => {
    const urls = ["https://a.example/1", "https://b.example/2"];
    mockPreviews({ [urls[0]]: previewFor(urls[0]), [urls[1]]: previewFor(urls[1]) });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(Object.keys(useQueueStore.getState().jobs).sort()).toEqual([
      `job-${urls[0]}`,
      `job-${urls[1]}`,
    ]);
  });

  // FR-232 — one set of choices for the whole paste. This hook used to call
  // `buildJobInput` with no options at all, which made a pasted list the one
  // path that ignored the picker entirely.
  it("attaches the shared output options to every job in the batch (FR-232)", async () => {
    const urls = ["https://a.example/1", "https://b.example/2"];
    mockPreviews({ [urls[0]]: previewFor(urls[0]), [urls[1]]: previewFor(urls[1]) });
    const outputOptions = { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "flac" } } as const;

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({
        urls,
        mediaType: "audio",
        outputDirectory: "/out",
        outputOptions,
      });
    });

    const inputs = createdInputs();
    expect(inputs).toHaveLength(2);
    for (const input of inputs) {
      expect(input.output_options).toEqual(outputOptions);
    }
  });

  // Omitting them stays a supported call that means today's behaviour, so a
  // caller with no picker is not forced to invent a value.
  it("sends no output options at all when the caller passes none", async () => {
    const urls = ["https://a.example/1"];
    mockPreviews({ [urls[0]]: previewFor(urls[0]) });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({ urls, mediaType: "audio", outputDirectory: "/out" });
    });

    expect(createdInputs()[0]).not.toHaveProperty("output_options");
  });

  // FR-234 — a gallery-backed link runs through gallery-dl, which reads none
  // of these fields; recording them would claim a choice that was ignored.
  it("keeps the output options off a gallery job (FR-234)", async () => {
    const url = "https://tiktok.com/@a/photo/1";
    mockPreviews({
      [url]: previewFor(url, {
        is_gallery: true,
        available_video_qualities: [],
        available_audio_formats: [],
        gallery_items: [{ url: "https://cdn/1.jpg", extension: "jpg", is_audio: false }],
      }),
    });

    const { result } = renderHook(() => useBatchDownload());
    await act(async () => {
      await result.current.run({
        urls: [url],
        mediaType: "audio",
        outputDirectory: "/out",
        outputOptions: NEW_JOB_OUTPUT_OPTIONS,
      });
    });

    expect(createdInputs()[0]).not.toHaveProperty("output_options");
  });

  it("reports running while the batch is in flight and stops when it ends", async () => {
    let releasePreview: (() => void) | null = null;
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "preview_media") {
        return new Promise((resolve) => {
          releasePreview = () =>
            resolve(previewFor((args as { sourceUrl: string }).sourceUrl));
        });
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return Promise.resolve([jobFor(input.source_url)]);
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useBatchDownload());
    let finished: Promise<unknown> | null = null;
    await act(async () => {
      finished = result.current.run({
        urls: ["https://a.example/1"],
        mediaType: "audio",
        outputDirectory: "/out",
      });
      // Let the worker start and mark itself busy before asserting.
      await Promise.resolve();
    });

    expect(result.current.running).toBe(true);

    await act(async () => {
      releasePreview?.();
      await finished;
    });

    expect(result.current.running).toBe(false);
  });
});
