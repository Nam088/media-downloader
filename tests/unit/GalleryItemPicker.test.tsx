import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { GalleryItemPicker } from "@/components/GalleryItemPicker";
import type { GalleryItemPreview } from "@/types/download";

function images(count: number): GalleryItemPreview[] {
  return Array.from({ length: count }, (_, i) => ({
    url: `https://cdn.example/${i}.jpg`,
    extension: "jpg",
    is_audio: false,
  }));
}

function thumbnailSources(container: HTMLElement): (string | null)[] {
  return [...container.querySelectorAll("img")].map((img) => img.getAttribute("src"));
}

/** Audio deliberately sits at index 1, *before* two of the three images, so a
 * component that emitted positions in the filtered image list rather than in
 * the original `gallery_items` array produces different numbers here: the
 * images live at original indices 0, 2 and 3 but at filtered positions 0, 1
 * and 2. Without the audio-first layout the index assertions below would pass
 * for both implementations. */
const AUDIO_IN_THE_MIDDLE: GalleryItemPreview[] = [
  { url: "https://cdn.example/a.jpg", extension: "jpg", is_audio: false },
  { url: "https://cdn.example/track.mp3", extension: "mp3", is_audio: true },
  { url: "https://cdn.example/b.jpg", extension: "jpg", is_audio: false },
  { url: "https://cdn.example/c.jpg", extension: "jpg", is_audio: false },
];

describe("GalleryItemPicker (FR-134)", () => {
  it("renders every selectable item, not just the first 24", () => {
    const { container } = render(
      <GalleryItemPicker items={images(30)} selectedIndices={[]} onChange={vi.fn()} />,
    );

    expect(screen.getAllByRole("checkbox")).toHaveLength(30);
    // The 25th and 30th items specifically: those are the ones the old
    // `slice(0, 24)` grid downloaded without ever showing them.
    const sources = thumbnailSources(container);
    expect(sources[24]).toBe("https://cdn.example/24.jpg");
    expect(sources[29]).toBe("https://cdn.example/29.jpg");
    expect(sources).toHaveLength(30);
  });

  it("lazy-loads the thumbnails, since a post can hold hundreds", () => {
    const { container } = render(
      <GalleryItemPicker items={images(3)} selectedIndices={[]} onChange={vi.fn()} />,
    );

    for (const img of container.querySelectorAll("img")) {
      expect(img).toHaveAttribute("loading", "lazy");
    }
  });

  it("ticks the boxes named by the original indices", () => {
    render(
      <GalleryItemPicker items={AUDIO_IN_THE_MIDDLE} selectedIndices={[2]} onChange={vi.fn()} />,
    );

    // Original index 2 is the *second* image in the grid, not the third.
    const checked = screen
      .getAllByRole<HTMLInputElement>("checkbox")
      .map((box) => box.checked);
    expect(checked).toEqual([false, true, false]);
  });

  it("deselects an item by its original index, not its position in the grid", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <GalleryItemPicker
        items={AUDIO_IN_THE_MIDDLE}
        selectedIndices={[0, 2, 3]}
        onChange={onChange}
      />,
    );

    // Second image in the grid = original index 2 (filtered position 1).
    await user.click(screen.getAllByRole("checkbox")[1]);

    expect(onChange).toHaveBeenCalledWith([0, 3]);
  });

  it("selects an item by its original index", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <GalleryItemPicker items={AUDIO_IN_THE_MIDDLE} selectedIndices={[0]} onChange={onChange} />,
    );

    // Third image in the grid = original index 3 (filtered position 2). A
    // filtered-index implementation would emit [0, 2] here, which the backend
    // would resolve to the wrong file on its re-crawl.
    await user.click(screen.getAllByRole("checkbox")[2]);

    expect(onChange).toHaveBeenCalledWith([0, 3]);
  });

  it("selects every image and nothing else", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <GalleryItemPicker items={AUDIO_IN_THE_MIDDLE} selectedIndices={[]} onChange={onChange} />,
    );

    await user.click(screen.getByRole("button", { name: /select all/i }));

    // Original index 1 is the audio track: it is never part of the selection,
    // since whether it gets downloaded is decided by the gallery mode.
    expect(onChange).toHaveBeenCalledWith([0, 2, 3]);
  });

  it("clears the selection", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <GalleryItemPicker
        items={AUDIO_IN_THE_MIDDLE}
        selectedIndices={[0, 2, 3]}
        onChange={onChange}
      />,
    );

    await user.click(screen.getByRole("button", { name: /select none/i }));

    expect(onChange).toHaveBeenCalledWith([]);
  });

  it("skips audio tracks, whose inclusion is governed by gallery mode", () => {
    const { container } = render(
      <GalleryItemPicker items={AUDIO_IN_THE_MIDDLE} selectedIndices={[]} onChange={vi.fn()} />,
    );

    expect(screen.getAllByRole("checkbox")).toHaveLength(3);
    expect(thumbnailSources(container)).toEqual([
      "https://cdn.example/a.jpg",
      "https://cdn.example/b.jpg",
      "https://cdn.example/c.jpg",
    ]);
  });

  it("counts progress against the selectable images, not the raw item count", () => {
    render(
      <GalleryItemPicker
        items={AUDIO_IN_THE_MIDDLE}
        selectedIndices={[0, 2, 3]}
        onChange={vi.fn()}
      />,
    );

    // 3 images among 4 gallery items — "3 of 3", never "3 of 4", because the
    // audio track can never be ticked.
    expect(screen.getByText(/3 of 3/i)).toBeInTheDocument();
  });

  it("renders nothing when the source has no images to pick from", () => {
    const { container } = render(
      <GalleryItemPicker
        items={[{ url: "https://cdn.example/track.mp3", extension: "mp3", is_audio: true }]}
        selectedIndices={[]}
        onChange={vi.fn()}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });
});
