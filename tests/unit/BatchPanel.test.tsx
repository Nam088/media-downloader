import { useState } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { BatchPanel } from "@/components/BatchPanel";
import type { BatchItem, BatchMediaType } from "@/hooks/use-batch-download";
import type { ComponentProps } from "react";

const URLS = ["https://a.example/1", "https://b.example/2", "https://c.example/3"];

const ITEMS: BatchItem[] = [
  { url: URLS[0], status: "created", title: "Bài A", errorCode: null },
  { url: URLS[1], status: "error", title: null, errorCode: "ACCESS_DENIED" },
  { url: URLS[2], status: "previewing", title: null, errorCode: null },
];

/**
 * The media type is controlled by `DownloadForm` now — the shared output
 * picker above the list has to show the controls that match it — so every test
 * needs something to hold it. Without this, clicking "full video" would report
 * a change that never comes back as a new prop and the panel would look frozen
 * on audio.
 *
 * The default lives here on purpose: it is the form's choice, and
 * `DownloadForm.test.tsx` is where it is asserted.
 */
function Harness(
  props: Omit<ComponentProps<typeof BatchPanel>, "mediaType" | "onMediaTypeChange">,
) {
  const [mediaType, setMediaType] = useState<BatchMediaType>("audio");
  return <BatchPanel {...props} mediaType={mediaType} onMediaTypeChange={setMediaType} />;
}

describe("BatchPanel (FR-101, FR-103)", () => {
  it("lets the user pick video instead of forcing audio", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn();
    render(<Harness urls={[URLS[0]]} items={[]} running={false} onRun={onRun} />);

    await user.click(screen.getByRole("radio", { name: /full video/i }));
    await user.click(screen.getByRole("button", { name: /download 1 link/i }));

    expect(onRun).toHaveBeenCalledWith("video");
  });

  it("defaults to audio when the user changes nothing", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn();
    render(<Harness urls={[URLS[0]]} items={[]} running={false} onRun={onRun} />);

    await user.click(screen.getByRole("button", { name: /download 1 link/i }));

    expect(onRun).toHaveBeenCalledWith("audio");
  });

  it("keeps the video choice when the run button is pressed a second time", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn();
    render(<Harness urls={URLS} items={[]} running={false} onRun={onRun} />);

    await user.click(screen.getByRole("radio", { name: /full video/i }));
    await user.click(screen.getByRole("button", { name: /download 3 links/i }));
    await user.click(screen.getByRole("button", { name: /download 3 links/i }));

    expect(onRun.mock.calls).toEqual([["video"], ["video"]]);
  });

  it("shows a per-url status row with the title once known and the reason when it failed", () => {
    render(<Harness urls={URLS} items={ITEMS} running onRun={vi.fn()} />);

    // Succeeded: the row shows the resolved title rather than the raw link.
    expect(screen.getByText("Bài A")).toBeInTheDocument();
    expect(screen.queryByText(URLS[0])).not.toBeInTheDocument();

    // Failed: the row shows the translated reason, not the bare error code.
    expect(screen.getByText(/private, requires login, or is DRM-protected/i)).toBeInTheDocument();
    expect(screen.queryByText("ACCESS_DENIED")).not.toBeInTheDocument();

    // Still working: no title yet, so the link itself stands in.
    expect(screen.getByText(URLS[2])).toBeInTheDocument();

    expect(screen.getByText("Added to the queue")).toBeInTheDocument();
    expect(screen.getByText("Reading link…")).toBeInTheDocument();
  });

  it("shows nothing about individual links before a run has started", () => {
    render(<Harness urls={URLS} items={[]} running={false} onRun={vi.fn()} />);

    expect(screen.queryByRole("list")).not.toBeInTheDocument();
  });

  it("disables the run button while a batch is in flight", () => {
    render(<Harness urls={[URLS[0]]} items={[]} running onRun={vi.fn()} />);

    expect(screen.getByRole("button", { name: /download 1 link/i })).toBeDisabled();
  });

  it("disables the run button when the caller says something else is blocking it", () => {
    render(<Harness urls={[URLS[0]]} items={[]} running={false} onRun={vi.fn()} disabled />);

    expect(screen.getByRole("button", { name: /download 1 link/i })).toBeDisabled();
  });

  it("enables the run button when nothing is blocking it", () => {
    render(<Harness urls={URLS} items={[]} running={false} onRun={vi.fn()} />);

    expect(screen.getByRole("button", { name: /download 3 links/i })).toBeEnabled();
  });
});
