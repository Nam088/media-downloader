import { useState } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown, Info, Sliders, Zap } from "lucide-react";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Switch } from "@/components/ui/switch";
import {
  AUDIO_FORMATS,
  VIDEO_CONTAINERS,
  audioFormatLabel,
  forcesConversion,
  videoContainerLabel,
  videoOutputDetail,
  type AudioFormat,
} from "@/lib/output-format-labels";
import { PresetManager } from "@/components/PresetManager";
import { SegmentOptionsPicker } from "@/components/SegmentOptionsPicker";
import { SubtitleOptionsPicker } from "@/components/SubtitleOptionsPicker";
import {
  NEW_JOB_SEGMENT_MODE,
  NEW_JOB_SUBTITLE_OPTIONS,
  supportsBitrate,
  supportsCoverArt,
  supportsEmbeddedSubtitles,
} from "@/types/download";
import type {
  AudioOutput,
  CodecPreference,
  MediaSource,
  MediaType,
  OutputOptions,
  VideoContainer,
} from "@/types/download";

const CODEC_PREFERENCES: CodecPreference[] = ["compatibility", "quality"];

const CODEC_PREFERENCE_LABEL_KEY: Record<CodecPreference, string> = {
  compatibility: "downloadForm.output_codec_compatibility",
  quality: "downloadForm.output_codec_quality",
};

const CODEC_PREFERENCE_HINT_KEY: Record<CodecPreference, string> = {
  compatibility: "downloadForm.output_codec_compatibility_hint",
  quality: "downloadForm.output_codec_quality_hint",
};

/** What the *encoder* is asked to aim for — a user-chosen target, not a list
 * of the source's real formats, so unlike the quality list (FR-004/FR-019)
 * these are legitimately fixed. `null` is "no explicit target": the backend
 * then falls back to the quality label already matched against the source
 * (see `audio_quality_arg` in `downloader/queue.rs`). */
const BITRATE_CHOICES: (number | null)[] = [null, 320, 256, 192, 128];

const AUTO_BITRATE_VALUE = "auto";

/** The value carried across a format change, so switching MP3 → Opus doesn't
 * silently reset a bitrate the user picked. */
function currentBitrate(audio: AudioOutput): number | null {
  return "bitrate_kbps" in audio ? (audio.bitrate_kbps ?? null) : null;
}

/** Each branch is written out rather than `{ format, bitrate_kbps }` because
 * the union has no `bitrate_kbps` on WAV/FLAC/source by design (FR-203) —
 * spelling the members out is what keeps that guarantee checkable. */
function withAudioFormat(current: AudioOutput, format: AudioFormat): AudioOutput {
  const bitrate_kbps = currentBitrate(current);
  switch (format) {
    case "mp3":
      return { format: "mp3", bitrate_kbps };
    case "m4a":
      return { format: "m4a", bitrate_kbps };
    case "opus":
      return { format: "opus", bitrate_kbps };
    case "wav":
      return { format: "wav" };
    case "flac":
      return { format: "flac" };
    case "source":
      return { format: "source" };
  }
}

function withBitrate(current: AudioOutput, bitrate_kbps: number | null): AudioOutput {
  switch (current.format) {
    case "mp3":
      return { format: "mp3", bitrate_kbps };
    case "m4a":
      return { format: "m4a", bitrate_kbps };
    case "opus":
      return { format: "opus", bitrate_kbps };
    default:
      // Unreachable through the UI (the control is hidden), and a no-op if it
      // ever isn't: a lossless format has nowhere to put the number.
      return current;
  }
}

/** FR-210 — why cover art cannot be embedded into this particular choice.
 * `null` when it can. Mirrors `queue::thumbnail_support`'s two reasons. */
function coverArtBlockedReasonKey(
  mediaType: MediaType,
  options: OutputOptions,
): string | null {
  if (supportsCoverArt(mediaType, options)) return null;
  return mediaType === "audio" && options.audio.format === "wav"
    ? "downloadForm.output_cover_art_unavailable_wav"
    : "downloadForm.output_cover_art_unavailable_source";
}

/** FR-220 — why a subtitle track cannot be embedded into this particular
 * choice. `null` when it can. Mirrors `queue::subtitle_embed_support`. */
