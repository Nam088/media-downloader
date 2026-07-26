import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronUp, TriangleAlert } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { useAppSettings } from "@/hooks/use-app-settings";
import { formatPlatformLabel } from "@/lib/format";
import { MUSIC_QUALITY_TIERS, type AppError, type MusicQualityTier } from "@/types/download";
import type { AppSettings } from "@/types/settings";

/** Same range the `update_settings` command clamps to. Enforced here as well
 * so the user sees straight away that 99 is not what got saved — the Rust
 * clamp stays, because the command is directly invokable and 0 would leave
 * the dispatcher never starting a job. */
const MIN_CONCURRENT_DOWNLOADS = 1;
const MAX_CONCURRENT_DOWNLOADS = 8;

/** The four providers `update_settings` accepts, in their default order. Used
 * only to backfill a CSV that somehow arrived short, so the list on screen is
 * never missing a provider the user has no way to add back. */
const SPOTIFLAC_PROVIDERS = ["tidal", "qobuz", "deezer", "amazon"] as const;

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

  // The two Telegram boxes follow the same draft-then-commit-on-blur shape, so
  // half a pasted token is never persisted keystroke by keystroke.
  const [botTokenDraft, setBotTokenDraft] = useState<string | null>(null);
  const [chatIdDraft, setChatIdDraft] = useState<string | null>(null);

  const concurrencyValue = concurrencyDraft ?? savedConcurrency?.toString() ?? "";
  const rateLimitValue = rateLimitDraft ?? savedRateLimit?.toString() ?? "";
  const botTokenValue = botTokenDraft ?? settings?.tg_bot_token ?? "";
  const chatIdValue = chatIdDraft ?? settings?.tg_chat_id ?? "";

  // The CSV is owned by the backend (validated as a subset/permutation of the
  // four providers); the page only reorders whatever it was given. Providers
  // the CSV happens to omit are appended in default order, so a row can never
  // vanish from a list that has no way of adding one back.
  const serviceOrder = settings
    ? (() => {
        const listed = settings.spotiflac_service_order
          .split(",")
          .map((provider) => provider.trim())
          .filter(
            (provider): provider is (typeof SPOTIFLAC_PROVIDERS)[number] =>
              (SPOTIFLAC_PROVIDERS as readonly string[]).includes(provider),
          );
        return [...listed, ...SPOTIFLAC_PROVIDERS.filter((known) => !listed.includes(known))];
      })()
    : [];

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

  async function handleMoveProvider(index: number, direction: -1 | 1) {
    const target = index + direction;
    if (target < 0 || target >= serviceOrder.length) return;
    const next = [...serviceOrder];
    [next[index], next[target]] = [next[target], next[index]];
    await persist({ spotiflac_service_order: next.join(",") });
  }

  async function handleChangeQuality(tier: MusicQualityTier) {
    if (tier !== settings?.spotiflac_quality) {
      await persist({ spotiflac_quality: tier });
    }
  }

  async function handleToggleExtensionsFallback(checked: boolean) {
    await persist({ spotiflac_extensions_fallback: checked });
  }

  async function handleCommitBotToken() {
    if (botTokenDraft === null || settings === null) return;
    const trimmed = botTokenDraft.trim();
    if (trimmed !== settings.tg_bot_token) {
      await persist({ tg_bot_token: trimmed });
    }
    setBotTokenDraft(null);
  }

  async function handleCommitChatId() {
    if (chatIdDraft === null || settings === null) return;
    // The backend rejects anything but digits-or-empty; committing only a
    // valid value means the box snaps back instead of surfacing a Rust error
    // for a stray letter.
    const trimmed = chatIdDraft.trim();
    if (/^\d*$/.test(trimmed) && trimmed !== settings.tg_chat_id) {
      await persist({ tg_chat_id: trimmed });
    }
    setChatIdDraft(null);
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
              {t("settings.max_concurrent_label")}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.max_concurrent_hint", {
                min: MIN_CONCURRENT_DOWNLOADS,
                max: MAX_CONCURRENT_DOWNLOADS,
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
              {t("settings.rate_limit_label")}
            </Label>
            {/* Required, not decoration: the cap is passed to each download
                process separately, so N downloads can use up to N times it. */}
            <span className="text-xs text-muted-foreground">
              {t("settings.rate_limit_hint", {
                concurrency: savedConcurrency ?? MIN_CONCURRENT_DOWNLOADS,
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
              {t("settings.background_label")}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.background_hint")}
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

      <h3 className="text-sm font-bold tracking-tight">{t("settings.spotiflac.section")}</h3>

      <div className="flex flex-col gap-4 rounded-lg border border-border/80 bg-card p-5 shadow-2xs">
        <div className="flex flex-col gap-3 py-1">
          <div className="flex flex-col gap-0.5">
            <Label className="text-sm font-medium">{t("settings.spotiflac.serviceOrder")}</Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.spotiflac.serviceOrderHint")}
            </span>
          </div>
          <ol className="flex flex-col gap-1.5">
            {serviceOrder.map((provider, index) => (
              <li
                key={provider}
                className="flex items-center gap-3 rounded-md border border-border/80 bg-muted/30 px-3 py-1.5"
              >
                <span className="w-5 shrink-0 text-center font-mono text-xs text-muted-foreground">
                  {index + 1}
                </span>
                <span className="min-w-0 flex-1 truncate text-sm font-medium">
                  {formatPlatformLabel(provider)}
                </span>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 rounded-md"
                  disabled={index === 0}
                  aria-label={t("settings.spotiflac.moveUp", {
                    provider: formatPlatformLabel(provider),
                  })}
                  onClick={() => void handleMoveProvider(index, -1)}
                >
                  <ChevronUp className="h-4 w-4" />
                </Button>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 rounded-md"
                  disabled={index === serviceOrder.length - 1}
                  aria-label={t("settings.spotiflac.moveDown", {
                    provider: formatPlatformLabel(provider),
                  })}
                  onClick={() => void handleMoveProvider(index, 1)}
                >
                  <ChevronDown className="h-4 w-4" />
                </Button>
              </li>
            ))}
          </ol>
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="spotiflac-quality" className="text-sm font-medium">
              {t("settings.spotiflac.quality")}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.spotiflac.qualityHint")}
            </span>
          </div>
          <Select
            value={settings?.spotiflac_quality}
            onValueChange={(tier) => void handleChangeQuality(tier as MusicQualityTier)}
          >
            <SelectTrigger id="spotiflac-quality" className="w-56 shrink-0">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {MUSIC_QUALITY_TIERS.map((tier) => (
                <SelectItem key={tier} value={tier}>
                  {t(`downloadForm.musicTier.${tier}`)}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="spotiflac-extensions-fallback" className="text-sm font-medium">
              {t("settings.spotiflac.extensionsFallback")}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.spotiflac.extensionsFallbackHint")}
            </span>
          </div>
          <Switch
            id="spotiflac-extensions-fallback"
            checked={settings?.spotiflac_extensions_fallback ?? false}
            onCheckedChange={(checked) => void handleToggleExtensionsFallback(checked)}
          />
        </div>

        <div className="h-px bg-border/60" />

        {/* Not decoration: the token grants full control of the bot, and the
            settings table is a plain SQLite file anyone on the machine can
            read — the user must know that before pasting one in. */}
        <p className="flex items-start gap-1.5 rounded-md bg-amber-500/10 px-3 py-2 text-xs leading-relaxed text-amber-700 dark:text-amber-400">
          <TriangleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span>{t("settings.spotiflac.plaintextWarning")}</span>
        </p>

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="tg-bot-token" className="text-sm font-medium">
              {t("settings.spotiflac.tgBotToken")}
            </Label>
            <span className="text-xs text-muted-foreground">
              {t("settings.spotiflac.telegramHint")}
            </span>
          </div>
          <Input
            id="tg-bot-token"
            type="password"
            autoComplete="off"
            placeholder={t("settings.spotiflac.tgBotTokenPlaceholder")}
            value={botTokenValue}
            onChange={(event) => setBotTokenDraft(event.target.value)}
            onBlur={() => void handleCommitBotToken()}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            className="w-64 shrink-0"
          />
        </div>

        <div className="h-px bg-border/60" />

        <div className="flex items-center justify-between gap-6 py-1">
          <div className="flex min-w-0 flex-col gap-0.5">
            <Label htmlFor="tg-chat-id" className="text-sm font-medium">
              {t("settings.spotiflac.tgChatId")}
            </Label>
          </div>
          <Input
            id="tg-chat-id"
            inputMode="numeric"
            autoComplete="off"
            placeholder={t("settings.spotiflac.tgChatIdPlaceholder")}
            value={chatIdValue}
            onChange={(event) => setChatIdDraft(event.target.value)}
            onBlur={() => void handleCommitChatId()}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
            className="w-40 shrink-0"
          />
        </div>
      </div>
    </div>
  );
}
