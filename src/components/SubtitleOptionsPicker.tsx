import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Captions, ChevronsUpDown, Search } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { SubtitleDelivery, SubtitleOptions, SubtitleTrackPreview } from "@/types/download";

export interface SubtitleOptionsPickerProps {
  /**
   * `MediaSource.subtitles`, passed through **unchanged** — including the
   * difference between `null` and `[]`, which this component exists to render
   * differently (FR-217/FR-221):
   *
   *   - `null`/`undefined`: nobody checked (a gallery preview, or a flat
   *     playlist whose per-video metadata was never fetched). Saying "no
   *     subtitles" here would be a claim the app cannot make.
   *   - `[]`: checked, and there are none.
   *   - non-empty: the real list.
   *
   * Collapsing the first two into "falsy" is exactly the bug this prop's type
   * refuses to allow.
   */
  tracks: SubtitleTrackPreview[] | null | undefined;
  value: SubtitleOptions;
  onChange: (next: SubtitleOptions) => void;
  /** FR-220 — whether the current output can hold an embedded subtitle track.
   * Computed by the parent from `supportsEmbeddedSubtitles`, so this component
   * never re-derives the rule. */
  embedSupported: boolean;
  /** Locale key explaining why embedding is unavailable; `null` when it is
   * available. The option stays on screen either way (SC-209). */
  embedBlockedReasonKey: string | null;
}

const DELIVERIES: SubtitleDelivery[] = ["separate_files", "embedded"];

const DELIVERY_LABEL_KEY: Record<SubtitleDelivery, string> = {
  separate_files: "downloadForm.subtitles_delivery_separate",
  embedded: "downloadForm.subtitles_delivery_embedded",
};

const DELIVERY_HINT_KEY: Record<SubtitleDelivery, string> = {
  separate_files: "downloadForm.subtitles_delivery_separate_hint",
  embedded: "downloadForm.subtitles_delivery_embedded_hint",
};

/**
 * Subtitle language picker (FR-217 → FR-221).
 *
 * Multi-select by design (FR-218): `languages` is a list, and
 * `include_auto_generated` is one flag over that whole list rather than a
 * per-language one, because that is how yt-dlp takes it — `--sub-langs` is a
 * list and `--write-auto-subs` is a single switch across it. The flag is
 * therefore recomputed from the current selection on every toggle instead of
 * being a control of its own the user could contradict.
 */
