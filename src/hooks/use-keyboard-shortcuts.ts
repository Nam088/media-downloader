import { useEffect } from "react";

interface KeyboardShortcutsOptions {
  onSearchFocus?: () => void;
  onSpaceToggle?: () => void;
  onEscapeClose?: () => void;
  onSecretTrigger?: () => void;
}

export function useKeyboardShortcuts(options: KeyboardShortcutsOptions = {}) {
  useEffect(() => {
    function handleKeyDown(e: KeyboardEvent) {
      const activeElement = document.activeElement;
      const isInputActive =
        activeElement &&
        (activeElement.tagName === "INPUT" ||
          activeElement.tagName === "TEXTAREA" ||
          (activeElement as HTMLElement).isContentEditable);

      // Secret combination: Cmd+Shift+M or Ctrl+Shift+M
      if ((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "m") {
        e.preventDefault();
        if (options.onSecretTrigger) {
          options.onSecretTrigger();
        }
        return;
      }

      // Cmd+K or Ctrl+K -> Focus Search
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        if (options.onSearchFocus) {
          options.onSearchFocus();
        } else {
          // Default: try focusing search inputs
          const searchInput =
            document.querySelector<HTMLInputElement>("input[type='search']") ||
            document.querySelector<HTMLInputElement>("input[placeholder*='search']") ||
            document.querySelector<HTMLInputElement>("input[placeholder*='tìm']") ||
            document.querySelector<HTMLInputElement>("input[data-testid*='search']");
          if (searchInput) {
            searchInput.focus();
            searchInput.select();
          }
        }
        return;
      }

      // Space -> Toggle playback when not typing
      if (e.code === "Space" && !isInputActive) {
        // Prevent page scrolling on Space bar
        if (options.onSpaceToggle) {
          e.preventDefault();
          options.onSpaceToggle();
        } else {
          const playPauseBtn = document.querySelector<HTMLButtonElement>(
            "[data-testid='media-player-toggle']",
          );
          if (playPauseBtn) {
            e.preventDefault();
            playPauseBtn.click();
          }
        }
        return;
      }

      // Esc -> Close active modals / drawers
      if (e.key === "Escape") {
        if (options.onEscapeClose) {
          options.onEscapeClose();
        } else {
          const closeBtn =
            document.querySelector<HTMLButtonElement>("[aria-label*='Close']") ||
            document.querySelector<HTMLButtonElement>("[data-testid*='close']");
          if (closeBtn) {
            closeBtn.click();
          }
        }
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [options]);
}
