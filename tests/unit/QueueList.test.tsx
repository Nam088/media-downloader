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
    title: null,
    playlist_title: null,
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

  it("shows a job's title instead of its raw source_url when a title is set", () => {
    useQueueStore.setState({ jobs: { "job-1": makeJob({ title: "Some Song (Official Video)" }) } });
    render(<QueueList />);
    expect(screen.getByText("Some Song (Official Video)")).toBeInTheDocument();
    expect(screen.queryByText("https://youtube.com/watch?v=abc")).not.toBeInTheDocument();
  });

  it("groups playlist jobs under one header instead of N separate rows", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 1",
          status: "downloading",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 2",
          status: "downloading",
          progress_percent: 0,
        }),
      },
    });
    render(<QueueList />);
    expect(screen.getByText("My Playlist")).toBeInTheDocument();
    expect(screen.getByText("0/2 completed")).toBeInTheDocument();
    // Expanded by default: both children are visible under the group.
    expect(screen.getByText("Video 1")).toBeInTheDocument();
    expect(screen.getByText("Video 2")).toBeInTheDocument();
  });

  it("keeps a playlist group visible, with the finished child still shown, until every job in it is completed", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 1",
          status: "completed",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 2",
          status: "downloading",
          progress_percent: 30,
        }),
      },
    });
    render(<QueueList />);
    // Regression guard: a standalone job disappears from this list once
    // completed (see the test above). A playlist group must not do the
    // same to individual children while siblings are still active, or the
    // completed count above would have nothing to count against.
    expect(screen.getByText("My Playlist")).toBeInTheDocument();
    expect(screen.getByText("Video 1")).toBeInTheDocument();
    expect(screen.getByText("Video 2")).toBeInTheDocument();
  });

  it("hides the playlist group once every job in it has completed", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          status: "completed",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          status: "completed",
          progress_percent: 100,
        }),
      },
    });
    render(<QueueList />);
    expect(screen.queryByText("My Playlist")).not.toBeInTheDocument();
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });

  it("falls back to a generic label when a playlist group has no stored title", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({ id: "job-1", is_playlist_item: true, parent_playlist_id: "pl-1", playlist_title: null }),
      },
    });
    render(<QueueList />);
    expect(screen.getByText("Playlist")).toBeInTheDocument();
  });
});
