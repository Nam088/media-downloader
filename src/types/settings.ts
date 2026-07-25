export type ThemePreference = "system" | "light" | "dark";
export type LanguagePreference = "system" | "en" | "vi";

export interface AppSettings {
  theme: ThemePreference;
  language: LanguagePreference;
  default_output_directory: string;
  show_logs_tab: boolean;
}
