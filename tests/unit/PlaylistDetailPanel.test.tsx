import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DownloadForm } from "@/components/DownloadForm";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type { CreatePlaylistJobsInput, MediaSource } from "@/types/download";
import type { AppSettings } from "@/types/settings";

// Mirrors the backend's own defaults (see `Db::get_settings`), except for the
// output directory, which is pre-filled so the panel's submit button is
// reachable without the test having to pick a folder first.
const SAMPLE_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "/tmp/out",
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
} satisfies AppSettings;

const PLAYLIST_PREVIEW: MediaSource = {
  source_url: "https://youtube.com/playlist?list=PLn7c8RY7CfPYhHPNxsNCWaixwIA0W-S8y",
  title: "Bước Qua Nhau - Vũ.",
  thumbnail_url: null,
  duration_seconds: null,
  platform: "youtube",
  is_playlist: true,
  playlist_item_count: 7,
  available_video_qualities: [],
  available_audio_formats: [],
  is_gallery: false,
  gallery_items: [],
  playlist_entries: Array.from({ length: 7 }, (_, i) => ({
    url: `https://www.youtube.com/watch?v=video${i}`,
    title: `Video ${i}`,
    duration_seconds: 200 + i,
    thumbnail_url: null,
  })),
};

/** The `create_playlist_download_jobs` payloads, in call order. */
function playlistInputs(): CreatePlaylistJobsInput[] {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([cmd]) => cmd === "create_playlist_download_jobs")
    .map(([, args]) => (args as { input: CreatePlaylistJobsInput }).input);
}

function mockGetSettingsThenPreview(preview: MediaSource) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(SAMPLE_SETTINGS);
    if (cmd === "preview_media") return Promise.resolve(preview);
    return Promise.resolve(undefined);
  });
}

describe("Playlist detail panel (inline, reproduction)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(SAMPLE_SETTINGS);
  });

  it("shows every entry inline right after preview, with no extra click needed", async () => {
    mockGetSettingsThenPreview(PLAYLIST_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), PLAYLIST_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));

    expect(await screen.findByText("Bước Qua Nhau - Vũ.")).toBeInTheDocument();

    // The list appears on its own; there is no generic "Download video"
    // button for a playlist with real entries anymore, since the panel
    // below carries its own submit button.
    expect(await screen.findByText(/Choose videos to download/i)).toBeInTheDocument();
    expect(screen.getByText("Video 0")).toBeInTheDocument();
    expect(screen.getByText("Video 6")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /download video/i })).not.toBeInTheDocument();
  });

  // FR-232. `CreatePlaylistJobsInput.output_options` existed from the day this
  // command shipped and nothing ever filled it, so every playlist submission
  // silently produced the pre-Phase-2 defaults no matter what was on screen.
  it("carries the chosen output options into the playlist submission (FR-232)", async () => {
    mockGetSettingsThenPreview(PLAYLIST_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), PLAYLIST_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));
    expect(await screen.findByText(/Choose videos to download/i)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /^MKV$/ }));
    await user.click(screen.getByRole("button", { name: /download 7 selected/i }));

    await waitFor(() => expect(playlistInputs()).toHaveLength(1));
    expect(playlistInputs()[0].output_options).toEqual({
      ...NEW_JOB_OUTPUT_OPTIONS,
      video_container: "mkv",
    });
  });

  it("blocks the playlist submission while the trim range is unusable (FR-223)", async () => {
    mockGetSettingsThenPreview(PLAYLIST_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), PLAYLIST_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));
    expect(await screen.findByText(/Choose videos to download/i)).toBeInTheDocument();

    const submit = screen.getByRole("button", { name: /download 7 selected/i });
    expect(submit).toBeEnabled();

    await user.click(screen.getByRole("button", { name: /output options/i }));
    await user.click(screen.getByRole("radio", { name: /just one part/i }));

    expect(screen.getByRole("button", { name: /download 7 selected/i })).toBeDisabled();

    await user.type(screen.getByLabelText(/start at/i), "0:30");

    expect(screen.getByRole("button", { name: /download 7 selected/i })).toBeEnabled();
  });

  // A flat playlist preview never fetched per-video metadata, so neither list
  // was checked — which is not the same as their being empty.
  it("says the subtitle and chapter lists were never checked for a playlist", async () => {
    mockGetSettingsThenPreview(PLAYLIST_PREVIEW);
    const user = userEvent.setup();
    render(<DownloadForm />);

    await user.type(screen.getByLabelText(/video or audio link/i), PLAYLIST_PREVIEW.source_url);
    await user.click(screen.getByRole("button", { name: /preview/i }));
    expect(await screen.findByText(/Choose videos to download/i)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /output options/i }));

    expect(screen.getByText(/subtitles was never checked/i)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /one file per chapter/i })).toBeDisabled();
    expect(screen.getByText(/Chapters were never checked/i)).toBeInTheDocument();
  });
});
