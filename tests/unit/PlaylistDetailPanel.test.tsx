import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DownloadForm } from "@/components/DownloadForm";
import type { MediaSource } from "@/types/download";
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
});
