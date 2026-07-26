import { describe, expect, it } from "vitest";

import { reconcilePresetOptions } from "@/lib/preset-reconcile";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type { MediaSource, OutputOptions } from "@/types/download";

function sourceWith(overrides: Partial<MediaSource> = {}): MediaSource {
  return {
    source_url: "https://youtube.com/watch?v=abc",
    title: "Sample",
    thumbnail_url: null,
    duration_seconds: 300,
    platform: "youtube",
    is_playlist: false,
    playlist_item_count: null,
    available_video_qualities: [{ label: "720p", filesize_bytes: null }],
    available_audio_formats: [
      { bitrate_kbps: 128, codec: "opus", filesize_bytes: null },
      { bitrate_kbps: 64, codec: "opus", filesize_bytes: null },
    ],
    is_gallery: false,
    gallery_items: [],
    playlist_entries: [],
    subtitles: [],
    chapters: [],
    ...overrides,
  };
}

const PRESET_320: OutputOptions = {
  ...NEW_JOB_OUTPUT_OPTIONS,
  audio: { format: "mp3", bitrate_kbps: 320 },
};

describe("reconcilePresetOptions (FR-231)", () => {
  it("leaves a preset alone when the source can deliver all of it", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith(),
    });

    expect(result.options).toEqual(preset);
    expect(result.adjustments).toEqual([]);
  });

  it("falls back to the nearest bitrate the source really offers", () => {
    const result = reconcilePresetOptions(PRESET_320, {
      mediaType: "audio",
      source: sourceWith(),
    });

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: 128 });
    expect(result.adjustments).toEqual([{ kind: "audio_bitrate", from: 320, to: 128 }]);
  });

  // "Nearest" has to mean nearest, not "the highest" and not "the first".
  it("picks the nearest offered bitrate below the requested one", () => {
    const result = reconcilePresetOptions(
      { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 130 } },
      {
        mediaType: "audio",
        source: sourceWith({
          available_audio_formats: [
            { bitrate_kbps: 320, codec: "aac", filesize_bytes: null },
            { bitrate_kbps: 128, codec: "aac", filesize_bytes: null },
          ],
        }),
      },
    );

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: 128 });
  });

  it("breaks an exact tie towards the higher bitrate", () => {
    const result = reconcilePresetOptions(
      { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 192 } },
      {
        mediaType: "audio",
        source: sourceWith({
          available_audio_formats: [
            { bitrate_kbps: 128, codec: "aac", filesize_bytes: null },
            { bitrate_kbps: 256, codec: "aac", filesize_bytes: null },
          ],
        }),
      },
    );

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: 256 });
  });

  // A link with audio but no published bitrate (TikTok's pre-muxed formats):
  // there is no nearest number, so keeping the preset's would be a claim the
  // source never made.
  it("drops the target entirely when the source publishes no bitrate at all", () => {
    const result = reconcilePresetOptions(PRESET_320, {
      mediaType: "audio",
      source: sourceWith({
        available_audio_formats: [{ bitrate_kbps: null, codec: "aac", filesize_bytes: null }],
      }),
    });

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: null });
    expect(result.adjustments).toEqual([{ kind: "audio_bitrate_unavailable", from: 320 }]);
  });

  it("says nothing about a bitrate when the source published no audio list to check", () => {
    const result = reconcilePresetOptions(PRESET_320, {
      mediaType: "audio",
      source: sourceWith({ available_audio_formats: [] }),
    });

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: 320 });
    expect(result.adjustments).toEqual([]);
  });

  it("leaves a lossless preset's audio untouched — there is no bitrate to fit", () => {
    const preset: OutputOptions = { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "flac" } };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith(),
    });

    expect(result.options.audio).toEqual({ format: "flac" });
    expect(result.adjustments).toEqual([]);
  });

  // The audio bitrate of a video job is chosen by the format selector, so
  // "adjusting" it would report a change with no effect on the file.
  it("does not touch the bitrate of a video job", () => {
    const result = reconcilePresetOptions(PRESET_320, {
      mediaType: "video",
      source: sourceWith(),
    });

    expect(result.options.audio).toEqual({ format: "mp3", bitrate_kbps: 320 });
    expect(result.adjustments).toEqual([]);
  });

  it("drops subtitle languages the source does not offer, keeping the rest", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      subtitles: { languages: ["vi", "fr"], delivery: "separate_files", include_auto_generated: false },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({
        subtitles: [{ language: "vi", label: "Vietnamese", auto_generated: false }],
      }),
    });

    expect(result.options.subtitles?.languages).toEqual(["vi"]);
    expect(result.adjustments).toEqual([
      { kind: "subtitle_languages_dropped", dropped: ["fr"], kept: ["vi"] },
    ]);
  });

  it("recomputes the automatic-captions flag from what survived", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      subtitles: { languages: ["ja", "vi"], delivery: "separate_files", include_auto_generated: true },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({
        subtitles: [{ language: "vi", label: "Vietnamese", auto_generated: false }],
      }),
    });

    // Only the author-provided language is left, so nothing should still be
    // asking yt-dlp for machine transcriptions.
    expect(result.options.subtitles).toEqual({
      languages: ["vi"],
      delivery: "separate_files",
      include_auto_generated: false,
    });
  });

  it("reports an empty subtitle list differently from a partial mismatch (FR-221)", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      subtitles: { languages: ["vi"], delivery: "separate_files", include_auto_generated: false },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({ subtitles: [] }),
    });

    expect(result.options.subtitles?.languages).toEqual([]);
    expect(result.adjustments).toEqual([{ kind: "subtitles_unavailable", dropped: ["vi"] }]);
  });

  // `null` is "nobody checked", which is not evidence that the languages are
  // missing — dropping them here would silently discard a working choice.
  it("keeps subtitle languages when the source's list was never checked", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      subtitles: { languages: ["vi"], delivery: "separate_files", include_auto_generated: false },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({ subtitles: null }),
    });

    expect(result.options.subtitles?.languages).toEqual(["vi"]);
    expect(result.adjustments).toEqual([]);
  });

  it("moves embedded subtitles to separate files when the output cannot hold them (FR-220)", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      video_container: "source",
      subtitles: { languages: ["vi"], delivery: "embedded", include_auto_generated: false },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "video",
      source: sourceWith({
        subtitles: [{ language: "vi", label: "Vietnamese", auto_generated: false }],
      }),
    });

    expect(result.options.subtitles?.delivery).toBe("separate_files");
    expect(result.adjustments).toEqual([{ kind: "subtitles_not_embeddable" }]);
  });

  it("keeps embedded subtitles when the container can hold them", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      video_container: "mkv",
      subtitles: { languages: ["vi"], delivery: "embedded", include_auto_generated: false },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "video",
      source: sourceWith({
        subtitles: [{ language: "vi", label: "Vietnamese", auto_generated: false }],
      }),
    });

    expect(result.options.subtitles?.delivery).toBe("embedded");
    expect(result.adjustments).toEqual([]);
  });

  it("turns a chapter split back into one whole file when the source has no chapters (FR-225)", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      segment: { mode: "split_chapters" },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({ chapters: [] }),
    });

    expect(result.options.segment).toEqual({ mode: "whole" });
    expect(result.adjustments).toEqual([{ kind: "chapters_unavailable" }]);
  });

  it("keeps a chapter split when the source has chapters", () => {
    const preset: OutputOptions = {
      ...NEW_JOB_OUTPUT_OPTIONS,
      audio: { format: "mp3", bitrate_kbps: 128 },
      segment: { mode: "split_chapters" },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({
        chapters: [{ title: "Intro", start_seconds: 0, end_seconds: 10 }],
      }),
    });

    expect(result.options.segment).toEqual({ mode: "split_chapters" });
    expect(result.adjustments).toEqual([]);
  });

  // A batch has no single format list, no single subtitle list and no single
  // chapter list: there is nothing to reconcile against, and inventing an
  // answer from one of the links would be worse than applying the preset.
  it("applies the preset verbatim when there is no single source", () => {
    const preset: OutputOptions = {
      ...PRESET_320,
      subtitles: { languages: ["vi"], delivery: "embedded", include_auto_generated: false },
      segment: { mode: "split_chapters" },
    };

    const result = reconcilePresetOptions(preset, { mediaType: "audio", source: null });

    expect(result.options).toEqual(preset);
    expect(result.adjustments).toEqual([]);
  });

  it("never mutates the preset it was handed", () => {
    const preset: OutputOptions = {
      ...PRESET_320,
      subtitles: { languages: ["vi", "fr"], delivery: "separate_files", include_auto_generated: false },
    };
    const before = structuredClone(preset);

    reconcilePresetOptions(preset, { mediaType: "audio", source: sourceWith() });

    expect(preset).toEqual(before);
  });

  it("reports every mismatch, not just the first one", () => {
    const preset: OutputOptions = {
      ...PRESET_320,
      subtitles: { languages: ["fr"], delivery: "separate_files", include_auto_generated: false },
      segment: { mode: "split_chapters" },
    };

    const result = reconcilePresetOptions(preset, {
      mediaType: "audio",
      source: sourceWith({ subtitles: [], chapters: [] }),
    });

    expect(result.adjustments.map((adjustment) => adjustment.kind)).toEqual([
      "audio_bitrate",
      "subtitles_unavailable",
      "chapters_unavailable",
    ]);
  });
});
