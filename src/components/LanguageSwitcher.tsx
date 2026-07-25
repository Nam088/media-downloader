import { useTranslation } from "react-i18next";
import { Languages } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useAppSettings } from "@/hooks/use-app-settings";
import { SUPPORTED_LANGUAGES } from "@/lib/i18n";
import type { LanguagePreference } from "@/types/settings";

/** FR-017/SC-007: switching language is a single action and applies
 * instantly across every screen (i18next re-renders all `t()` calls), and
 * the choice is persisted to `AppSettings.language` for the next launch. */
export function LanguageSwitcher() {
  const { t, i18n } = useTranslation();
  const { updateSettings } = useAppSettings();

  async function choose(language: LanguagePreference) {
    if (language === "system") {
      i18n.changeLanguage(undefined);
    } else {
      i18n.changeLanguage(language);
    }
    await updateSettings({ language });
  }

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={t("language.toggle_label")}>
          <Languages className="h-4 w-4" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end">
        {SUPPORTED_LANGUAGES.map((lang) => (
          <DropdownMenuItem key={lang} onClick={() => choose(lang)}>
            {t(`language.${lang}`)}
          </DropdownMenuItem>
        ))}
        <DropdownMenuItem onClick={() => choose("system")}>
          {t("language.system")}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
