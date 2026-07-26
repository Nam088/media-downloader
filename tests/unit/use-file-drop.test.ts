import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Mock } from "vitest";

import { useFileDrop } from "@/hooks/use-file-drop";

/**
 * jsdom has no Tauri runtime, so these tests can only prove the *handler*:
 * given a `tauri://drag-drop` payload, it filters paths and reads the right
 * ones through the backend. That Tauri actually emits the event for a real
 * drop is a manual check (`pnpm tauri dev`).
 */
type DragDropHandler = (event: { payload: { paths: string[] } }) => void;

const listenMock = listen as unknown as Mock;

describe("useFileDrop (FR-104, FR-105)", () => {
  let handler: DragDropHandler | null = null;
  let unlisten: Mock;

  beforeEach(() => {
    handler = null;
    unlisten = vi.fn();
    vi.mocked(invoke).mockReset();
    listenMock.mockReset();
    listenMock.mockImplementation((_event: string, callback: DragDropHandler) => {
      handler = callback;
      return Promise.resolve(unlisten);
    });
  });

  async function drop(paths: string[]) {
    await waitFor(() => expect(handler).not.toBeNull());
    handler?.({ payload: { paths } });
  }

  it("subscribes to the window's drag-drop event", async () => {
    renderHook(() => useFileDrop(vi.fn()));

    await waitFor(() => expect(listenMock).toHaveBeenCalledWith("tauri://drag-drop", expect.any(Function)));
  });

  it("reads urls out of a dropped text file through the backend", async () => {
    vi.mocked(invoke).mockResolvedValue(["https://a.example/1", "https://b.example/2"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/list.txt"]);

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("read_url_list_file", { path: "/tmp/list.txt" });
      expect(onUrls).toHaveBeenCalledWith(["https://a.example/1", "https://b.example/2"]);
    });
  });

  it("ignores files that are not text lists", async () => {
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/photo.png", "/tmp/clip.mp4"]);

    // Nothing to await on the happy path, so give any stray promise chain a
    // chance to run before asserting that it did not.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(invoke).not.toHaveBeenCalled();
    expect(onUrls).not.toHaveBeenCalled();
  });

  it("picks the list files out of a mixed drop and leaves the rest alone", async () => {
    vi.mocked(invoke).mockResolvedValue(["https://a.example/1"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/photo.png", "/tmp/List.TXT"]);

    await waitFor(() => expect(onUrls).toHaveBeenCalledWith(["https://a.example/1"]));
    expect(vi.mocked(invoke).mock.calls).toEqual([
      ["read_url_list_file", { path: "/tmp/List.TXT" }],
    ]);
  });

  it("merges urls from several dropped files and drops repeats", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(["https://a.example/1", "https://shared.example/x"])
      .mockResolvedValueOnce(["https://shared.example/x", "https://b.example/2"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/a.txt", "/tmp/b.list"]);

    await waitFor(() =>
      expect(onUrls).toHaveBeenCalledWith([
        "https://a.example/1",
        "https://shared.example/x",
        "https://b.example/2",
      ]),
    );
  });

  it("still delivers the readable files when one of them fails", async () => {
    vi.mocked(invoke)
      .mockRejectedValueOnce({ code: "FILE_TOO_LARGE", message: "too big" })
      .mockResolvedValueOnce(["https://b.example/2"]);
    const onUrls = vi.fn();
    renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/huge.txt", "/tmp/b.txt"]);

    await waitFor(() => expect(onUrls).toHaveBeenCalledWith(["https://b.example/2"]));
  });

  it("stops listening when the component goes away", async () => {
    const { unmount } = renderHook(() => useFileDrop(vi.fn()));

    await waitFor(() => expect(handler).not.toBeNull());
    unmount();

    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });

  it("does not deliver urls to a callback whose component already unmounted", async () => {
    // Held on an object rather than in a bare `let`: TypeScript's control-flow
    // analysis can't see the assignment that happens inside the executor.
    const pending: { release?: (urls: string[]) => void } = {};
    vi.mocked(invoke).mockImplementation(
      () =>
        new Promise<string[]>((resolve) => {
          pending.release = resolve;
        }),
    );
    const onUrls = vi.fn();
    const { unmount } = renderHook(() => useFileDrop(onUrls));

    await drop(["/tmp/list.txt"]);
    await waitFor(() => expect(invoke).toHaveBeenCalled());

    unmount();
    pending.release?.(["https://a.example/1"]);
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(onUrls).not.toHaveBeenCalled();
  });
});
