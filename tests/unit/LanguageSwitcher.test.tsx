import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import i18n from "@/lib/i18n";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";

describe("LanguageSwitcher", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "get_settings") {
        return Promise.resolve({ theme: "system", language: "system", default_output_directory: "" });
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
