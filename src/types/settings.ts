export type ThemePreference = "system" | "light" | "dark";
export type LanguagePreference = "system" | "en" | "vi";

export interface AppSettings {
  theme: ThemePreference;
  language: LanguagePreference;
  default_output_directory: string;
  show_logs_tab: boolean;
  /** How many downloads run at once. Clamped to 1..=8 by the backend. */
  max_concurrent_downloads: number;
  /**
   * Speed cap in KB/s applied to *each* download process, not the app as a
   * whole — with N running, combined throughput can reach N times this.
   * 0 means unlimited.
   */
  rate_limit_kbps: number;
  /** Auto-retry attempts for transient failures; 0 disables auto-retry. */
  max_retry_attempts: number;
  /** Closing the window minimises to the tray instead of quitting. */
  run_in_background: boolean;
}