function subtitleEmbedBlockedReasonKey(
  mediaType: MediaType,
  options: OutputOptions,
): string | null {
  if (supportsEmbeddedSubtitles(mediaType, options)) return null;
  return mediaType === "audio"
    ? "downloadForm.subtitles_embed_unavailable_audio"
    : "downloadForm.subtitles_embed_unavailable_source";
}

export interface OutputOptionsPickerProps {
  /** Decides which controls even apply: audio jobs never use the container,
   * video jobs never use the audio format, gallery jobs use none of it. */
  mediaType: MediaType;
  value: OutputOptions;
  onChange: (next: OutputOptions) => void;
  /**
   * The link these options will be applied to, when there is exactly one.
   *
   * Supplies the three source-dependent facts the sub-pickers need — the
   * subtitle list, the chapter list, and the duration — and the format list
   * a preset is reconciled against (FR-231). `null`/omitted is the batch case:
   * several links share one set of options, so nothing here is knowable, and
   * every control that depends on the source says so rather than guessing.
   */
  source?: MediaSource | null;
}

/**
 * The output-format picker (FR-201 → FR-211), kept collapsed by default: the
 * spec's own assumption is that most people never open it, so the basic flow
 * has to stay a paste-preview-download.
 *
 * What is *not* collapsed is the one line saying whether a conversion will
 * run (FR-207) — hiding that behind the toggle would mean the users who never
 * open the section are exactly the ones never told.
 */
