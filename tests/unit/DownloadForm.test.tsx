import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { DownloadForm } from "@/components/DownloadForm";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { CreateJobInput, DownloadJob, MediaSource } from "@/types/download";
import type { AppSettings } from "@/types/settings";

// Mirrors the backend's own defaults (see `Db::get_settings`) so these tests
// exercise the form as a fresh install would render it.
const SAMPLE_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "",
  show_logs_tab: false,
  max_concurrent_downloads: 3,
  rate_limit_kbps: 0,
  max_retry_attempts: 3,
  run_in_background: false,
} satisfies AppSettings;

const SAMPLE_PREVIEW: MediaSource = {
  source_url: "https://youtube.com/watch?v=abc",
  title: "Sample video",
  thumbnail_url: null,
  duration_seconds: 120,
  platform: "youtube",
  is_playlist: false,
  playlist_item_count: null,
  available_video_qualities: [
    { label: "720p", filesize_bytes: null },
    { label: "480p", filesize_bytes: null },
  ],
  available_audio_formats: [
    { bitrate_kbps: 160, codec: "opus", filesize_bytes: 2_400_000 },
    { bitrate_kbps: 70, codec: "opus", filesize_bytes: 1_100_000 },
  ],
  // A single yt-dlp-backed video: not gallery-dl's, and not a playlist, so
  // both of those lists are empty for every case in this file.
  is_gallery: false,
  gallery_items: [],
  playlist_entries: [],
};

/** A settings row with a save folder already chosen, so the batch run button
 * isn't blocked on picking one. */
const SETTINGS_WITH_FOLDER: AppSettings = {
  ...SAMPLE_SETTINGS,
  default_output_directory: "/downloads",
};

function jobFor(url: string): DownloadJob {
  return {
    id: `job-${url}`,
    source_url: url,
    platform: "youtube",
    media_type: "video",
    audio_quality: null,
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "queued",
    progress_percent: 0,
    speed_bytes_per_sec: null,
    eta_seconds: null,
    error_message: null,
    output_directory: "/downloads",
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

function mockGetSettingsThenPreview(preview: MediaSource) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(SAMPLE_SETTINGS);
    if (cmd === "preview_media") return Promise.resolve(preview);
    return Promise.resolve(undefined);
  });
}

/** Settings + a working preview/create pair, for the batch-mode cases. */
function mockBatchBackend() {
  vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
    if (cmd === "get_settings") return Promise.resolve(SETTINGS_WITH_FOLDER);
    if (cmd === "preview_media") {
      const { sourceUrl } = args as { sourceUrl: string };
      return Promise.resolve({ ...SAMPLE_PREVIEW, source_url: sourceUrl, title: `Clip ${sourceUrl}` });
    }
    if (cmd === "create_download_job") {
      const { input } = args as { input: CreateJobInput };
      return Promise.resolve([jobFor(input.source_url)]);
    }
    return Promise.resolve(undefined);
  });
}

function createdInputs(): CreateJobInput[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "create_download_job")
    .map(([, args]) => (args as { input: CreateJobInput }).input);
}

