import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AlertTriangle, Scissors } from "lucide-react";

import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Switch } from "@/components/ui/switch";
import { formatSeconds, parseTimeInput } from "@/lib/trim-input";
import { validateTrimRange } from "@/types/download";
import type { ChapterPreview, SegmentMode } from "@/types/download";

/** The three shapes `SegmentMode` can take, in the order they are offered. */
const MODES: SegmentMode["mode"][] = ["whole", "trim", "split_chapters"];

const MODE_LABEL_KEY: Record<SegmentMode["mode"], string> = {
  whole: "downloadForm.segment_whole",
  trim: "downloadForm.segment_trim",
  split_chapters: "downloadForm.segment_split_chapters",
};

const MODE_HINT_KEY: Record<SegmentMode["mode"], string> = {
  whole: "downloadForm.segment_whole_hint",
  trim: "downloadForm.segment_trim_hint",
  split_chapters: "downloadForm.segment_split_chapters_hint",
};

const TRIM_ERROR_KEY: Record<
  NonNullable<ReturnType<typeof validateTrimRange>>,
  string
> = {
  empty: "downloadForm.segment_error_empty",
  negative: "downloadForm.segment_error_negative",
  end_before_start: "downloadForm.segment_error_end_before_start",
  beyond_duration: "downloadForm.segment_error_beyond_duration",
};

export interface SegmentOptionsPickerProps {
  value: SegmentMode;
  onChange: (next: SegmentMode) => void;
  /**
   * `MediaSource.chapters`, unchanged — same three states as the subtitle list:
   * `null`/`undefined` means nobody checked, `[]` means checked and there are
   * none, non-empty is the real list whose length is shown to the user
   * (FR-225).
   */
  chapters: ChapterPreview[] | null | undefined;
  /** Used only to catch a start time past the end of the content; the backend
   * cannot make this check because duration lives on the preview, not the job. */
  durationSeconds?: number | null;
}

/**
 * Trim / chapter-split picker (FR-222 → FR-227).
 *
 * One `RadioGroup` over a discriminated union, not two independent toggles:
 * FR-226 forbids trimming and chapter-splitting at once, and picking a radio
 * makes that combination unreachable by construction rather than by a rule
 * some future edit has to remember. The type has nowhere to put both.
 */
