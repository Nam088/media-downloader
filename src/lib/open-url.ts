import { openUrl } from "@tauri-apps/plugin-opener";

/**
 * Safely opens an external URL using system default browser via `@tauri-apps/plugin-opener`.
 * Falls back to `window.open` if running outside desktop webview environment.
 */
export async function openExternalUrl(url: string | null | undefined): Promise<void> {
  if (!url || typeof url !== "string") return;
  const trimmed = url.trim();
  if (!trimmed) return;

  try {
    await openUrl(trimmed);
  } catch (err) {
    console.warn("Failed to open URL with plugin-opener, falling back to window.open:", err);
    window.open(trimmed, "_blank", "noopener,noreferrer");
  }
}
