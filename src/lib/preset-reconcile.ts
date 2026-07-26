import type { TFunction } from "i18next";

import { supportsBitrate, supportsEmbeddedSubtitles } from "@/types/download";
import type {
  AudioOutput,
  MediaSource,
  MediaType,
  OutputOptions,
  SubtitleOptions,
} from "@/types/download";

/**
 * FR-231 — one thing a preset asked for that this particular link cannot give,
 * and what was used instead.
 *
 * A tagged union of *data*, not a pre-rendered sentence: the wording belongs in
 * the locale files (`presetAdjustmentMessage` below turns each one into copy),
 * and a test asserting "the bitrate fell back from 320 to 128" should not have
 * to match English prose to do it.
 */
export type PresetAdjustment =
  /** The preset's bitrate is not among the ones this link publishes. */
  | { kind: "audio_bitrate"; from: number; to: number }
  /** The link publishes no bitrate at all, so there is no nearest one to pick. */
  | { kind: "audio_bitrate_unavailable"; from: number }
  /** Some of the preset's subtitle languages are not offered here. */
  | { kind: "subtitle_languages_dropped"; dropped: string[]; kept: string[] }
  /** The link has no subtitles at all (`subtitles === []`). */
  | { kind: "subtitles_unavailable"; dropped: string[] }
  /** FR-220 — this output container cannot hold a subtitle track. */
  | { kind: "subtitles_not_embeddable" }
  /** FR-225 — the link has no chapters to split on. */
  | { kind: "chapters_unavailable" };

export interface ReconciledPreset {
  /** The preset's options, adjusted to what this source can actually deliver. */
  options: OutputOptions;
  /** Empty when the preset applied verbatim. */
  adjustments: PresetAdjustment[];
}

/** The bitrate this audio choice is asking for, or `null` when it asks for
 * none (a lossless format, "keep source", or "match the chosen quality"). */
function requestedBitrate(audio: AudioOutput): number | null {
  if (!supportsBitrate(audio)) return null;
  return "bitrate_kbps" in audio ? (audio.bitrate_kbps ?? null) : null;
}

function withBitrate(audio: AudioOutput, bitrate_kbps: number | null): AudioOutput {
  switch (audio.format) {
    case "mp3":
      return { format: "mp3", bitrate_kbps };
    case "m4a":
      return { format: "m4a", bitrate_kbps };
    case "opus":
      return { format: "opus", bitrate_kbps };
    default:
      return audio;
  }
}

/**
 * The offered bitrate closest to `wanted`, breaking a tie upwards.
 *
 * Ties go to the higher bitrate on purpose: asked for 192 with 128 and 256 on
 * offer, the user who picked a target is better served by the one that keeps
 * more of the signal than by the one that throws more of it away.
 */
function nearestBitrate(offered: number[], wanted: number): number {
  return offered.reduce((best, candidate) => {
    const bestDistance = Math.abs(best - wanted);
    const distance = Math.abs(candidate - wanted);
    if (distance < bestDistance) return candidate;
    if (distance === bestDistance && candidate > best) return candidate;
    return best;
  });
}

export interface ReconcileArgs {
  /** What this job will be downloaded as — decides which halves of the preset
   * are even read (an audio bitrate means nothing to a video job). */
  mediaType: MediaType;
  /** The link the preset is being applied to. `null`/omitted for a batch of
   * several links, which has no single format list to reconcile against; the
   * preset is then used verbatim. */
  source?: MediaSource | null;
}

/**
 * FR-231 — fit a saved preset to the link it is being applied to, and report
 * every place the two disagreed.
 *
 * The backend deliberately returns presets exactly as they were saved (see
 * `Preset.output_options`), because only the caller knows which link is on
 * screen. This is that caller. Anything the source cannot deliver is replaced
 * with the nearest thing it can, and each replacement is returned as data so
 * the UI can say what changed instead of quietly producing a different file
 * than the preset's name promises.
 *
 * Silence is the failure mode this exists to prevent: a preset named
 * "Archive 320" applied to a link that only serves 128 kbps must not keep
 * calling itself 320.
 */