export function SubtitleOptionsPicker({
  tracks,
  value,
  onChange,
  embedSupported,
  embedBlockedReasonKey,
}: SubtitleOptionsPickerProps) {
  const { t } = useTranslation();
  const [search, setSearch] = useState("");

  const heading = (
    <div className="flex items-center gap-2">
      <Captions className="h-4 w-4 shrink-0 text-primary" />
      <Label className="text-xs font-semibold tracking-tight text-foreground/80">
        {t("downloadForm.subtitles_label")}
      </Label>
    </div>
  );

  // State 1 of 3: never checked. Not the same claim as "there are none".
  if (tracks == null) {
    return (
      <div className="flex flex-col gap-2 border-t border-border/60 pt-3">
        {heading}
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t("downloadForm.subtitles_unknown")}
        </p>
      </div>
    );
  }

  // State 2 of 3: checked, and this link has none (FR-221).
  if (tracks.length === 0) {
    return (
      <div className="flex flex-col gap-2 border-t border-border/60 pt-3">
        {heading}
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t("downloadForm.subtitles_none")}
        </p>
      </div>
    );
  }

  function toggle(language: string) {
    const languages = value.languages.includes(language)
      ? value.languages.filter((picked) => picked !== language)
      : [...value.languages, language];

    onChange({
      ...value,
      languages,
      include_auto_generated: languages.some((picked) =>
        (tracks ?? []).some((track) => track.language === picked && track.auto_generated),
      ),
    });
  }

  const anyPicked = value.languages.length > 0;
  const delivery = embedSupported ? value.delivery : "separate_files";

  const query = search.trim().toLowerCase();
  const filteredTracks = query
    ? tracks.filter(
        (track) =>
          track.language.toLowerCase().includes(query) ||
          (track.label ?? "").toLowerCase().includes(query),
      )
    : tracks;

  const triggerLabel = anyPicked
    ? t("downloadForm.subtitles_trigger_selected_count", { count: value.languages.length })
    : t("downloadForm.subtitles_trigger_placeholder");

  return (
    <div className="flex flex-col gap-2.5 border-t border-border/60 pt-3">
      {heading}

      <Popover>
        <PopoverTrigger asChild>
          <Button
            type="button"
            variant="outline"
            data-testid="subtitle-language-trigger"
            className="w-full justify-between text-xs font-normal"
          >
            <span className="truncate">{triggerLabel}</span>
            <ChevronsUpDown className="h-3.5 w-3.5 shrink-0 opacity-50" />
          </Button>
        </PopoverTrigger>
        <PopoverContent className="flex flex-col gap-2">
          <div className="relative">
            <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("downloadForm.subtitles_search_placeholder")}
              aria-label={t("downloadForm.subtitles_search_placeholder")}
              className="pl-8"
            />
          </div>

          {filteredTracks.length === 0 && (
            <p className="text-xs leading-relaxed text-muted-foreground">
              {t("downloadForm.subtitles_search_no_results")}
            </p>
          )}

          <div className="flex max-h-64 flex-col gap-1.5 overflow-y-auto">
            {filteredTracks.map((track) => {
              const id = `subtitle-language-${track.language}`;
              const checked = value.languages.includes(track.language);
              return (
                <label
                  key={track.language}
                  htmlFor={id}
                  className={`flex cursor-pointer items-center gap-3 rounded-md border border-border/80 bg-card px-3 py-2 text-xs transition-all hover:border-primary/40 ${
                    checked ? "border-primary bg-primary/5" : ""
                  }`}
                >
                  <input
                    type="checkbox"
                    id={id}
                    checked={checked}
                    onChange={() => toggle(track.language)}
                    className="h-3.5 w-3.5 shrink-0 accent-primary"
                  />
                  {/* The source's own name, or the bare code when it gave none —
                      never a name invented from the code (FR-211). */}
                  <span className="min-w-0 flex-1 truncate font-semibold">
                    {track.label ?? track.language}
                  </span>
                  <span className="shrink-0 font-mono text-muted-foreground">{track.language}</span>
                  {/* FR-217: author-provided and machine-made are two different
                      things, and the difference is visible on every row rather
                      than only on the machine-made ones. */}
                  <span
                    className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold ${
                      track.auto_generated
                        ? "bg-amber-500/15 text-amber-600 dark:text-amber-400"
                        : "bg-primary/10 text-primary"
                    }`}
                  >
                    {t(
                      track.auto_generated
                        ? "downloadForm.subtitles_track_auto"
                        : "downloadForm.subtitles_track_author",
                    )}
                  </span>
                </label>
              );
            })}
          </div>
        </PopoverContent>
      </Popover>

      {value.include_auto_generated && (
        <p className="text-xs leading-relaxed text-muted-foreground">
          {t("downloadForm.subtitles_auto_note")}
        </p>
      )}

      {anyPicked && (
        <div className="flex flex-col gap-2 pt-1">
          <Label className="text-xs font-semibold tracking-tight text-foreground/80">
            {t("downloadForm.subtitles_delivery_label")}
          </Label>
          <RadioGroup
            value={delivery}
            onValueChange={(next) => onChange({ ...value, delivery: next as SubtitleDelivery })}
            className="gap-1.5"
          >
            {DELIVERIES.map((option) => {
              // FR-220/SC-209: "embed" is disabled with the reason attached,
              // not hidden — a control that vanishes teaches the user nothing.
              const blocked = option === "embedded" && !embedSupported;
              return (
                <label
                  key={option}
                  htmlFor={`subtitle-delivery-${option}`}
                  className={`flex items-center gap-3 rounded-md border border-border/80 bg-card px-3 py-2 text-xs transition-all ${
                    blocked ? "cursor-not-allowed opacity-60" : "cursor-pointer hover:border-primary/40"
                  } ${delivery === option && !blocked ? "border-primary bg-primary/5" : ""}`}
                >
                  <RadioGroupItem
                    value={option}
                    id={`subtitle-delivery-${option}`}
                    disabled={blocked}
                  />
                  <span className="w-28 shrink-0 font-semibold">{t(DELIVERY_LABEL_KEY[option])}</span>
                  <span className="flex-1 text-muted-foreground">
                    {blocked && embedBlockedReasonKey
                      ? t(embedBlockedReasonKey)
                      : t(DELIVERY_HINT_KEY[option])}
                  </span>
                </label>
              );
            })}
          </RadioGroup>
        </div>
      )}
    </div>
  );
}
