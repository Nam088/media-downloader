import { describe, expect, it, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueueList } from "@/components/QueueList";
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
    ...overrides,
  };
}

describe("QueueList", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {} });
  });

  it("shows an empty state when there are no active jobs", () => {
    render(<QueueList />);
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });

  it("renders an active job with its progress percentage", () => {
    useQueueStore.setState({ jobs: { "job-1": makeJob() } });
    render(<QueueList />);
    expect(screen.getByText("https://youtube.com/watch?v=abc")).toBeInTheDocument();
    expect(screen.getByText("42%")).toBeInTheDocument();
  });

  it("does not list completed jobs among active downloads", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ status: "completed", progress_percent: 100 }) },
    });
    render(<QueueList />);
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });
});
