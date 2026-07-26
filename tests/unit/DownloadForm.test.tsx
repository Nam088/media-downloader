import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DownloadForm } from "@/components/DownloadForm";
import type { MediaSource } from "@/types/download";
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

function mockGetSettingsThenPreview(preview: MediaSource) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "get_settings") return Promise.resolve(SAMPLE_SETTINGS);
    if (cmd === "preview_media") return Promise.resolve(preview);
    return Promise.resolve(undefined);
  });
}

describe("DownloadForm", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(SAMPLE_SETTINGS);
  });

  it("shows the batch-mode hint once 2+ links are pasted (FR-001)", async () => {
    const user = userEvent.setup();
    render(<DownloadForm />);

    const textarea = screen.getByLabelText(/video or audio link/i);
    await user.type(textarea, "https://youtube.com/1\nhttps://youtube.com/2");

    expect(screen.getByText(/2 links detected/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /download all \(2\)/i })).toBeInTheDocument();
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
