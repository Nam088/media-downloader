import { useTranslation } from "react-i18next";
import { useTheme } from "next-themes";
import { Moon, Sun, Monitor } from "lucide-react";
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
        <Button variant="ghost" size="icon" aria-label={t("theme.toggle_label")}>
          <Icon className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        <DropdownMenuItem onClick={() => choose("light")}>
          <Sun className="mr-2 h-4 w-4" />
          {t("theme.light")}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => choose("dark")}>
          <Moon className="mr-2 h-4 w-4" />
          {t("theme.dark")}
        </DropdownMenuItem>
        <DropdownMenuItem onClick={() => choose("system")}>
          <Monitor className="mr-2 h-4 w-4" />
          {t("theme.system")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
