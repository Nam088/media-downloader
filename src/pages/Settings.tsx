import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { useAppSettings } from "@/hooks/use-app-settings";
import type { AppError } from "@/types/download";
import type { AppSettings } from "@/types/settings";

/** Same range the `update_settings` command clamps to. Enforced here as well
 * so the user sees straight away that 99 is not what got saved — the Rust
 * clamp stays, because the command is directly invokable and 0 would leave
 * the dispatcher never starting a job. */
const MIN_CONCURRENT_DOWNLOADS = 1;
const MAX_CONCURRENT_DOWNLOADS = 8;

/**
 * Reads a whole-number field.
 *
 * Returns `null` for an empty or non-numeric box rather than falling back to
 * `Number("") === 0`: for the concurrency field 0 is an invalid value, and for
 * the rate limit 0 means "unlimited" — one stray keystroke must not silently
 * mean either of those. A `null` result is treated as "nothing was typed" and
 * the field is put back to the stored value.
 */
function parseWholeNumber(raw: string): number | null {
  const trimmed = raw.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) ? value : null;
}

export function Settings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useAppSettings();

  const savedConcurrency = settings?.max_concurrent_downloads;
  const savedRateLimit = settings?.rate_limit_kbps;

  // The two number boxes keep the in-progress text as a string, so a half-typed
  // or momentarily empty value stays on screen instead of being coerced into a
  // number on every keystroke; they are committed on blur.
  //
  // `null` means "not being edited": the box then simply shows the stored
  // value. Clearing the draft after a commit is therefore all it takes to snap
  // the box back to what was actually persisted — the clamped 8 after typing
  // 99, or the untouched old value after an invalid entry or a failed save —
  // with no effect mirroring settings into state.
  const [concurrencyDraft, setConcurrencyDraft] = useState<string | null>(null);
  const [rateLimitDraft, setRateLimitDraft] = useState<string | null>(null);

  const concurrencyValue = concurrencyDraft ?? savedConcurrency?.toString() ?? "";
  const rateLimitValue = rateLimitDraft ?? savedRateLimit?.toString() ?? "";

  async function persist(patch: Partial<AppSettings>): Promise<boolean> {
    try {
      await updateSettings(patch);
      return true;
    } catch (err) {
      const appError = err as AppError;
      toast.error(appError.message ?? t("errors.INTERNAL"));
      return false;
    }
  }

  async function handleChooseDefaultDirectory() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") {
      await persist({ default_output_directory: selected });
    }
  }

  async function handleToggleShowLogsTab(checked: boolean) {
    await persist({ show_logs_tab: checked });
  }

  async function handleCommitConcurrency() {
    if (concurrencyDraft === null || savedConcurrency === undefined) return;
    const parsed = parseWholeNumber(concurrencyDraft);
    if (parsed !== null) {
      const clamped = Math.min(
        Math.max(parsed, MIN_CONCURRENT_DOWNLOADS),
        MAX_CONCURRENT_DOWNLOADS,
      );
      if (clamped !== savedConcurrency) {
        await persist({ max_concurrent_downloads: clamped });
      }
    }
    // Either way the box now shows the stored value: the new one on success,
    // the untouched old one after an empty/invalid entry or a failed write.
    setConcurrencyDraft(null);
  }

  async function handleCommitRateLimit() {
    if (rateLimitDraft === null || savedRateLimit === undefined) return;
    const parsed = parseWholeNumber(rateLimitDraft);
    if (parsed !== null && parsed !== savedRateLimit) {
      await persist({ rate_limit_kbps: parsed });
    }
    setRateLimitDraft(null);
  }

  async function handleToggleRunInBackground(checked: boolean) {
    await persist({ run_in_background: checked });
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <h2 className="text-lg font-bold tracking-tight">{t("nav.settings")}</h2>

      <div className="flex flex-col gap-4 rounded-lg border border-border/80 bg-card p-5 shadow-2xs">
        <div className="flex items-center justify-between py-1">
          <div className="flex flex-col gap-0.5">
            <Label className="text-sm font-medium">{t("settings.theme_label")}</Label>
          </div>
          <ThemeToggle />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between py-1">
          <div className="flex flex-col gap-0.5">
            <Label className="text-sm font-medium">{t("settings.language_label")}</Label>
          </div>
          <LanguageSwitcher />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex flex-col gap-3 py-1">
          <Label className="text-sm font-medium">
            {t("settings.default_output_directory_label")}
          </Label>
          <div className="flex items-center gap-3 rounded-md border border-border/80 bg-muted/30 p-2 pl-3">
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground/80">
              {settings?.default_output_directory || t("downloadForm.no_directory_chosen")}
            </span>
            <Button
              variant="outline"
              size="sm"
              onClick={handleChooseDefaultDirectory}
              className="rounded-md text-xs"
            >
              {t("downloadForm.choose_directory_button")}
            </Button>
          </div>
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="max-concurrent-downloads" className="text-sm font-medium">
              {t("settings.max_concurrent_label", { defaultValue: "Concurrent downloads" })}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.max_concurrent_hint", {
                min: MIN_CONCURRENT_DOWNLOADS,
                max: MAX_CONCURRENT_DOWNLOADS,
                defaultValue:
                  "How many downloads run at the same time ({{min}}–{{max}}). More is not always faster — sources rate-limit too.",
              })}
            </span>
          </div>
          <Input
            id="max-concurrent-downloads"
            type="number"
            inputMode="numeric"
            min={MIN_CONCURRENT_DOWNLOADS}
            max={MAX_CONCURRENT_DOWNLOADS}
            value={concurrencyValue}
            onChange={(event) => setConcurrencyDraft(event.target.value)}
            onBlur={() => void handleCommitConcurrency()}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            className="w-20 shrink-0 text-center"
          />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="rate-limit-kbps" className="text-sm font-medium">
              {t("settings.rate_limit_label", { defaultValue: "Speed limit (KB/s)" })}
            </Label>
            {/* Required, not decoration: the cap is passed to each download
                process separately, so N downloads can use up to N times it. */}
            <span className="text-xs text-muted-foreground">
              {t("settings.rate_limit_hint", {
                concurrency: savedConcurrency ?? MIN_CONCURRENT_DOWNLOADS,
                defaultValue:
                  "0 means unlimited. The limit applies per download, not to the app total — with {{concurrency}} running at once the combined speed can reach {{concurrency}}× this number.",
              })}
            </span>
          </div>
          <Input
            id="rate-limit-kbps"
            type="number"
            inputMode="numeric"
            min={0}
            value={rateLimitValue}
            onChange={(event) => setRateLimitDraft(event.target.value)}
            onBlur={() => void handleCommitRateLimit()}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            className="w-24 shrink-0 text-center"
          />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="run-in-background" className="text-sm font-medium">
              {t("settings.background_label", { defaultValue: "Keep running in the background" })}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.background_hint", {
                defaultValue:
                  "Closing the window minimises to the system tray and downloads keep going.",
              })}
            </span>
          </div>
          <Switch
            id="run-in-background"
            checked={settings?.run_in_background ?? false}
            onCheckedChange={(checked) => void handleToggleRunInBackground(checked)}
          />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between py-1">
          <div className="flex flex-col gap-0.5">
            <Label className="text-sm font-medium">{t("settings.show_logs_tab_label")}</Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.show_logs_tab_hint")}
            </span>
          </div>
          <Switch
            checked={settings?.show_logs_tab ?? false}
            onCheckedChange={handleToggleShowLogsTab}
          />
        </div>
      </div>
    </div>
  );
}
