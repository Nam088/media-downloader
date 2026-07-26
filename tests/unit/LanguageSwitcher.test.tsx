import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import i18n from "@/lib/i18n";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import type { AppSettings } from "@/types/settings";

// `satisfies` is load-bearing: `invoke` resolves to `unknown`, so without it
// this fixture would silently drift out of shape as AppSettings gains fields.
const SAMPLE_SETTINGS = {
  theme: "system",
  language: "system",
  default_output_directory: "",
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

describe("LanguageSwitcher", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve(SAMPLE_SETTINGS);
      }
      return Promise.resolve(undefined);
    });
  });

  afterEach(async () => {
    await i18n.changeLanguage("en");
  });

  it("switches every on-screen string to the chosen language instantly (SC-007)", async () => {
    const user = userEvent.setup();
    render(<LanguageSwitcher />);

    await user.click(screen.getByRole("button", { name: /change language/i }));
    await user.click(await screen.findByText("Tiếng Việt"));

    expect(i18n.language).toBe("vi");
    expect(invoke).toHaveBeenCalledWith(
      "update_settings",
      expect.objectContaining({ patch: expect.objectContaining({ language: "vi" }) }),
    );
  });
});
