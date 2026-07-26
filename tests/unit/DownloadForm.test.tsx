import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { DownloadForm } from "@/components/DownloadForm";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
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

/** The same link, but with the two source-dependent lists actually filled in
 * — the state a real single-video preview comes back in. */
const PREVIEW_WITH_EXTRAS: MediaSource = {
  ...SAMPLE_PREVIEW,
  subtitles: [
    { language: "vi", label: "Vietnamese", auto_generated: false },
    { language: "en", label: "English", auto_generated: true },
  ],
  chapters: [
    { title: "Intro", start_seconds: 0, end_seconds: 30 },
    { title: "Rest", start_seconds: 30, end_seconds: 120 },
  ],
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

/** Settings with a save folder + a preview + a working create call, so a
 * single-link download can actually be submitted. */
function mockSingleJobBackend(preview: MediaSource) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(SETTINGS_WITH_FOLDER);
    if (cmd === "preview_media") return Promise.resolve(preview);
    if (cmd === "create_download_job") return Promise.resolve([jobFor(preview.source_url)]);
    return Promise.resolve(undefined);
  });
}

/** Paste the link and preview it, leaving the form on the preview screen. */
async function previewSingleLink(
  user: ReturnType<typeof userEvent.setup>,
  preview: MediaSource = SAMPLE_PREVIEW,
) {
  await user.type(screen.getByLabelText(/video or audio link/i), preview.source_url);
  await user.click(screen.getByRole("button", { name: /^preview$/i }));
  expect(await screen.findByText(preview.title)).toBeInTheDocument();
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

  // The link counter went through `t()` in Task 22, which turned a plain
  // template string into a pluralised key. Asserting the rendered words (not
  // just "some counter exists") is what catches a missing `_one`/`_other`
  // form, which i18next would otherwise paper over by echoing the key back.
  it("labels the link counter with the singular form for one link (FR-132)", async () => {
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), "https://youtube.com/1");

    expect(screen.getByText("Single URL")).toBeInTheDocument();
  });

  it("labels the link counter with the plural form for several links (FR-132)", async () => {
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2\nhttps://youtube.com/3",
    );

    expect(screen.getByText("3 URLs")).toBeInTheDocument();
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
  // The whole point of the picker: before it existed every job was created
  // with backend defaults, so none of the shipped output formats were
  // reachable.
  it("sends the output format the user picked with the job (FR-201/FR-235)", async () => {
    mockSingleJobBackend(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /audio only/i }));
    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /^FLAC$/ }));
    await user.click(screen.getByRole("switch", { name: /embed title and artist/i }));
    await user.click(screen.getByRole("button", { name: /download audio/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0].output_options).toEqual({
      audio: { format: "flac" },
      video_container: "mp4",
      codec_preference: "compatibility",
      embed_metadata: false,
      embed_thumbnail: true,
    });
  });

  // FR-208/FR-209: on for a new job. Sending nothing would instead reproduce
  // the pre-Phase-2 behaviour (both flags off), which is what
  // `NEW_JOB_OUTPUT_OPTIONS` exists to distinguish.
  it("sends the new-job defaults even when the picker is never opened (FR-208/FR-209)", async () => {
    mockSingleJobBackend(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /download video/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0].output_options).toEqual({
      audio: { format: "mp3" },
      video_container: "mp4",
      codec_preference: "compatibility",
      embed_metadata: true,
      embed_thumbnail: true,
    });
  });

  // FR-206. `SAMPLE_PREVIEW`'s audio formats are Opus, so the old
  // `MP3 / ${codec}` label printed "MP3 / OPUS" — a file no pipeline can
  // produce. The row now names the real source codec and the real target.
  it("labels audio rows with the conversion that will actually happen (FR-206)", async () => {
    mockGetSettingsThenPreview(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /audio only/i }));

    expect(screen.getAllByText("OPUS → MP3")).toHaveLength(2);
    expect(screen.queryByText("MP3 / OPUS")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /^FLAC$/ }));

    expect(screen.getAllByText("OPUS → FLAC")).toHaveLength(2);
    expect(screen.queryByText("OPUS → MP3")).not.toBeInTheDocument();
  });

  it("labels an audio row as untouched when the source format is kept (FR-206/FR-202)", async () => {
    mockGetSettingsThenPreview(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /audio only/i }));
    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /keep source/i }));

    expect(screen.getAllByText("OPUS · kept as-is")).toHaveLength(2);
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });

  // The video detail column used to be the constant "MP4 / H264 / AAC",
  // printed even for an MKV/quality-codec job.
  it("labels video rows from the container and codec actually chosen (FR-206)", async () => {
    mockGetSettingsThenPreview(SAMPLE_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);

    // Three: one detail line per quality row (720p, 480p) plus the picker's
    // own collapsed summary, which describes the same output.
    expect(screen.getAllByText("MP4 · H.264 / AAC")).toHaveLength(3);
    expect(screen.queryByText("MP4 / H264 / AAC")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /^MKV$/ }));
    await user.click(screen.getByRole("radio", { name: /^Quality/ }));

    expect(screen.getAllByText("MKV · Best codec the source has")).toHaveLength(3);
    // The old label is gone from the rows entirely (the words "H.264 / AAC"
    // still appear once, inside the compatibility option's own hint).
    expect(screen.queryByText("MP4 · H.264 / AAC")).not.toBeInTheDocument();
  });

  it("hides the output options entirely for a gallery source (FR-234)", async () => {
    mockGetSettingsThenPreview({
      ...SAMPLE_PREVIEW,
      is_gallery: true,
      available_audio_formats: [],
      available_video_qualities: [],
      gallery_items: [
        { url: "https://cdn.example/1.jpg", extension: "jpg", is_audio: false },
        { url: "https://cdn.example/2.jpg", extension: "jpg", is_audio: false },
      ],
    });
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user);

    // The gallery-specific controls are there, so the preview really did
    // render — the output picker is the only thing missing.
    expect(screen.getByText(/what to download/i)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /output options/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/audio format/i)).not.toBeInTheDocument();
  });

  // FR-217/FR-218 — the languages come from this link's own list, and the one
  // the user ticked is what the job carries.
  it("sends the subtitle languages picked from the source's own list (FR-217/FR-218)", async () => {
    mockSingleJobBackend(PREVIEW_WITH_EXTRAS);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user, PREVIEW_WITH_EXTRAS);
    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    await user.click(screen.getByRole("button", { name: /download video/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0].output_options?.subtitles).toEqual({
      languages: ["vi"],
      delivery: "separate_files",
      include_auto_generated: false,
    });
  });

  // FR-217/FR-221 — the same form, three different things to say about a
  // subtitle list, depending on what the preview actually found out.
  it("distinguishes an unchecked subtitle list from a source that has none", async () => {
    mockGetSettingsThenPreview({ ...SAMPLE_PREVIEW, subtitles: null });
    const user = userEvent.setup();
    const unchecked = render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /output options/i }));
    expect(screen.getByText(/subtitles was never checked/i)).toBeInTheDocument();
    unchecked.unmount();

    mockGetSettingsThenPreview({ ...SAMPLE_PREVIEW, subtitles: [] });
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /output options/i }));
    expect(screen.getByText(/offers no subtitles/i)).toBeInTheDocument();
    expect(screen.queryByText(/subtitles was never checked/i)).not.toBeInTheDocument();
  });

  // FR-223. The picker names the reason at the field; the form refuses to
  // create the job at all while that reason stands.
  it("blocks the download while the trim range is unusable (FR-223)", async () => {
    mockSingleJobBackend(PREVIEW_WITH_EXTRAS);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewSingleLink(user, PREVIEW_WITH_EXTRAS);
    // Enabled to begin with, so its later disabling means something.
    expect(screen.getByRole("button", { name: /download video/i })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /just one part/i }));

    expect(screen.getByRole("button", { name: /download video/i })).toBeDisabled();
    expect(screen.getByRole("alert")).toBeInTheDocument();

    await user.type(screen.getByLabelText(/start at/i), "1:00");
    await user.type(screen.getByLabelText(/end at/i), "0:30");
    expect(screen.getByRole("button", { name: /download video/i })).toBeDisabled();

    await user.clear(screen.getByLabelText(/end at/i));
    await user.type(screen.getByLabelText(/end at/i), "1:30");

    const download = screen.getByRole("button", { name: /download video/i });
    expect(download).toBeEnabled();
    await user.click(download);

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0].output_options?.segment).toEqual({
      mode: "trim",
      start_seconds: 60,
      end_seconds: 90,
      accurate_cut: false,
    });
  });

  // FR-225 — offered only for a source that really has chapters, and
  // explained rather than hidden when it does not.
  it("offers the chapter split only for a source with chapters (FR-225)", async () => {
    mockGetSettingsThenPreview(PREVIEW_WITH_EXTRAS);
    const user = userEvent.setup();
    const withChapters = render(<DownloadForm />);

    await previewSingleLink(user, PREVIEW_WITH_EXTRAS);
    await user.click(screen.getByRole("button", { name: /output options/i }));
    expect(screen.getByRole("radio", { name: /split into 2 chapters/i })).toBeEnabled();
    withChapters.unmount();

    mockGetSettingsThenPreview({ ...SAMPLE_PREVIEW, chapters: [] });
    render(<DownloadForm />);

    await previewSingleLink(user);
    await user.click(screen.getByRole("button", { name: /output options/i }));
    expect(screen.getByRole("radio", { name: /one file per chapter/i })).toBeDisabled();
    expect(screen.getByText(/no chapter list/i)).toBeInTheDocument();
  });

  // FR-232. The batch used to be the path where every output choice was
  // dropped: `buildJobInput` was called without any, so a pasted list always
  // produced the pre-Phase-2 defaults.
  it("carries the shared output options into every job of a pasted batch (FR-232)", async () => {
    mockBatchBackend();
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2",
    );
    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /^FLAC$/ }));
    await user.click(screen.getByRole("button", { name: /download 2 links/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(2));
    for (const input of createdInputs()) {
      expect(input.output_options).toEqual({
        ...NEW_JOB_OUTPUT_OPTIONS,
        audio: { format: "flac" },
      });
    }
  });

  // The batch picker has to describe what the batch will produce, so the two
  // choices cannot be held separately.
  it("switches the batch output picker to video controls when the batch does (FR-232)", async () => {
    mockBatchBackend();
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(
      screen.getByLabelText(/video or audio link/i),
      "https://youtube.com/1\nhttps://youtube.com/2",
    );
    await user.click(screen.getByRole("button", { name: /output options/i }));

    // Audio is the default for a pasted batch.
    expect(screen.getByText(/audio format/i)).toBeInTheDocument();
    expect(screen.queryByText(/video container/i)).not.toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /full video/i }));

    expect(screen.getByText(/video container/i)).toBeInTheDocument();
    expect(screen.queryByText(/audio format/i)).not.toBeInTheDocument();
  });

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