export function reconcilePresetOptions(
  preset: OutputOptions,
  { mediaType, source }: ReconcileArgs,
): ReconciledPreset {
  const adjustments: PresetAdjustment[] = [];
  let options: OutputOptions = { ...preset };

  if (!source) {
    return { options, adjustments };
  }

  // --- Audio bitrate: the "quality the source cannot provide" of FR-231. ---
  // Only for an audio job: a video job's audio track is chosen by the format
  // selector, so "reconciling" a bitrate there would report a change that has
  // no effect on the file.
  const wanted = requestedBitrate(options.audio);
  if (mediaType === "audio" && wanted != null && source.available_audio_formats.length > 0) {
    const offered = source.available_audio_formats
      .map((format) => format.bitrate_kbps)
      .filter((bitrate): bitrate is number => bitrate != null);

    if (offered.length === 0) {
      // The link publishes audio but labels it with no bitrate at all (TikTok's
      // pre-muxed formats, for instance). There is no nearest value to fall
      // back to, so fall back to "no explicit target" rather than keeping a
      // number this link never offered.
      options = { ...options, audio: withBitrate(options.audio, null) };
      adjustments.push({ kind: "audio_bitrate_unavailable", from: wanted });
    } else if (!offered.includes(wanted)) {
      const nearest = nearestBitrate(offered, wanted);
      options = { ...options, audio: withBitrate(options.audio, nearest) };
      adjustments.push({ kind: "audio_bitrate", from: wanted, to: nearest });
    }
  }

  // --- Subtitles: a saved language list vs. the ones this link really has. ---
  const tracks = source.subtitles;
  const subtitles = options.subtitles;
  if (subtitles && subtitles.languages.length > 0 && tracks != null) {
    const kept = subtitles.languages.filter((language) =>
      tracks.some((track) => track.language === language),
    );
    const dropped = subtitles.languages.filter((language) => !kept.includes(language));

    if (dropped.length > 0) {
      const next: SubtitleOptions = {
        ...subtitles,
        languages: kept,
        // Recomputed rather than carried over: the flag describes the kept
        // list, and keeping it set for a list that no longer contains an
        // auto-generated language would pull in captions nobody asked for.
        include_auto_generated: kept.some((language) =>
          tracks.some((track) => track.language === language && track.auto_generated),
        ),
      };
      options = { ...options, subtitles: next };
      adjustments.push(
        tracks.length === 0
          ? { kind: "subtitles_unavailable", dropped }
          : { kind: "subtitle_languages_dropped", dropped, kept },
      );
    }
  }

  // --- FR-220 — an embedded track needs a container that can hold one. ---
  const afterSubtitles = options.subtitles;
  if (
    afterSubtitles &&
    afterSubtitles.languages.length > 0 &&
    afterSubtitles.delivery === "embedded" &&
    !supportsEmbeddedSubtitles(mediaType, options)
  ) {
    options = {
      ...options,
      subtitles: { ...afterSubtitles, delivery: "separate_files" },
    };
    adjustments.push({ kind: "subtitles_not_embeddable" });
  }

  // --- FR-225 — chapter split needs chapters. ---
  // `null` (never checked) is left alone on purpose: it is not evidence that
  // there are none, and yt-dlp simply produces one file when a video turns out
  // to have no chapter list.
  if (options.segment?.mode === "split_chapters" && source.chapters?.length === 0) {
    options = { ...options, segment: { mode: "whole" } };
    adjustments.push({ kind: "chapters_unavailable" });
  }

  return { options, adjustments };
}

/** One adjustment as a sentence for the user (FR-231's "say what changed"). */
export function presetAdjustmentMessage(t: TFunction, adjustment: PresetAdjustment): string {
  switch (adjustment.kind) {
    case "audio_bitrate":
      return t("downloadForm.presets_adjusted_bitrate", {
        from: adjustment.from,
        to: adjustment.to,
      });
    case "audio_bitrate_unavailable":
      return t("downloadForm.presets_adjusted_bitrate_unavailable", { from: adjustment.from });
    case "subtitle_languages_dropped":
      return t("downloadForm.presets_adjusted_subtitles_dropped", {
        languages: adjustment.dropped.join(", "),
        count: adjustment.dropped.length,
      });
    case "subtitles_unavailable":
      return t("downloadForm.presets_adjusted_subtitles_unavailable");
    case "subtitles_not_embeddable":
      return t("downloadForm.presets_adjusted_subtitles_not_embeddable");
    case "chapters_unavailable":
      return t("downloadForm.presets_adjusted_chapters_unavailable");
  }
}
