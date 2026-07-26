import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import i18n from "@/lib/i18n";
import { HistoryList } from "@/components/HistoryList";
import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

function makeJob(overrides: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: "job-1",
    source_url: "https://example.com/v",
    platform: "youtube",
    media_type: "audio",
    audio_quality: "128kbps",
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "completed",
    progress_percent: 100,
    speed_bytes_per_sec: null,
    eta_seconds: null,
    error_message: null,
    output_directory: "/out",
    output_file_path: "/out/song.mp3",
    is_playlist_item: false,
    parent_playlist_id: null,
    retried_from_job_id: null,
    created_at: "2026-07-26T00:00:00Z",
    updated_at: "2026-07-26T00:00:00Z",
    title: null,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
    ...overrides,
  };
}

interface FakeQuery {
  search: string | null;
  status: string | null;
  limit: number;
  offset: number;
}

let backendJobs: DownloadJob[] = [];
let recorded: { command: string; args: Record<string, unknown> }[] = [];

function callsTo(command: string) {
  return recorded.filter((call) => call.command === command);
}

function lastQuery(command: string): FakeQuery {
  const calls = callsTo(command);
  return calls[calls.length - 1].args.query as FakeQuery;
}

/** Applies a query the same way the backend's `HistoryFilterSql` would,
 * against the fake in-memory job list. */
function applyQuery(jobs: DownloadJob[], query: FakeQuery): DownloadJob[] {
  const term = query.search?.toLowerCase().trim() ?? "";
  const filtered = jobs.filter((job) => {
    const matchesStatus = !query.status || job.status === query.status;
    const matchesSearch =
      !term ||
      job.source_url.toLowerCase().includes(term) ||
      (job.output_file_path && job.output_file_path.toLowerCase().includes(term)) ||
      job.platform.toLowerCase().includes(term);
    return matchesStatus && matchesSearch;
  });
  return filtered
    .slice()
    .sort((a, b) => (a.updated_at < b.updated_at ? 1 : -1));
}

beforeEach(() => {
  void i18n.changeLanguage("en");
  recorded = [];
  backendJobs = [];
  useQueueStore.setState({ jobs: {}, liveProgress: {} });

  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const parameters = (args ?? {}) as Record<string, unknown>;
    recorded.push({ command, args: parameters });

    if (command === "list_history") {
      const query = parameters.query as FakeQuery;
      const matched = applyQuery(backendJobs, query);
      return matched.slice(query.offset, query.offset + query.limit) as never;
    }
    if (command === "count_history") {
      const query = parameters.query as FakeQuery;
      return applyQuery(backendJobs, query).length as never;
    }
    if (command === "clear_history") {
      const count = backendJobs.length;
      backendJobs = [];
      return count as never;
    }
    return undefined as never;
  });
});

describe("HistoryList — paged navigation", () => {
  it("asks for page one with the default page size on first load", async () => {
    backendJobs = [makeJob({ id: "a" })];
    render(<HistoryList />);

    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));
    expect(lastQuery("list_history")).toEqual({ search: null, status: null, limit: 20, offset: 0 });
    expect(lastQuery("count_history")).toEqual({ search: null, status: null, limit: 20, offset: 0 });
  });

  it("forwards the search term and status tab from its props into the query", async () => {
    backendJobs = [makeJob({ id: "a" })];
    render(<HistoryList searchTerm="tiktok clip" filterStatus="failed" />);

    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));
    expect(lastQuery("list_history")).toEqual({
      search: "tiktok clip",
      status: "failed",
      limit: 20,
      offset: 0,
    });
  });

  it("moves to the next page with an offset one page size further", async () => {
    backendJobs = Array.from({ length: 45 }, (_, i) => makeJob({ id: `job-${i}` }));
    const user = userEvent.setup();
    render(<HistoryList />);
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));

    await user.click(screen.getByTestId("history-next-page"));

    await waitFor(() => expect(callsTo("list_history")).toHaveLength(2));
    expect(lastQuery("list_history")).toEqual({ search: null, status: null, limit: 20, offset: 20 });
  });

  it("jumps straight to a clicked page number", async () => {
    backendJobs = Array.from({ length: 45 }, (_, i) => makeJob({ id: `job-${i}` }));
    const user = userEvent.setup();
    render(<HistoryList />);
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));

    await user.click(screen.getByTestId("history-page-3"));

    await waitFor(() => expect(callsTo("list_history")).toHaveLength(2));
    expect(lastQuery("list_history")).toEqual({ search: null, status: null, limit: 20, offset: 40 });
  });

  it("disables Previous on the first page and Next on the last page", async () => {
    backendJobs = Array.from({ length: 25 }, (_, i) => makeJob({ id: `job-${i}` }));
    const user = userEvent.setup();
    render(<HistoryList />);
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));

    expect(screen.getByTestId("history-prev-page")).toBeDisabled();
    expect(screen.getByTestId("history-next-page")).not.toBeDisabled();

    await user.click(screen.getByTestId("history-next-page"));
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(2));

    expect(screen.getByTestId("history-prev-page")).not.toBeDisabled();
    expect(screen.getByTestId("history-next-page")).toBeDisabled();
  });

  it("changing the page size refetches page one with the new limit", async () => {
    backendJobs = Array.from({ length: 45 }, (_, i) => makeJob({ id: `job-${i}` }));
    const user = userEvent.setup();
    render(<HistoryList />);
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(1));

    await user.click(screen.getByTestId("history-next-page"));
    await waitFor(() => expect(callsTo("list_history")).toHaveLength(2));

    await user.selectOptions(screen.getByTestId("history-page-size"), "10");

    await waitFor(() => expect(lastQuery("list_history").limit).toBe(10));
    expect(lastQuery("list_history").offset).toBe(0);
  });
});

describe("HistoryList — clear history", () => {
  it("does not clear anything until the confirmation is accepted", async () => {
    backendJobs = [makeJob({ id: "a" })];
    const user = userEvent.setup();
    render(<HistoryList />);
    await screen.findAllByTestId("history-item");

    await user.click(screen.getByTestId("history-clear-button"));
    await user.click(screen.getByTestId("history-clear-cancel"));

    expect(callsTo("clear_history")).toHaveLength(0);
    expect(await screen.findAllByTestId("history-item")).toHaveLength(1);
  });

  it("empties the list once the confirmation is accepted", async () => {
    backendJobs = [makeJob({ id: "a" }), makeJob({ id: "b" })];
    const user = userEvent.setup();
    render(<HistoryList />);
    await screen.findAllByTestId("history-item");

    await user.click(screen.getByTestId("history-clear-button"));
    await user.click(screen.getByTestId("history-clear-confirm"));

    await waitFor(() => expect(callsTo("clear_history")).toHaveLength(1));
    expect(await screen.findByRole("heading", { name: /no download history yet/i })).toBeInTheDocument();
  });

  it("has no clear button when there is no history", async () => {
    backendJobs = [];
    render(<HistoryList />);

    await screen.findByRole("heading", { name: /no download history yet/i });
    expect(screen.queryByTestId("history-clear-button")).not.toBeInTheDocument();
  });
});