describe("DownloadForm", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(SAMPLE_SETTINGS);
    vi.mocked(openDialog).mockReset();
    vi.mocked(openDialog).mockResolvedValue(null);
    useQueueStore.setState({ jobs: {} });
    // `ensureLoaded` caches its fetch promise for the lifetime of the module,
    // so without this every test after the first would silently reuse the
    // first one's settings — including its empty default save folder.
    useSettingsStore.setState({ settings: null, loading: true, fetchPromise: null });
  });

  it("shows the batch-mode hint once 2+ links are pasted (FR-001)", async () => {
    const user = userEvent.setup();
    render(<DownloadForm />);

    const textarea = screen.getByLabelText(/video or audio link/i);
    await user.type(textarea, "https://youtube.com/1\nhttps://youtube.com/2");

    expect(screen.getByText(/2 links detected/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /download 2 links/i })).toBeInTheDocument();
  });

  it("queues video jobs for the whole batch when the user picks video (FR-101)", async () => {
    mockBatchBackend();
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2",
    );
    await user.click(screen.getByRole("radio", { name: /full video/i }));
    await user.click(screen.getByRole("button", { name: /download 2 links/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(2));
    expect(createdInputs().map((input) => input.media_type)).toEqual(["video", "video"]);
    expect(createdInputs().map((input) => input.video_quality)).toEqual(["720p", "720p"]);
  });

  it("reports each link's own outcome instead of one all-or-nothing result (FR-103)", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string, args?: unknown) => {
      if (cmd === "get_settings") return Promise.resolve(SETTINGS_WITH_FOLDER);
      if (cmd === "preview_media") {
        const { sourceUrl } = args as { sourceUrl: string };
        if (sourceUrl.endsWith("/2")) {
          return Promise.reject({ code: "ACCESS_DENIED", message: "private" });
        }
        return Promise.resolve({ ...SAMPLE_PREVIEW, source_url: sourceUrl, title: "Good clip" });
      }
      if (cmd === "create_download_job") {
        const { input } = args as { input: CreateJobInput };
        return Promise.resolve([jobFor(input.source_url)]);
      }
      return Promise.resolve(undefined);
    });
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2",
    );
    await user.click(screen.getByRole("button", { name: /download 2 links/i }));

    expect(await screen.findByText("Good clip")).toBeInTheDocument();
    expect(
      await screen.findByText(/private, requires login, or is DRM-protected/i),
    ).toBeInTheDocument();
    // The healthy link still made it to the queue.
    expect(createdInputs().map((input) => input.source_url)).toEqual(["https://youtube.com/1"]);
  });

  it("cannot start a batch before a save folder is known", async () => {
    const user = userEvent.setup();
    render(<DownloadForm />); // SAMPLE_SETTINGS has an empty default folder

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2",
    );

    expect(screen.getByRole("button", { name: /download 2 links/i })).toBeDisabled();
  });

  it("merges an imported URL list into the textarea without repeating what's there (FR-106)", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "get_settings") return Promise.resolve(SETTINGS_WITH_FOLDER);
      if (cmd === "read_url_list_file") {
        return Promise.resolve(["https://youtube.com/1", "https://youtube.com/3"]);
      }
      return Promise.resolve(undefined);
    });
    vi.mocked(openDialog).mockResolvedValue("/tmp/list.txt");
    const user = userEvent.setup();
    render(<DownloadForm />);

    const textarea = screen.getByLabelText(/video or audio link/i);
    await user.type(textarea, "https://youtube.com/1");
    await user.click(screen.getByRole("button", { name: /import list file/i }));

    await waitFor(() =>
      expect(textarea).toHaveValue("https://youtube.com/1\nhttps://youtube.com/3"),
    );
    expect(invoke).toHaveBeenCalledWith("read_url_list_file", { path: "/tmp/list.txt" });
  });

  it("explains why an imported list file could not be read", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "get_settings") return Promise.resolve(SETTINGS_WITH_FOLDER);
      if (cmd === "read_url_list_file") {
        return Promise.reject({ code: "FILE_TOO_LARGE", message: "too big" });
      }
      return Promise.resolve(undefined);
    });
    vi.mocked(openDialog).mockResolvedValue("/tmp/huge.txt");
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.click(screen.getByRole("button", { name: /import list file/i }));

    expect(await screen.findByText(/a URL list must be under 5 MB/i)).toBeInTheDocument();
  });

  it("renders quality options only from what preview_media actually returns, never a hard-coded list (FR-004/FR-019)", async () => {
    mockGetSettingsThenPreview(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), SAMPLE_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));

    expect(await screen.findByText("Sample video")).toBeInTheDocument();

    // Quality options default to the video panel; switch to audio to reach
    // the list this test actually exercises.
    await user.click(screen.getByRole("button", { name: /audio only/i }));

    // Only the bitrates preview_media returned should exist as options —
    // no "128kbps"/"320kbps" style constants baked into the component.
    expect(screen.getByText("160kbps")).toBeInTheDocument();
    expect(screen.queryByText("128kbps")).not.toBeInTheDocument();
  });

  it("shows the duration badge when the source reported one", async () => {
    mockGetSettingsThenPreview(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), SAMPLE_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));

    expect(await screen.findByText("Sample video")).toBeInTheDocument();
    expect(screen.getByText("2:00")).toBeInTheDocument();
  });

  // The badge is guarded on the raw duration, not on the formatted string --
  // formatDuration always returns "--:--" for a missing value, so guarding on
  // its result would put a meaningless clock badge on every live stream.
  it("hides the duration badge entirely when the source has no duration", async () => {
    mockGetSettingsThenPreview({ ...SAMPLE_PREVIEW, duration_seconds: null });
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), SAMPLE_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));

    expect(await screen.findByText("Sample video")).toBeInTheDocument();
    expect(screen.queryByText("--:--")).not.toBeInTheDocument();
  });
});
