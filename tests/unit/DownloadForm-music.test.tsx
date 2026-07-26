import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DownloadForm } from "@/components/DownloadForm";
import { useQueueStore } from "@/stores/queue-store";
import { useSettingsStore } from "@/stores/settings-store";
import type { CreateJobInput, DownloadJob, MediaSource } from "@/types/download";
import type { AppSettings } from "@/types/settings";

const SETTINGS: AppSettings = {
  theme: "system",
  language: "system",
  default_output_directory: "/downloads",
  show_logs_tab: false,
  max_concurrent_downloads: 3,
  rate_limit_kbps: 0,
  max_retry_attempts: 3,
  run_in_background: false,
  spotiflac_service_order: "tidal,qobuz,deezer,amazon",
  spotiflac_quality: "flac16",
  spotiflac_extensions_fallback: true,
  tg_bot_token: "",
  tg_chat_id: "",
};

/** A single Spotify track as `preview_media` returns it through the
 * SpotiFLAC worker: no yt-dlp format lists at all, just the tier list —
 * which is the whole "this is a music source" signal. */
const MUSIC_PREVIEW: MediaSource = {
  source_url: "https://open.spotify.com/track/abc123",
  title: "Artist – Track",
  thumbnail_url: null,
  duration_seconds: 215,
  platform: "spotify",
  is_playlist: false,
  playlist_item_count: null,
  available_video_qualities: [],
  available_audio_formats: [],
  is_gallery: false,
  gallery_items: [],
  playlist_entries: [],
  available_music_tiers: ["flac16", "flac24", "mp3_320"],
};

function musicJob(): DownloadJob {
  return {
    id: "job-music",
    source_url: MUSIC_PREVIEW.source_url,
    platform: "spotify",
    media_type: "music",
    audio_quality: "flac16",
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
    created_at: "2026-07-27T00:00:00Z",
    updated_at: "2026-07-27T00:00:00Z",
    title: MUSIC_PREVIEW.title,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
  };
}

function mockBackend(settings: AppSettings = SETTINGS) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(settings);
    if (cmd === "preview_media") return Promise.resolve(MUSIC_PREVIEW);
    if (cmd === "create_download_job") return Promise.resolve([musicJob()]);
    return Promise.resolve(undefined);
  });
}

function createdInputs(): CreateJobInput[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "create_download_job")
    .map(([, args]) => (args as { input: CreateJobInput }).input);
}

async function previewMusicLink(user: ReturnType<typeof userEvent.setup>) {
  await user.type(screen.getByLabelText(/video or audio link/i), MUSIC_PREVIEW.source_url);
  await user.click(screen.getByRole("button", { name: /^preview$/i }));
  expect(await screen.findByText(MUSIC_PREVIEW.title)).toBeInTheDocument();
}

describe("DownloadForm — music sources (T020)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(SETTINGS);
    useQueueStore.setState({ jobs: {}, liveProgress: {} });
    useSettingsStore.setState({ settings: null, loading: true, fetchPromise: null });
  });

  it("shows the three quality tiers and hides the video/audio controls", async () => {
    mockBackend();
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewMusicLink(user);

    expect(screen.getByRole("radio", { name: /FLAC 16-bit Lossless/i })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /FLAC 24-bit Hi-Res/i })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /MP3 320kbps/i })).toBeInTheDocument();

    // None of the yt-dlp/gallery machinery applies to a music job.
    expect(screen.queryByRole("button", { name: /full video/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /audio only/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /output options/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/what to download/i)).not.toBeInTheDocument();
  });

  it("defaults the tier to the persisted spotiflac_quality setting", async () => {
    mockBackend({ ...SETTINGS, spotiflac_quality: "flac24" });
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewMusicLink(user);

    expect(screen.getByRole("radio", { name: /FLAC 24-bit Hi-Res/i })).toBeChecked();
    expect(screen.getByRole("radio", { name: /FLAC 16-bit Lossless/i })).not.toBeChecked();
  });

  it("submits a music job carrying the chosen tier in audio_quality", async () => {
    mockBackend();
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewMusicLink(user);
    await user.click(screen.getByRole("radio", { name: /MP3 320kbps/i }));
    await user.click(screen.getByRole("button", { name: /download audio/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0]).toEqual({
      source_url: MUSIC_PREVIEW.source_url,
      media_type: "music",
      audio_quality: "mp3_320",
      video_quality: null,
      output_directory: "/downloads",
      playlist_scope: undefined,
      title: MUSIC_PREVIEW.title,
    });
    // Specifically: no gallery/video knobs, no output_options — the backend
    // rejects those for a music job.
    expect(createdInputs()[0]).not.toHaveProperty("gallery_mode");
    expect(createdInputs()[0]).not.toHaveProperty("output_options");
  });

  it("falls back to flac16 when settings hold no usable tier", async () => {
    mockBackend({ ...SETTINGS, spotiflac_quality: "" as AppSettings["spotiflac_quality"] });
    const user = userEvent.setup();
    render(<DownloadForm />);

    await previewMusicLink(user);
    await user.click(screen.getByRole("button", { name: /download audio/i }));

    await waitFor(() => expect(createdInputs()).toHaveLength(1));
    expect(createdInputs()[0].audio_quality).toBe("flac16");
  });
});
