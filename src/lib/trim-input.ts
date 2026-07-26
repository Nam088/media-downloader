import { validateTrimRange } from "@/types/download";
import type { OutputOptions } from "@/types/download";

/**
 * Reading and writing the two trim boxes, and the one question a submit button
 * needs to ask about them.
 *
 * Kept out of the component that renders those boxes because two other screens
 * — the single-link form and the playlist panel — have to ask the same
 * question before creating a job, and importing a validity rule out of a UI
 * component is how the rule ends up copied instead of shared.
 */

/**
 * Why this job cannot be created yet, or `null` when the segment choice is
 * fine (FR-223).
 *
 * Exported and taken by the *whole* `OutputOptions` rather than by a
 * `TrimRange`, so a submit button can ask the question without first having to
 * know that `segment` is a union and which of its members carries a range —
 * the check cannot be forgotten for one call site and remembered for another.
 */
export function trimErrorFor(
  options: OutputOptions,
  durationSeconds?: number | null,
): ReturnType<typeof validateTrimRange> {
  const segment = options.segment;
  if (!segment || segment.mode !== "trim") return null;
  return validateTrimRange(segment, durationSeconds);
}

/** `90` → `"1:30"`, for pre-filling the boxes from a preset's saved range. */
export function formatSeconds(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return "";
  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const mmss = `${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  return hours > 0 ? `${hours}:${mmss}` : mmss;
}

/**
 * `"1:30"`, `"01:02:03"` and `"90"` all mean the same 90/3723/90 seconds.
 *
 * Three results, all meaningful: a number, `null` for "this bound is not set"
 * (an empty box — the type's own way of saying "from the beginning"/"to the
 * end"), and `NaN` for text that is not a time at all. `NaN` rather than
 * silently treating garbage as "unset", because `validateTrimRange` rejects a
 * non-finite bound and so keeps the download blocked while nonsense is on
 * screen.
 */
export function parseTimeInput(raw: string): number | null {
  const text = raw.trim();
  if (text.length === 0) return null;
  if (/^\d+(\.\d+)?$/.test(text)) return Number(text);

  const parts = text.split(":");
  if (parts.length < 2 || parts.length > 3) return Number.NaN;
  if (!parts.every((part) => /^\d+(\.\d+)?$/.test(part))) return Number.NaN;

  return parts.reduce((total, part) => total * 60 + Number(part), 0);
}

