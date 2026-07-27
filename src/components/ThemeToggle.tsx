import { useTranslation } from "react-i18next";
import { useTheme } from "next-themes";
import { Moon, Sun, Monitor, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAppSettings } from "@/hooks/use-app-settings";
import type { ThemePreference } from "@/types/settings";

const ICONS: Record<ThemePreference, typeof Sun> = {
  light: Sun,
  dark: Moon,
  system: Monitor,
};

/** FR-016/SC-006: switching theme is a single action and applies instantly —
 * `next-themes` flips the `dark` class immediately, and the choice is also
 * persisted to `AppSettings.theme` so it survives an app restart. */
export function ThemeToggle() {
  const { t } = useTranslation();
  const { theme, setTheme } = useTheme();
  const { updateSettings } = useAppSettings();

  async function choose(nextTheme: ThemePreference) {
    setTheme(nextTheme);
    await updateSettings({ theme: nextTheme });
  }

  const Icon = ICONS[(theme as ThemePreference) ?? "system"] ?? Monitor;

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={t("theme.toggle_label")} className="h-9 w-9 rounded-lg hover:bg-accent/60">
          <Icon className="h-4 w-4 text-foreground/80 transition-transform duration-200 hover:scale-110" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        <DropdownMenuItem 
          onClick={() => choose("light")}
          className={theme === "light" ? "bg-primary/10 font-medium text-primary" : ""}
        >
          <Sun className="mr-2 h-4 w-4" />
          <span className="flex-1">{t("theme.light")}</span>
          {theme === "light" && <Check className="ml-auto h-3.5 w-3.5 text-primary" />}
        </DropdownMenuItem>
        <DropdownMenuItem 
          onClick={() => choose("dark")}
          className={theme === "dark" ? "bg-primary/10 font-medium text-primary" : ""}
        >
          <Moon className="mr-2 h-4 w-4" />
          <span className="flex-1">{t("theme.dark")}</span>
          {theme === "dark" && <Check className="ml-auto h-3.5 w-3.5 text-primary" />}
        </DropdownMenuItem>
        <DropdownMenuItem 
          onClick={() => choose("system")}
          className={theme === "system" ? "bg-primary/10 font-medium text-primary" : ""}
        >
          <Monitor className="mr-2 h-4 w-4" />
          <span className="flex-1">{t("theme.system")}</span>
          {theme === "system" && <Check className="ml-auto h-3.5 w-3.5 text-primary" />}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
