import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";
import "@/lib/i18n";

// Components call these Tauri APIs directly; outside a real Tauri webview
// they'd throw ("not in a Tauri context"), so every unit test gets a
// default no-op/empty mock. Individual tests override return values with
// `vi.mocked(invoke).mockResolvedValueOnce(...)` as needed.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
}));

// jsdom doesn't implement these Pointer Events APIs that Radix UI (used by
// shadcn's Select/DropdownMenu/Dialog) relies on for its interactions.
if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = () => false;
}
if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = () => {};
}
if (!Element.prototype.releasePointerCapture) {
  Element.prototype.releasePointerCapture = () => {};
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = () => {};
}

// jsdom has no `matchMedia`; next-themes uses it to detect the OS color
// scheme when the preference is "system".
if (!window.matchMedia) {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  });
}
