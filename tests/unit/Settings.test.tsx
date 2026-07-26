import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { ThemeProvider } from "@/components/theme-provider";
import { Settings } from "@/pages/Settings";
import { useSettingsStore } from "@/stores/settings-store";
import type { AppSettings } from "@/types/settings";

// `satisfies` is load-bearing: `invoke` resolves to `unknown`, so without it
// this fixture would silently drift out of shape as AppSettings gains fields.
const SAVED_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "/out",
  show_logs_tab: false,
  max_concurrent_downloads: 3,
  rate_limit_kbps: 0,
  max_retry_attempts: 3,
  run_in_background: false,
  spotiflac_service_order: "tidal,qobuz,deezer,amazon",
  spotiflac_quality: "flac16",
  spotiflac_extensions_fallback: true,
  tg_bot_token: "",
  tg_chat_id: "",
} satisfies AppSettings;

/**
 * Every `update_settings` patch the page sent, in call order.
 *
 * Reading the recorded calls rather than asserting on a single
 * `toHaveBeenCalledWith` lets a test also state that *nothing* was written —
 * which is the whole point of the "empty box" case below.
 */
function patchesSent(): Array<Partial<AppSettings>> {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "update_settings")
    .map(([, args]) => (args as { patch: Partial<AppSettings> }).patch);
}

function renderSettings() {
  return render(
    <ThemeProvider>
      <Settings />
    </ThemeProvider>,
  );
}

/** Waits for the async `get_settings` fetch to land in the shared store, so a
 * test never types into a field that is about to be overwritten by the load. */
async function loadedConcurrencyBox(): Promise<HTMLInputElement> {
  const box = await screen.findByLabelText<HTMLInputElement>(/concurrent downloads/i);
  await screen.findByDisplayValue(String(SAVED_SETTINGS.max_concurrent_downloads));
  return box;
}

describe("Settings page — concurrency, rate limit, background mode (FR-112, FR-113, FR-126, FR-127)", () => {
  beforeEach(() => {
    // The settings store is a module-level singleton, so a previous test's
    // loaded state (and its cached fetch promise) would otherwise leak in.
    useSettingsStore.setState({ settings: null, loading: true, fetchPromise: null });

    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((command: string, args?: unknown) => {
      if (command === "get_settings") {
        return Promise.resolve(SAVED_SETTINGS);
      }
      if (command === "update_settings") {
        // Mirror the backend: merge the patch and echo the stored result back.
        // Deliberately *without* the Rust clamp, so a test asserting the UI
        // clamps cannot pass on the strength of the mock doing it.
        const { patch } = args as { patch: Partial<AppSettings> };
        return Promise.resolve({ ...SAVED_SETTINGS, ...patch });
      }
      return Promise.resolve(undefined);
    });
  });

  it("persists a new concurrency value", async () => {
    const user = userEvent.setup();
    renderSettings();

    const box = await loadedConcurrencyBox();
    await user.clear(box);
    await user.type(box, "6");
    await user.tab();

    expect(patchesSent()).toEqual([{ max_concurrent_downloads: 6 }]);
  });

  it("clamps concurrency into 1–8 before sending it, and shows the clamped value", async () => {
    const user = userEvent.setup();
    renderSettings();

    const box = await loadedConcurrencyBox();
    await user.clear(box);
    await user.type(box, "99");
    await user.tab();

    // The Rust command clamps too, but only the UI can give the user immediate
    // feedback that 99 is not what was saved.
    expect(patchesSent()).toEqual([{ max_concurrent_downloads: 8 }]);
    expect(box).toHaveValue(8);
  });

  it("keeps the saved concurrency when the box is left empty instead of writing 0", async () => {
    const user = userEvent.setup();
    renderSettings();

    const box = await loadedConcurrencyBox();
    await user.clear(box);
    await user.tab();

    // `Number("")` is 0, which would stop the dispatcher from ever starting a
    // job. An empty box means "no value typed", not "zero".
    expect(patchesSent()).toEqual([]);
    expect(box).toHaveValue(SAVED_SETTINGS.max_concurrent_downloads);
  });

  it("persists a speed limit", async () => {
    const user = userEvent.setup();
    renderSettings();

    await loadedConcurrencyBox();
    const box = await screen.findByLabelText<HTMLInputElement>(/speed limit/i);
    await user.clear(box);
    await user.type(box, "512");
    await user.tab();

    expect(patchesSent()).toEqual([{ rate_limit_kbps: 512 }]);
  });

  it("explains that the speed limit applies per download, not to the app total", async () => {
    renderSettings();

    // Not decoration: a user who caps at 500 KB/s to protect a shared line and
    // then measures 1.5 MB/s across 3 downloads will report the cap as broken.
    // "applies per download" rather than "per download": the SpotiFLAC quality
    // hint also says "per download", and findByText refuses two matches.
    const hint = await screen.findByText(/applies per download/i);
    expect(hint).toHaveTextContent(/not.*total/i);
  });

  it("warns that raising the concurrency is not reliably faster", async () => {
    renderSettings();

    expect(await screen.findByText(/not always faster/i)).toBeInTheDocument();
  });

  it("toggles background mode", async () => {
    const user = userEvent.setup();
    renderSettings();

    await loadedConcurrencyBox();
    await user.click(screen.getByRole("switch", { name: /keep running in the background/i }));

    expect(patchesSent()).toEqual([{ run_in_background: true }]);
  });
});
