import { useState } from "react";
import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Sliders, Download, Monitor, Layers, Palette, Gauge, FolderInput, User, ExternalLink, Info, FolderGit2 } from "lucide-react";
import { openExternalUrl } from "@/lib/open-url";
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

type SettingsTab = "all" | "general" | "downloads" | "system" | "about";

function parseWholeNumber(raw: string): number | null {
  const trimmed = raw.trim();
  if (!/^\d+$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) ? value : null;
}

export function Settings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useAppSettings();

  const [activeTab, setActiveTab] = useState<SettingsTab>("all");

  const savedConcurrency = settings?.max_concurrent_downloads;
  const savedRateLimit = settings?.rate_limit_kbps;

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

  const tabs: { id: SettingsTab; label: string; icon: typeof Sliders }[] = [
    { id: "all", label: t("settings.tab_all"), icon: Layers },
    { id: "general", label: t("settings.tab_general"), icon: Palette },
    { id: "downloads", label: t("settings.tab_downloads"), icon: Download },
    { id: "system", label: t("settings.tab_system"), icon: Monitor },
    { id: "about", label: t("settings.tab_about"), icon: Info },
  ];

  const showGeneral = activeTab === "all" || activeTab === "general";
  const showDownloads = activeTab === "all" || activeTab === "downloads";
  const showSystem = activeTab === "all" || activeTab === "system";
  const showAbout = activeTab === "all" || activeTab === "about";

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <div className="flex flex-col gap-1">
        <h2 className="text-xl font-bold tracking-tight text-foreground">{t("nav.settings")}</h2>
        <p className="text-xs text-muted-foreground">{t("settings.subtitle")}</p>
      </div>

      {/* Sub-Tab Navigation Header */}
      <div className="flex items-center gap-1.5 border-b border-border/70 pb-3">
        {tabs.map((tab) => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 rounded-lg px-3 py-2 text-xs font-semibold transition-all duration-150 cursor-pointer ${
                isActive
                  ? "bg-primary text-primary-foreground shadow-xs"
                  : "text-muted-foreground hover:bg-muted/70 hover:text-foreground"
              }`}
            >
              <Icon className="h-3.5 w-3.5" />
              <span>{tab.label}</span>
            </button>
          );
        })}
      </div>

      <div className="flex flex-col gap-6">
        {/* Section 1: General & Appearance */}
        {showGeneral && (
          <div className="flex flex-col gap-4 rounded-xl border border-border/80 bg-card p-5 shadow-2xs transition-all hover:border-primary/30">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground pb-1 border-b border-border/40">
              <Palette className="h-4 w-4 text-primary" />
              <span>{t("settings.section_general")}</span>
            </div>

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
        )}

        {/* Section 2: Downloads & Speed */}
        {showDownloads && (
          <div className="flex flex-col gap-4 rounded-xl border border-border/80 bg-card p-5 shadow-2xs transition-all hover:border-primary/30">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground pb-1 border-b border-border/40">
              <Gauge className="h-4 w-4 text-primary" />
              <span>{t("settings.section_downloads")}</span>
            </div>

            <div className="flex flex-col gap-3 py-1">
              <Label className="text-sm font-medium">
                {t("settings.default_output_directory_label")}
              </Label>
              <div className="flex items-center gap-3 rounded-lg border border-border/80 bg-muted/30 p-2 pl-3">
                <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground/80">
                  {settings?.default_output_directory || t("downloadForm.no_directory_chosen")}
                </span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleChooseDefaultDirectory}
                  className="rounded-md text-xs font-semibold"
                >
                  <FolderInput className="h-3.5 w-3.5 mr-1.5" />
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
                className="w-20 shrink-0 text-center font-bold"
              />
            </div>

            <div className="h-px bg-border/60" />

            <div className="flex items-center justify-between gap-6 py-1">
              <div className="flex min-w-0 flex-col gap-0.5">
                <Label htmlFor="rate-limit-kbps" className="text-sm font-medium">
                  {t("settings.rate_limit_label")}
                </Label>
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
                className="w-24 shrink-0 text-center font-bold"
              />
            </div>
          </div>
        )}

        {/* Section 3: System */}
        {showSystem && (
          <div className="flex flex-col gap-4 rounded-xl border border-border/80 bg-card p-5 shadow-2xs transition-all hover:border-primary/30">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground pb-1 border-b border-border/40">
              <Monitor className="h-4 w-4 text-primary" />
              <span>{t("settings.section_system")}</span>
            </div>

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
          </div>
        )}

        {/* Section 4: About & Author */}
        {showAbout && (
          <div className="flex flex-col gap-4 rounded-xl border border-border/80 bg-card p-5 shadow-2xs transition-all hover:border-primary/30">
            <div className="flex items-center gap-2 text-sm font-bold text-foreground pb-1 border-b border-border/40">
              <Info className="h-4 w-4 text-primary" />
              <span>{t("settings.section_about")}</span>
            </div>

            <div className="flex items-center justify-between py-1">
              <div className="flex items-center gap-2.5">
                <User className="h-4 w-4 text-muted-foreground" />
                <Label className="text-sm font-medium">{t("settings.author_label")}</Label>
              </div>
              <span className="rounded-md bg-primary/10 px-2.5 py-1 font-mono text-xs font-bold text-primary border border-primary/20">
                {"Nam088"}
              </span>
            </div>

            <div className="h-px bg-border/60" />

            <div className="flex items-center justify-between py-1">
              <div className="flex items-center gap-2.5">
                <FolderGit2 className="h-4 w-4 text-muted-foreground" />
                <Label className="text-sm font-medium">{t("settings.github_label")}</Label>
              </div>
              <button
                type="button"
                onClick={() => void openExternalUrl("https://github.com/Nam088")}
                className="inline-flex items-center gap-1.5 rounded-md bg-muted px-2.5 py-1 text-xs font-semibold text-foreground hover:bg-primary/10 hover:text-primary transition-colors cursor-pointer"
              >
                <span>{"github.com/Nam088"}</span>
                <ExternalLink className="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