export function SegmentOptionsPicker({
  value,
  onChange,
  chapters,
  durationSeconds,
}: SegmentOptionsPickerProps) {
  const { t } = useTranslation();
  const trim = value.mode === "trim" ? value : null;

  // The boxes keep what was typed, so a half-entered "1:" stays on screen
  // instead of being rewritten mid-keystroke by the parsed value coming back
  // down as a prop. `null` means "not being edited", in which case the box
  // simply shows the value in `value` — that is what makes a preset's saved
  // range appear in the boxes without an effect mirroring props into state.
  const [startDraft, setStartDraft] = useState<string | null>(null);
  const [endDraft, setEndDraft] = useState<string | null>(null);

  const startText =
    startDraft ?? (trim?.start_seconds != null ? formatSeconds(trim.start_seconds) : "");
  const endText = endDraft ?? (trim?.end_seconds != null ? formatSeconds(trim.end_seconds) : "");

  const chapterCount = chapters?.length ?? 0;
  const chaptersAvailable = chapterCount > 0;
  const chapterBlockedKey =
    chapters == null
      ? "downloadForm.segment_chapters_unknown"
      : "downloadForm.segment_chapters_none";

  const trimError = trim ? validateTrimRange(trim, durationSeconds) : null;

  function selectMode(mode: SegmentMode["mode"]) {
    if (mode !== "trim") {
      setStartDraft(null);
      setEndDraft(null);
    }
    if (mode === "whole") onChange({ mode: "whole" });
    else if (mode === "split_chapters") onChange({ mode: "split_chapters" });
    else onChange({ mode: "trim", start_seconds: null, end_seconds: null, accurate_cut: false });
  }

  function setBound(bound: "start_seconds" | "end_seconds", raw: string) {
    if (!trim) return;
    if (bound === "start_seconds") setStartDraft(raw);
    else setEndDraft(raw);
    onChange({ ...trim, [bound]: parseTimeInput(raw) });
  }

  return (
    <div className="flex flex-col gap-2.5 border-t border-border/60 pt-3">
      <div className="flex items-center gap-2">
        <Scissors className="h-4 w-4 shrink-0 text-primary" />
        <Label className="text-xs font-semibold tracking-tight text-foreground/80">
          {t("downloadForm.segment_label")}
        </Label>
      </div>

      <RadioGroup
        value={value.mode}
        onValueChange={(next) => selectMode(next as SegmentMode["mode"])}
        className="gap-1.5"
      >
        {MODES.map((mode) => {
          // FR-225: offered only when there really are chapters. The option
          // stays visible and explains itself rather than disappearing, so a
          // user who came looking for it learns why it is not there (SC-209).
          const blocked = mode === "split_chapters" && !chaptersAvailable;
          return (
            <label
              key={mode}
              htmlFor={`segment-mode-${mode}`}
              className={`flex items-center gap-3 rounded-md border border-border/80 bg-card px-3 py-2 text-xs transition-all ${
                blocked ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:border-primary/40"
              } ${value.mode === mode && !blocked ? "border-primary bg-primary/5" : ""}`}
            >
              <RadioGroupItem value={mode} id={`segment-mode-${mode}`} disabled={blocked} />
              <span className="w-32 shrink-0 font-semibold">
                {mode === "split_chapters" && chaptersAvailable
                  ? t("downloadForm.segment_split_chapters_count", { count: chapterCount })
                  : t(MODE_LABEL_KEY[mode])}
              </span>
              <span className="flex-1 text-muted-foreground">
                {blocked ? t(chapterBlockedKey) : t(MODE_HINT_KEY[mode])}
              </span>
            </label>
          );
        })}
      </RadioGroup>

      {trim && (
        <div className="flex flex-col gap-3 rounded-md border border-border/70 bg-muted/20 p-3">
          <div className="flex flex-col gap-3 sm:flex-row">
            <div className="flex flex-1 flex-col gap-1.5">
              <Label htmlFor="segment-start" className="text-xs font-semibold">
                {t("downloadForm.segment_start_label")}
              </Label>
              <Input
                id="segment-start"
                value={startText}
                onChange={(event) => setBound("start_seconds", event.target.value)}
                placeholder={t("downloadForm.segment_time_placeholder")}
                aria-invalid={trimError != null}
                className="font-mono"
              />
            </div>
            <div className="flex flex-1 flex-col gap-1.5">
              <Label htmlFor="segment-end" className="text-xs font-semibold">
                {t("downloadForm.segment_end_label")}
              </Label>
              <Input
                id="segment-end"
                value={endText}
                onChange={(event) => setBound("end_seconds", event.target.value)}
                placeholder={t("downloadForm.segment_time_placeholder")}
                aria-invalid={trimError != null}
                className="font-mono"
              />
            </div>
          </div>

          <p className="text-xs text-muted-foreground">{t("downloadForm.segment_time_hint")}</p>

          {/* FR-223: the reason is named at the field, and the same condition
              blocks the download button — see `trimErrorFor`. */}
          {trimError && (
            <p role="alert" className="flex items-start gap-1.5 text-xs font-medium text-destructive">
              <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>{t(TRIM_ERROR_KEY[trimError])}</span>
            </p>
          )}

          <div className="flex items-start justify-between gap-4 border-t border-border/60 pt-3">
            <div className="flex min-w-0 flex-col gap-0.5">
              <Label htmlFor="segment-accurate-cut" className="text-xs font-semibold">
                {t("downloadForm.segment_accurate_label")}
              </Label>
              {/* FR-224 — the cost is stated on the control itself, not buried
                  in documentation nobody opens. */}
              <span className="text-xs text-muted-foreground">
                {t("downloadForm.segment_accurate_hint")}
              </span>
            </div>
            <Switch
              id="segment-accurate-cut"
              checked={trim.accurate_cut ?? false}
              onCheckedChange={(checked) => onChange({ ...trim, accurate_cut: checked })}
            />
          </div>
        </div>
      )}
    </div>
  );
}
