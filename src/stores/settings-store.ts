import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type { AppSettings } from "@/types/settings";

interface SettingsState {
  settings: AppSettings | null;
  loading: boolean;
  fetchPromise: Promise<void> | null;
  ensureLoaded: () => Promise<void>;
  updateSettings: (patch: Partial<AppSettings>) => Promise<AppSettings>;
}

/**
 * Single shared source of truth for `AppSettings` (theme/language/default
 * output directory). Every component that needs settings (App.tsx,
 * ThemeToggle, LanguageSwitcher, DownloadForm, Settings page) reads from
 * this same store via `useAppSettings()` instead of each keeping its own
 * local `useState` copy — with independent copies, one component's
 * `updateSettings` call (e.g. changing language) never reached the others,
 * so a stale copy elsewhere could still be showing/enforcing the old value.
 */
export const useSettingsStore = create<SettingsState>((set, get) => ({
  settings: null,
  loading: true,
  fetchPromise: null,
  ensureLoaded: () => {
    const existing = get().fetchPromise;
    if (existing) return existing;
    const promise = invoke<AppSettings>("get_settings")
      .then((result) => set({ settings: result, loading: false }))
      .catch(() => set({ loading: false }));
    set({ fetchPromise: promise });
    return promise;
  },
  updateSettings: async (patch) => {
    const result = await invoke<AppSettings>("update_settings", { patch });
    set({ settings: result });
    return result;
  },
}));
