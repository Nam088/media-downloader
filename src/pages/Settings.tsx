import { useTranslation } from "react-i18next";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { useAppSettings } from "@/hooks/use-app-settings";

export function Settings() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useAppSettings();

  async function handleChooseDefaultDirectory() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") {
      await updateSettings({ default_output_directory: selected });
    }
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
          <Label className="text-sm font-medium">{t("settings.default_output_directory_label")}</Label>
          <div className="flex items-center gap-3 rounded-md border border-border/80 bg-muted/30 p-2 pl-3">
            <span className="min-w-0 flex-1 truncate font-mono text-xs text-foreground/80">
              {settings?.default_output_directory || t("downloadForm.no_directory_chosen")}
            </span>
            <Button variant="outline" size="sm" onClick={handleChooseDefaultDirectory} className="rounded-md text-xs">
              {t("downloadForm.choose_directory_button")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