export function OutputOptionsPicker({
  mediaType,
  value,
  onChange,
  source,
}: OutputOptionsPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  // FR-234: a gallery-dl job never reaches yt-dlp, so none of these choices
  // are read. Hiding them beats rendering controls whose input is discarded.
  if (mediaType === "gallery") return null;

  /**
   * Every change leaves through here so one rule cannot be applied by some
   * controls and forgotten by others: switching the container to "keep source"
   * takes away the ability to embed a subtitle track (FR-220), and a delivery
   * choice made before that switch would otherwise survive as a request the
   * backend silently drops.
   */
  function emit(next: OutputOptions) {
    const subtitles = next.subtitles;
    if (
      subtitles &&
      subtitles.delivery === "embedded" &&
      !supportsEmbeddedSubtitles(mediaType, next)
    ) {
      onChange({ ...next, subtitles: { ...subtitles, delivery: "separate_files" } });
      return;
    }
    onChange(next);
  }

  const isAudio = mediaType === "audio";
  const bitrate = currentBitrate(value.audio);
  const summary = isAudio
    ? bitrate == null
      ? audioFormatLabel(t, value.audio.format)
      : t("downloadForm.output_summary_with_bitrate", {
          format: audioFormatLabel(t, value.audio.format),
          kbps: bitrate,
        })
    : videoOutputDetail(t, value);
  const converting = forcesConversion(mediaType, value);
  const coverArtBlockedKey = coverArtBlockedReasonKey(mediaType, value);

  return (
    <div className="flex flex-col gap-2.5 rounded-lg border border-border/70 bg-muted/20 p-3.5">
      <button
        type="button"
        onClick={() => setOpen((current) => !current)}
        aria-expanded={open}
        className="flex w-full items-center gap-3 text-left"
      >
        <Sliders className="h-4 w-4 shrink-0 text-primary" />
        <span className="text-xs font-semibold tracking-tight text-foreground/80">
          {t("downloadForm.output_options_toggle")}
        </span>
        <span className="min-w-0 flex-1 truncate text-right text-xs font-medium text-muted-foreground">
          {summary}
        </span>
        <ChevronDown
          className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>

      {/* FR-207. Both branches are stated, so "no conversion" is something the
          user can read rather than infer from the absence of a warning. */}
      <p className="flex items-start gap-1.5 text-xs leading-relaxed text-muted-foreground">
        {converting ? (
          <Info className="mt-0.5 h-3.5 w-3.5 shrink-0 text-amber-500" />
        ) : (
          <Zap className="mt-0.5 h-3.5 w-3.5 shrink-0 text-primary" />
        )}
        <span>
          {converting
            ? isAudio
              ? t("downloadForm.output_conversion_warning_audio", {
                  format: audioFormatLabel(t, value.audio.format),
                })
              : t("downloadForm.output_conversion_warning_video", {
                  container: videoContainerLabel(t, value.video_container),
                })
            : t("downloadForm.output_no_conversion_hint")}
        </span>
      </p>

      {open && (
        <div className="flex flex-col gap-4 border-t border-border/60 pt-3">
          {/* FR-228→FR-233. Inside the collapsed section on purpose: presets
              are an advanced-user tool, and the spec's assumptions say the
              basic flow must not grow a control for them. */}
          <PresetManager
            value={value}
            onApply={onChange}
            mediaType={mediaType}
            source={source}
          />

          {isAudio ? (
            <>
              <div className="flex flex-col gap-2">
                <Label className="text-xs font-semibold tracking-tight text-foreground/80">
                  {t("downloadForm.output_audio_format_label")}
                </Label>
                <RadioGroup
                  value={value.audio.format}
                  onValueChange={(next) =>
                    emit({ ...value, audio: withAudioFormat(value.audio, next as AudioFormat) })
                  }
                  className="grid grid-cols-3 gap-1.5"
                >
                  {AUDIO_FORMATS.map((format) => (
                    <label
                      key={format}
                      htmlFor={`audio-format-${format}`}
                      className={`flex cursor-pointer items-center gap-2 rounded-md border border-border/80 bg-card px-2.5 py-2 text-xs font-semibold transition-all hover:border-primary/40 ${
                        value.audio.format === format ? "border-primary bg-primary/5" : ""
                      }`}
                    >
                      <RadioGroupItem value={format} id={`audio-format-${format}`} />
                      <span className="truncate">{audioFormatLabel(t, format)}</span>
                    </label>
                  ))}
                </RadioGroup>
              </div>

              {/* FR-203: driven by the shared predicate, so the control cannot
                  drift from the type that decides which members even have a
                  bitrate field. */}
              {supportsBitrate(value.audio) && (
                <div className="flex flex-col gap-2">
                  <Label className="text-xs font-semibold tracking-tight text-foreground/80">
                    {t("downloadForm.output_bitrate_label")}
                  </Label>
                  <RadioGroup
                    value={bitrate == null ? AUTO_BITRATE_VALUE : String(bitrate)}
                    onValueChange={(next) =>
                      emit({
                        ...value,
                        audio: withBitrate(
                          value.audio,
                          next === AUTO_BITRATE_VALUE ? null : Number(next),
                        ),
                      })
                    }
                    className="flex flex-wrap gap-1.5"
                  >
                    {BITRATE_CHOICES.map((choice) => {
                      const optionValue = choice == null ? AUTO_BITRATE_VALUE : String(choice);
                      return (
                        <label
                          key={optionValue}
                          htmlFor={`audio-bitrate-${optionValue}`}
                          className={`flex cursor-pointer items-center gap-2 rounded-md border border-border/80 bg-card px-2.5 py-2 text-xs font-semibold transition-all hover:border-primary/40 ${
                            optionValue === (bitrate == null ? AUTO_BITRATE_VALUE : String(bitrate))
                              ? "border-primary bg-primary/5"
                              : ""
                          }`}
                        >
                          <RadioGroupItem value={optionValue} id={`audio-bitrate-${optionValue}`} />
                          <span>
                            {choice == null
                              ? t("downloadForm.output_bitrate_auto")
                              : t("downloadForm.output_bitrate_option", { kbps: choice })}
                          </span>
                        </label>
                      );
                    })}
                  </RadioGroup>
                </div>
              )}
            </>
          ) : (
            <>
              <div className="flex flex-col gap-2">
                <Label className="text-xs font-semibold tracking-tight text-foreground/80">
                  {t("downloadForm.output_container_label")}
                </Label>
                <RadioGroup
                  value={value.video_container}
                  onValueChange={(next) =>
                    emit({ ...value, video_container: next as VideoContainer })
                  }
                  className="grid grid-cols-3 gap-1.5"
                >
                  {VIDEO_CONTAINERS.map((container) => (
                    <label
                      key={container}
                      htmlFor={`video-container-${container}`}
                      className={`flex cursor-pointer items-center gap-2 rounded-md border border-border/80 bg-card px-2.5 py-2 text-xs font-semibold transition-all hover:border-primary/40 ${
                        value.video_container === container ? "border-primary bg-primary/5" : ""
                      }`}
                    >
                      <RadioGroupItem value={container} id={`video-container-${container}`} />
                      <span className="truncate">{videoContainerLabel(t, container)}</span>
                    </label>
                  ))}
                </RadioGroup>
              </div>

              <div className="flex flex-col gap-2">
                <Label className="text-xs font-semibold tracking-tight text-foreground/80">
                  {t("downloadForm.output_codec_label")}
                </Label>
                <RadioGroup
                  value={value.codec_preference}
                  onValueChange={(next) =>
                    emit({ ...value, codec_preference: next as CodecPreference })
                  }
                  className="gap-1.5"
                >
                  {CODEC_PREFERENCES.map((preference) => (
                    <label
                      key={preference}
                      htmlFor={`codec-preference-${preference}`}
                      className={`flex cursor-pointer items-center gap-3 rounded-md border border-border/80 bg-card px-3 py-2 text-xs transition-all hover:border-primary/40 ${
                        value.codec_preference === preference ? "border-primary bg-primary/5" : ""
                      }`}
                    >
                      <RadioGroupItem value={preference} id={`codec-preference-${preference}`} />
                      <span className="w-28 shrink-0 font-semibold">
                        {t(CODEC_PREFERENCE_LABEL_KEY[preference])}
                      </span>
                      <span className="flex-1 text-muted-foreground">
                        {t(CODEC_PREFERENCE_HINT_KEY[preference])}
                      </span>
                    </label>
                  ))}
                </RadioGroup>
              </div>
            </>
          )}

          <div className="flex flex-col gap-3 border-t border-border/60 pt-3">
            <div className="flex items-start justify-between gap-4">
              <div className="flex min-w-0 flex-col gap-0.5">
                <Label htmlFor="embed-metadata" className="text-xs font-semibold">
                  {t("downloadForm.output_embed_metadata_label")}
                </Label>
                <span className="text-xs text-muted-foreground">
                  {t("downloadForm.output_embed_metadata_hint")}
                </span>
              </div>
              <Switch
                id="embed-metadata"
                checked={value.embed_metadata}
                onCheckedChange={(checked) => emit({ ...value, embed_metadata: checked })}
              />
            </div>

            <div className="flex items-start justify-between gap-4">
              <div className="flex min-w-0 flex-col gap-0.5">
                <Label htmlFor="embed-thumbnail" className="text-xs font-semibold">
                  {t("downloadForm.output_embed_cover_art_label")}
                </Label>
                {/* FR-210/SC-209: the control stays visible and says why it
                    cannot apply, instead of vanishing and leaving the user to
                    wonder where their cover art went. */}
                <span className="text-xs text-muted-foreground">
                  {coverArtBlockedKey
                    ? t(coverArtBlockedKey)
                    : t("downloadForm.output_embed_cover_art_hint")}
                </span>
              </div>
              <Switch
                id="embed-thumbnail"
                checked={value.embed_thumbnail && coverArtBlockedKey === null}
                disabled={coverArtBlockedKey !== null}
                onCheckedChange={(checked) => emit({ ...value, embed_thumbnail: checked })}
              />
            </div>
          </div>

          {/* FR-217→FR-221. The three states of `source.subtitles` are passed
              straight through; `source` being absent (a batch) is itself the
              "not checked" state, which is exactly what it means. */}
          <SubtitleOptionsPicker
            tracks={source?.subtitles}
            value={value.subtitles ?? NEW_JOB_SUBTITLE_OPTIONS}
            onChange={(subtitles) => emit({ ...value, subtitles })}
            embedSupported={supportsEmbeddedSubtitles(mediaType, value)}
            embedBlockedReasonKey={subtitleEmbedBlockedReasonKey(mediaType, value)}
          />

          {/* FR-222→FR-227. */}
          <SegmentOptionsPicker
            value={value.segment ?? NEW_JOB_SEGMENT_MODE}
            onChange={(segment) => emit({ ...value, segment })}
            chapters={source?.chapters}
            durationSeconds={source?.duration_seconds}
          />
        </div>
      )}
    </div>
  );
}
