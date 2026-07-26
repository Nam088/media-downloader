import type { TFunction } from "i18next";

import type {
  AudioOutput,
  MediaType,
  OutputOptions,
  VideoContainer,
} from "@/types/download";

export type AudioFormat = AudioOutput["format"];

/** Order the formats are offered in: the two everyday lossy choices first,
 * then the archival ones, then "don't touch my file" last (FR-201). */
export const AUDIO_FORMATS: AudioFormat[] = ["mp3", "m4a", "opus", "wav", "flac", "source"];

const AUDIO_FORMAT_LABEL_KEY: Record<AudioFormat, string> = {
  mp3: "downloadForm.output_audio_format_mp3",
  m4a: "downloadForm.output_audio_format_m4a",
  opus: "downloadForm.output_audio_format_opus",
  wav: "downloadForm.output_audio_format_wav",
  flac: "downloadForm.output_audio_format_flac",
  source: "downloadForm.output_audio_format_source",
};

export const VIDEO_CONTAINERS: VideoContainer[] = ["mp4", "mkv", "source"];

const VIDEO_CONTAINER_LABEL_KEY: Record<VideoContainer, string> = {
  mp4: "downloadForm.output_container_mp4",
  mkv: "downloadForm.output_container_mkv",
  source: "downloadForm.output_container_source",
};

export function audioFormatLabel(t: TFunction, format: AudioFormat): string {
  return t(AUDIO_FORMAT_LABEL_KEY[format]);
}

export function videoContainerLabel(t: TFunction, container: VideoContainer): string {
  return t(VIDEO_CONTAINER_LABEL_KEY[container]);
}

/**
 * FR-206 — the detail line on one audio quality row, describing the file the
 * user will actually get.
 *
 * Both halves are real: `sourceCodec` is what yt-dlp reported for that
 * specific format of that specific link, and the target comes from the current
 * output selection. The old label read `MP3 / ${codec}`, which printed
 * "MP3 / OPUS" — a file that cannot exist — because the "MP3" half was a
 * constant that had nothing to do with what would be produced.
 */
export function audioOutputDetail(
  t: TFunction,
  sourceCodec: string,
  audio: AudioOutput,
): string {
  const from =
    sourceCodec.trim().length > 0
      ? sourceCodec.toUpperCase()
      : t("downloadForm.output_source_codec_unknown");
  return audio.format === "source"
    ? t("downloadForm.output_detail_keep_source", { format: from })
    : t("downloadForm.output_detail_converted", {
        from,
        to: audioFormatLabel(t, audio.format),
      });
}

/**
 * FR-206 for a video quality row. Nothing here is a constant string: the
 * container comes from the picker, and the codec half reflects which `-f`
 * chain `video_format_selector` will build — "compatibility" constrains the
 * selection to avc1 + mp4a, "quality" deliberately drops that constraint and
 * takes whatever the source ranks highest (VP9/AV1 included), which is exactly
 * why it cannot be labelled H.264.
 */
export function videoOutputDetail(t: TFunction, options: OutputOptions): string {
  return t("downloadForm.output_detail_video", {
    container: videoContainerLabel(t, options.video_container),
    codec: t(
      options.codec_preference === "compatibility"
        ? "downloadForm.output_codec_detail_compatibility"
        : "downloadForm.output_codec_detail_quality",
    ),
  });
}

/**
 * FR-207 — whether the current selection makes the backend run a conversion
 * step after the download.
 *
 * Mirrors `build_ytdlp_args`: an audio format other than "source" adds
 * `-x --audio-format …`, and a container other than "source" adds
 * `--merge-output-format …`. "Keep source" adds neither, so ffmpeg is never
 * invoked to reshape the file — the reason SC-202 expects it to be faster.
 */
export function forcesConversion(mediaType: MediaType, options: OutputOptions): boolean {
  if (mediaType === "audio") return options.audio.format !== "source";
  if (mediaType === "video") return options.video_container !== "source";
  return false;
}
