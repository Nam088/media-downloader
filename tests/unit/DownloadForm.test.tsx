import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { DownloadForm } from "@/components/DownloadForm";
import type { MediaSource } from "@/types/download";
import type { AppSettings } from "@/types/settings";

const SAMPLE_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "",
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
});
