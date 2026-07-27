import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { ThemeProvider } from "@/components/theme-provider";
import { ThemeToggle } from "@/components/ThemeToggle";
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
} satisfies AppSettings;

describe("ThemeToggle", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve(SAMPLE_SETTINGS);
      }
      return Promise.resolve(undefined);
    });
  });

  it("persists the chosen theme via update_settings when an option is picked", async () => {
    const user = userEvent.setup();
    render(
      <ThemeProvider>
        <ThemeToggle />
      </ThemeProvider>,
    );

    await user.click(screen.getByRole("button", { name: /change theme/i }));
    await user.click(await screen.findByText("Dark"));

    expect(invoke).toHaveBeenCalledWith(
      "update_settings",
      expect.objectContaining({ patch: expect.objectContaining({ theme: "dark" }) }),
    );
  });
});
