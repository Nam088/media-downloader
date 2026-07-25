import { useEffect } from "react";
import { useSettingsStore } from "@/stores/settings-store";

/** Thin wrapper around the shared `useSettingsStore` (see that file for why
 * settings live in one store instead of per-component state). `ensureLoaded`
 * is idempotent — the first caller triggers the `get_settings` fetch, every
 * later caller (in any component) just subscribes to the same result. */
export function useAppSettings() {
  const settings = useSettingsStore((state) => state.settings);
  const loading = useSettingsStore((state) => state.loading);
  const updateSettings = useSettingsStore((state) => state.updateSettings);

  useEffect(() => {
    useSettingsStore.getState().ensureLoaded();
  }, []);

  return { settings, loading, updateSettings };
}
