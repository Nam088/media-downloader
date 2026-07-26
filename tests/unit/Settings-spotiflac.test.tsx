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

/** Every `update_settings` patch the page sent, in call order. */
function patchesSent(): Array<Partial<AppSettings>> {
  return vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === "update_settings")
    .map(([, args]) => (args as { patch: Partial<AppSettings> }).patch);
}

/** Renders the page and waits for the async `get_settings` fetch to land, so
 * a test never interacts with a control that is about to be re-rendered. */
async function renderLoadedSettings() {
  render(
    <ThemeProvider>
      <Settings />
    </ThemeProvider>,
  );
  await screen.findByText("TIDAL");
}

describe("Settings page — SpotiFLAC section (T024, T037)", () => {
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
        const { patch } = args as { patch: Partial<AppSettings> };
        return Promise.resolve({ ...SAVED_SETTINGS, ...patch });
      }
      return Promise.resolve(undefined);
    });
  });

  it("lists the providers in CSV order under their brand names", async () => {
    await renderLoadedSettings();

    const items = screen.getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      expect.stringContaining("TIDAL"),
      expect.stringContaining("Qobuz"),
      expect.stringContaining("Deezer"),
      expect.stringContaining("Amazon Music"),
    ]);
  });

  it("backfills a provider the stored CSV left out instead of dropping the row", async () => {
    // The backend always stores all four, so this only ever fires on a corrupt
    // or hand-edited row — but a missing provider would be a dead end, since
    // reordering is the only way the page can put one back.
    vi.mocked(invoke).mockImplementation((command: string) =>
      command === "get_settings"
        ? Promise.resolve({ ...SAVED_SETTINGS, spotiflac_service_order: "qobuz,amazon" })
        : Promise.resolve(undefined),
    );
    await renderLoadedSettings();

    const items = screen.getAllByRole("listitem");
    expect(items.map((item) => item.textContent)).toEqual([
      expect.stringContaining("Qobuz"),
      expect.stringContaining("Amazon Music"),
      expect.stringContaining("TIDAL"),
      expect.stringContaining("Deezer"),
    ]);
  });

  it("persists the reordered CSV when a provider is moved down", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    await user.click(screen.getByRole("button", { name: "Move TIDAL down" }));

    expect(patchesSent()).toEqual([{ spotiflac_service_order: "qobuz,tidal,deezer,amazon" }]);
  });

  it("persists the reordered CSV when a provider is moved up", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    await user.click(screen.getByRole("button", { name: "Move Deezer up" }));

    expect(patchesSent()).toEqual([{ spotiflac_service_order: "tidal,deezer,qobuz,amazon" }]);
  });

  it("disables moving the first provider up and the last one down", async () => {
    await renderLoadedSettings();

    expect(screen.getByRole("button", { name: "Move TIDAL up" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Amazon Music down" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move Qobuz up" })).toBeEnabled();
  });

  it("persists a new default quality tier", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    await user.click(screen.getByRole("combobox", { name: /default music quality/i }));
    await user.click(await screen.findByRole("option", { name: /mp3 320/i }));

    expect(patchesSent()).toEqual([{ spotiflac_quality: "mp3_320" }]);
  });

  it("toggles the JS-extensions fallback", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    await user.click(screen.getByRole("switch", { name: /extension providers/i }));

    // Fixture starts with the fallback on, so the click turns it off.
    expect(patchesSent()).toEqual([{ spotiflac_extensions_fallback: false }]);
  });

  it("masks the bot token and commits it on blur, trimmed", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    const box = screen.getByLabelText<HTMLInputElement>(/telegram bot token/i);
    expect(box).toHaveAttribute("type", "password");

    await user.type(box, "  123:abc  ");
    // Nothing may be written while the user is still typing.
    expect(patchesSent()).toEqual([]);

    await user.tab();
    expect(patchesSent()).toEqual([{ tg_bot_token: "123:abc" }]);
  });

  it("commits a numeric chat id on blur", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    const box = screen.getByLabelText<HTMLInputElement>(/telegram chat id/i);
    await user.type(box, "123456789");
    await user.tab();

    expect(patchesSent()).toEqual([{ tg_chat_id: "123456789" }]);
  });

  it("drops a non-numeric chat id instead of persisting it", async () => {
    const user = userEvent.setup();
    await renderLoadedSettings();

    const box = screen.getByLabelText<HTMLInputElement>(/telegram chat id/i);
    await user.type(box, "not-a-number");
    await user.tab();

    // The backend would reject it anyway; the box snaps back to the stored
    // (empty) value rather than surfacing a Rust validation error.
    expect(patchesSent()).toEqual([]);
    expect(box).toHaveValue(SAVED_SETTINGS.tg_chat_id);
  });

  it("warns that the token is stored as plaintext", async () => {
    await renderLoadedSettings();

    expect(screen.getByText(/plain text/i)).toBeInTheDocument();
  });
});
