import type { MusicQualityTier } from "@/types/download";

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
  /**
   * Provider priority for SpotiFLAC music downloads, as a CSV of provider
   * ids — default `"tidal,qobuz,deezer,amazon"`. The backend only accepts a
   * subset/permutation of those four, never an empty string.
   */
  spotiflac_service_order: string;
  /** Default quality tier a new music job starts on; default `"flac16"`. */
  spotiflac_quality: MusicQualityTier;
  /** Let the worker fall back to JS-extension providers when the four native
   * ones fail (needs Node on PATH); default true. */
  spotiflac_extensions_fallback: boolean;
  /** Telegram bot token for Cloudflare-challenge notifications. Stored as
   * plaintext in the settings table — empty means "not configured". */
  tg_bot_token: string;
  /** Telegram chat id the bot writes to; digits only, or empty. */
  tg_chat_id: string;
}
