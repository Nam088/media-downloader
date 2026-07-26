import { useTranslation } from "react-i18next";

import type { GalleryItemPreview } from "@/types/download";

interface GalleryItemPickerProps {
  items: GalleryItemPreview[];
  /** Positions in `items` that are currently picked. */
  selectedIndices: number[];
  onChange: (indices: number[]) => void;
}

/**
 * The image grid for gallery-backed sources (TikTok photo posts, Instagram
 * carousels, ...).
 *
 * Two things this component is careful about:
 *
 * 1. It renders **every** image, in a scrolling container. The previous inline
 *    grid cut off at 24 thumbnails while still defaulting the selection to
 *    everything, so a 30-image post silently downloaded six files the user
 *    could neither see nor untick (FR-134).
 * 2. The numbers it emits are positions in the **original** `gallery_items`
 *    array, not in the filtered image list. The backend replays a selection
 *    against a fresh crawl by ordinal position — TikTok's per-item CDN URLs
 *    are signed and short-lived, so they can't be used as identifiers (see
 *    `models::DownloadJob.selected_gallery_indices`). Emitting a filtered
 *    index would therefore download a different file than the one clicked.
 *
 * Audio tracks are left out of the grid entirely: whether the background
 * audio is downloaded is decided by the gallery mode (`files` / `audio_only` /
 * `images_only` / `slideshow`), not by a per-item tick.
 */
export function GalleryItemPicker({ items, selectedIndices, onChange }: GalleryItemPickerProps) {
  const { t } = useTranslation();

  const imageEntries = items
    .map((item, originalIndex) => ({ item, originalIndex }))
    .filter(({ item }) => !item.is_audio);

  const selected = new Set(selectedIndices);
  // Counted over the images alone: the audio track is unselectable, so
  // measuring against `items.length` would leave the picker permanently
  // stuck one short of "all selected".
  const selectedCount = imageEntries.filter(({ originalIndex }) =>
    selected.has(originalIndex),
  ).length;

  // Audio-only sources have nothing to pick; the gallery mode selector alone
  // covers them.
  if (imageEntries.length === 0) return null;

  function toggle(originalIndex: number) {
    const next = new Set(selected);
    if (next.has(originalIndex)) {
      next.delete(originalIndex);
    } else {
      next.add(originalIndex);
    }
    onChange([...next].sort((a, b) => a - b));
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium text-muted-foreground">
          {t("downloadForm.gallery_selected_count", {
            selected: selectedCount,
            total: imageEntries.length,
          })}
        </span>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={() => onChange(imageEntries.map(({ originalIndex }) => originalIndex))}
            className="text-xs font-semibold text-primary hover:underline"
          >
            {t("downloadForm.gallery_select_all")}
          </button>
          <button
            type="button"
            onClick={() => onChange([])}
            className="text-xs font-semibold text-muted-foreground hover:underline"
          >
            {t("downloadForm.gallery_select_none")}
          </button>
        </div>
      </div>

      <div className="grid max-h-96 grid-cols-4 gap-2 overflow-y-auto sm:grid-cols-6">
        {imageEntries.map(({ item, originalIndex }, position) => {
          const isSelected = selected.has(originalIndex);
          return (
            <label
              // Positional key: gallery URLs are not guaranteed unique (the
              // same asset can appear twice in a carousel).
              key={originalIndex}
              className="group relative aspect-square cursor-pointer overflow-hidden rounded-md border border-border/50 shadow-2xs"
            >
              <input
                type="checkbox"
                checked={isSelected}
                onChange={() => toggle(originalIndex)}
                aria-label={t("downloadForm.gallery_item_label", {
                  defaultValue: "Image {{number}}",
                  number: position + 1,
                })}
                className="absolute right-1 top-1 z-10 h-4 w-4 accent-primary"
              />
              <img
                src={item.url}
                alt=""
                loading="lazy"
                className={`h-full w-full object-cover transition-opacity ${
                  isSelected ? "" : "opacity-35"
                }`}
              />
            </label>
          );
        })}
      </div>
    </div>
  );
}
