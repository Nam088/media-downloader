import { invoke } from "@tauri-apps/api/core";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { QueueToolbar } from "@/components/QueueToolbar";
import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

function makeJob(overrides: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: "job-1",
    source_url: "https://youtube.com/watch?v=abc",
    platform: "youtube",
    media_type: "audio",
    audio_quality: "128kbps",
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "downloading",
    progress_percent: 42,
    speed_bytes_per_sec: 1_500_000,
    eta_seconds: 10,
    error_message: null,
    output_directory: "/tmp",
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
    ...overrides,
  };
}

describe("QueueToolbar (FR-118)", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {} });
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(() => Promise.resolve([]));
  });

  it("pauses every job", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={3} pausedCount={0} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /pause all/i }));

    expect(invoke).toHaveBeenCalledWith("pause_all_jobs");
  });

  it("resumes every paused job", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={0} pausedCount={2} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /resume all/i }));

    expect(invoke).toHaveBeenCalledWith("resume_all_jobs");
  });

  it("asks for confirmation before cancelling everything", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={3} pausedCount={0} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /cancel all/i }));
    expect(invoke).not.toHaveBeenCalledWith("cancel_all_jobs");

    await user.click(screen.getByRole("button", { name: /confirm/i }));
    expect(invoke).toHaveBeenCalledWith("cancel_all_jobs");
  });

  it("backs out of cancelling everything without touching the queue", async () => {
    const user = userEvent.setup();
    render(<QueueToolbar activeCount={3} pausedCount={0} finishedCount={0} />);

    await user.click(screen.getByRole("button", { name: /cancel all/i }));
    await user.click(screen.getByRole("button", { name: /^cancel$/i }));

    expect(invoke).not.toHaveBeenCalledWith("cancel_all_jobs");
    // Back to the un-confirmed state, ready to be asked again.
    expect(screen.getByRole("button", { name: /cancel all/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /confirm/i })).not.toBeInTheDocument();
  });

  it("disables actions that would do nothing", () => {
    render(<QueueToolbar activeCount={0} pausedCount={0} finishedCount={0} />);

    expect(screen.getByRole("button", { name: /pause all/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /resume all/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /cancel all/i })).toBeDisabled();
    expect(screen.getByRole("button", { name: /clear finished/i })).toBeDisabled();
  });

  it("enables each action as soon as it has something to act on", () => {
    render(<QueueToolbar activeCount={1} pausedCount={1} finishedCount={1} />);

    expect(screen.getByRole("button", { name: /pause all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /resume all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /cancel all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /clear finished/i })).toBeEnabled();
  });

  it("cancel all stays available when the only jobs left are paused", () => {
    render(<QueueToolbar activeCount={0} pausedCount={2} finishedCount={0} />);

    expect(screen.getByRole("button", { name: /cancel all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /pause all/i })).toBeDisabled();
  });

  it("clears finished jobs from the view without deleting them", async () => {
    const user = userEvent.setup();
    useQueueStore.setState({
      jobs: {
        done: makeJob({ id: "done", status: "completed" }),
        broken: makeJob({ id: "broken", status: "failed" }),
        stopped: makeJob({ id: "stopped", status: "canceled" }),
        running: makeJob({ id: "running", status: "downloading" }),
        waiting: makeJob({ id: "waiting", status: "paused" }),
      },
    });

    render(<QueueToolbar activeCount={1} pausedCount={1} finishedCount={3} />);
    await user.click(screen.getByRole("button", { name: /clear finished/i }));

    expect(Object.keys(useQueueStore.getState().jobs).sort()).toEqual(["running", "waiting"]);
    // History reads these very rows from the database, so clearing the view
    // must never reach the backend.
    expect(invoke).not.toHaveBeenCalled();
  });
});
