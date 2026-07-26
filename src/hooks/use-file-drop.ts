import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { dedupeUrls } from "@/lib/url-parsing";

/** Extensions treated as a plain-text list of URLs. */
export const URL_LIST_EXTENSIONS = ["txt", "list", "csv"] as const;

export function isUrlListPath(path: string): boolean {
  const lowered = path.toLowerCase();
  return URL_LIST_EXTENSIONS.some((extension) => lowered.endsWith(`.${extension}`));
}

/**
 * Read one or more URL-list files through the backend and merge the results.
 *
 * The reading happens in Rust (`read_url_list_file`) on purpose: the webview
 * needs exactly the ability "read the one file the user just pointed at", so
 * there is no reason to hand it filesystem permissions. The parsing rules on
 * that side are a deliberate character-for-character match of
 * `lib/url-parsing.ts`, so a pasted file and a dropped file produce the same
 * list — which is also why the merge below uses `dedupeUrls` rather than a
 * second, subtly different notion of "same URL".
 *
 * A file that can't be read is skipped, not fatal: dropping five lists and
 * losing all five because one was a binary is worse than losing the one.
 */
export async function readUrlListFiles(paths: string[]): Promise<string[]> {
  const collected: string[] = [];
  for (const path of paths) {
    try {
      const urls = await invoke<string[]>("read_url_list_file", { path });
      collected.push(...urls);
    } catch (error) {
      console.error("failed to read dropped url list", path, error);
    }
  }
  return dedupeUrls(collected).unique;
}

/**
 * Accept URL-list files dropped onto the window (FR-105).
 *
 * URLs dragged straight out of a browser take a different route entirely —
 * they arrive as a paste/drop of text on the textarea itself, handled in
 * `DownloadForm`.
 */
export function useFileDrop(onUrls: (urls: string[]) => void) {
  // Held in a ref so a caller that rebuilds its callback each render doesn't
  // tear down and re-register the window listener on every render.
  const onUrlsRef = useRef(onUrls);
  useEffect(() => {
    onUrlsRef.current = onUrls;
  });

  useEffect(() => {
    let disposed = false;

    const unlistenPromise = listen<{ paths: string[] }>("tauri://drag-drop", (event) => {
      const listFiles = (event.payload.paths ?? []).filter(isUrlListPath);
      if (listFiles.length === 0) return;

      void readUrlListFiles(listFiles).then((urls) => {
        // The drop may land moments before the form unmounts; delivering URLs
        // into a dead callback would be at best pointless.
        if (!disposed) onUrlsRef.current(urls);
      });
    });

    return () => {
      disposed = true;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);
}
