import { useTranslation } from "react-i18next";
import { Languages, Check } from "lucide-react";
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
  const { settings, updateSettings } = useAppSettings();

  async function choose(language: LanguagePreference) {
    if (language === "system") {
      i18n.changeLanguage(undefined);
    } else {
      i18n.changeLanguage(language);
    }
    await updateSettings({ language });
  }

  const currentLang = settings?.language ?? "system";

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button variant="ghost" size="icon" aria-label={t("language.toggle_label")} className="h-9 w-9 rounded-lg hover:bg-accent/60">
          <Languages className="h-4 w-4 text-foreground/80 transition-transform duration-200 hover:scale-110" />
        </Button>
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" className="w-36">
        {SUPPORTED_LANGUAGES.map((lang) => {
          const isSelected = currentLang === lang;
          return (
            <DropdownMenuItem 
              key={lang} 
              onClick={() => choose(lang)}
              className={isSelected ? "bg-primary/10 font-medium text-primary" : ""}
            >
              <span className="flex-1">{t(`language.${lang}`)}</span>
              {isSelected && <Check className="ml-auto h-3.5 w-3.5 text-primary" />}
            </DropdownMenuItem>
          );
        })}
        <DropdownMenuItem 
          onClick={() => choose("system")}
          className={currentLang === "system" ? "bg-primary/10 font-medium text-primary" : ""}
        >
          <span className="flex-1">{t("language.system")}</span>
          {currentLang === "system" && <Check className="ml-auto h-3.5 w-3.5 text-primary" />}
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
