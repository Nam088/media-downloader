import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";

import { MediaPlayer, stopActiveMedia } from "@/components/MediaPlayer";
import tauriConfig from "../../src-tauri/tauri.conf.json";

/**
 * What these tests do NOT prove: that sound comes out of the speakers, that a
 * frame is ever decoded, or that seeking lands where it should. jsdom has no
 * media pipeline at all — `play()`, `pause()` and the decoder are absent, so
 * they are stubbed below with the smallest thing that behaves like the spec
 * (flip `paused`, fire the matching event). Everything asserted here is
 * *wiring*: which URL the element is pointed at, which element gets paused
 * when another starts, and what is rendered when the webview says no.
 * Real playback is a manual check in a bundled build.
 */

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  // Mirrors Tauri's own implementation on macOS/Linux (`scripts/core.js`):
  // `asset://localhost/` + encodeURIComponent(path).
  convertFileSrc: vi.fn(
    (path: string, protocol = "asset") => `${protocol}://localhost/${encodeURIComponent(path)}`,
  ),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
}));

const played: HTMLMediaElement[] = [];
const paused: HTMLMediaElement[] = [];

/** Make a media element report a duration; jsdom's is a read-only NaN. */
function setDuration(element: HTMLMediaElement, seconds: number) {
  Object.defineProperty(element, "duration", { value: seconds, configurable: true });
  fireEvent.loadedMetadata(element);
}

/** Make a media element report a failure, then raise `error` as a webview would. */
function failWith(element: HTMLMediaElement, code: number) {
  Object.defineProperty(element, "error", { value: { code }, configurable: true });
  fireEvent.error(element);
}

beforeAll(() => {
  const setPaused = (element: HTMLMediaElement, value: boolean) =>
    Object.defineProperty(element, "paused", { value, configurable: true });

  Object.defineProperty(HTMLMediaElement.prototype, "play", {
    configurable: true,
    value: function (this: HTMLMediaElement) {
      played.push(this);
      setPaused(this, false);
      this.dispatchEvent(new Event("play"));
      return Promise.resolve();
    },
  });

  Object.defineProperty(HTMLMediaElement.prototype, "pause", {
    configurable: true,
    value: function (this: HTMLMediaElement) {
      paused.push(this);
      if (!this.paused) {
        setPaused(this, true);
        this.dispatchEvent(new Event("pause"));
      }
    },
  });
});

beforeEach(() => {
  played.length = 0;
  paused.length = 0;
  stopActiveMedia();
  vi.mocked(openPath).mockClear();
  vi.mocked(openPath).mockResolvedValue(undefined);
});

describe("MediaPlayer (FR-312)", () => {
  it("points the element at an asset URL built from the path, never at file bytes", () => {
    render(<MediaPlayer filePath="/Users/tester/Downloads/clip.mp4" />);

    expect(convertFileSrc).toHaveBeenCalledWith("/Users/tester/Downloads/clip.mp4");
    expect(screen.getByTestId("media-player-element")).toHaveAttribute(
      "src",
      "asset://localhost/%2FUsers%2Ftester%2FDownloads%2Fclip.mp4",
    );
  });

  it("renders a video element for video and a bare audio element for audio", () => {
    const { unmount } = render(<MediaPlayer filePath="/Users/tester/Downloads/clip.mp4" />);
    expect(screen.getByTestId("media-player-element").tagName).toBe("VIDEO");
    unmount();

    render(<MediaPlayer filePath="/Users/tester/Downloads/song.mp3" />);
    expect(screen.getByTestId("media-player-element").tagName).toBe("AUDIO");
  });

  it("plays, pauses, and reports which state it is in", async () => {
    const user = userEvent.setup();
    render(<MediaPlayer filePath="/Users/tester/Downloads/clip.mp4" />);

    const toggle = screen.getByTestId("media-player-toggle");
    expect(toggle).toHaveAttribute("data-playing", "false");

    await user.click(toggle);
    expect(played).toHaveLength(1);
    expect(toggle).toHaveAttribute("data-playing", "true");

    await user.click(toggle);
    expect(paused).toHaveLength(1);
    expect(toggle).toHaveAttribute("data-playing", "false");
  });

  it("seeks the element to the position the slider was dragged to", () => {
    render(<MediaPlayer filePath="/Users/tester/Downloads/clip.mp4" />);
    const element = screen.getByTestId("media-player-element") as HTMLMediaElement;
    setDuration(element, 240);

    fireEvent.change(screen.getByTestId("media-player-seek"), { target: { value: "90" } });

    expect(element.currentTime).toBe(90);
    expect(screen.getByTestId("media-player-time")).toHaveTextContent("1:30 / 4:00");
  });

  it("sets the element volume and mutes at zero", () => {
    render(<MediaPlayer filePath="/Users/tester/Downloads/clip.mp4" />);
    const element = screen.getByTestId("media-player-element") as HTMLMediaElement;

    fireEvent.change(screen.getByTestId("media-player-volume"), { target: { value: "0.25" } });
    expect(element.volume).toBeCloseTo(0.25);
    expect(element.muted).toBe(false);

    fireEvent.change(screen.getByTestId("media-player-volume"), { target: { value: "0" } });
    expect(element.volume).toBe(0);
    expect(element.muted).toBe(true);
  });
});

describe("MediaPlayer unsupported formats (FR-315)", () => {
  it("does not mount a dead player for a container no webview plays", () => {
    render(<MediaPlayer filePath="/Users/tester/Downloads/movie.mkv" />);

    expect(screen.queryByTestId("media-player-element")).toBeNull();
    const reason = screen.getByTestId("media-player-failure-reason");
    expect(reason).toHaveAttribute("data-reason", "format");
    expect(reason.textContent?.trim()).not.toBe("");
  });

  it("offers to hand the file to the system default application", async () => {
    const user = userEvent.setup();
    render(<MediaPlayer filePath="/Users/tester/Downloads/movie.mkv" />);

    await user.click(screen.getByTestId("media-player-open-externally"));

    expect(openPath).toHaveBeenCalledWith("/Users/tester/Downloads/movie.mkv");
    expect(screen.queryByTestId("media-player-open-failed")).toBeNull();
  });

  it("says so when even the system application could not be launched", async () => {
    const user = userEvent.setup();
    vi.mocked(openPath).mockRejectedValueOnce(new Error("no handler"));
    render(<MediaPlayer filePath="/Users/tester/Downloads/movie.mkv" />);

    await user.click(screen.getByTestId("media-player-open-externally"));

    expect(await screen.findByTestId("media-player-open-failed")).toBeInTheDocument();
  });

  it("falls back with the element's own reason when playback fails at runtime", () => {
    // A `.mp4` is worth attempting, so the player mounts; the codec inside it
    // (or a 403 from the asset protocol) is only discovered by trying.
    render(<MediaPlayer filePath="/Users/tester/Downloads/exotic-codec.mp4" />);

    failWith(screen.getByTestId("media-player-element") as HTMLMediaElement, 4);

    expect(screen.getByTestId("media-player-failure-reason")).toHaveAttribute(
      "data-reason",
      "format",
    );
    expect(screen.getByTestId("media-player-open-externally")).toBeInTheDocument();
  });

  it("distinguishes a decode failure from an unplayable source", () => {
    render(<MediaPlayer filePath="/Users/tester/Downloads/truncated.mp4" />);

    failWith(screen.getByTestId("media-player-element") as HTMLMediaElement, 3);

    expect(screen.getByTestId("media-player-failure-reason")).toHaveAttribute(
      "data-reason",
      "decode",
    );
  });
});

describe("MediaPlayer one item at a time (FR-316)", () => {
  it("stops the item that was playing when a second one starts", async () => {
    const user = userEvent.setup();
    render(
      <>
        <MediaPlayer filePath="/Users/tester/Downloads/first.mp4" />
        <MediaPlayer filePath="/Users/tester/Downloads/second.mp4" />
      </>,
    );

    const [firstToggle, secondToggle] = screen.getAllByTestId("media-player-toggle");
    const [first, second] = screen.getAllByTestId("media-player-element") as HTMLMediaElement[];

    await user.click(firstToggle);
    expect(first.paused).toBe(false);

    await user.click(secondToggle);

    expect(paused).toContain(first);
    expect(first.paused).toBe(true);
    expect(second.paused).toBe(false);
    expect(firstToggle).toHaveAttribute("data-playing", "false");
    expect(secondToggle).toHaveAttribute("data-playing", "true");
  });

  it("releases the speakers when the playing item goes away", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<MediaPlayer filePath="/Users/tester/Downloads/first.mp4" />);
    const element = screen.getByTestId("media-player-element") as HTMLMediaElement;

    await user.click(screen.getByTestId("media-player-toggle"));
    unmount();

    expect(paused).toContain(element);
  });
});

/* ------------------------------------------------------------------------ */
/* FR-313 / FR-314 / SC-306 — the shipped Tauri security configuration.      */
/* ------------------------------------------------------------------------ */

const security = tauriConfig.app.security;

/**
 * Where Tauri's `$VARIABLE` scope prefixes land on macOS
 * (`tauri::path::BaseDirectory`). A fixed fixture, not a lookup of this
 * machine's real directories — the point is to evaluate the *policy*, which
 * is the same on every machine, against paths that are obviously private.
 */
const HOME = "/Users/tester";
const BASE_DIRECTORIES: Record<string, string> = {
  $HOME: HOME,
  $DOWNLOAD: `${HOME}/Downloads`,
  $VIDEO: `${HOME}/Movies`,
  $AUDIO: `${HOME}/Music`,
  $PICTURE: `${HOME}/Pictures`,
  $DOCUMENT: `${HOME}/Documents`,
  $DESKTOP: `${HOME}/Desktop`,
  $PUBLIC: `${HOME}/Public`,
  $DATA: `${HOME}/Library/Application Support`,
  $LOCALDATA: `${HOME}/Library/Application Support`,
  $CACHE: `${HOME}/Library/Caches`,
  $CONFIG: `${HOME}/Library/Application Support`,
  $TEMP: "/tmp",
  $APPDATA: `${HOME}/Library/Application Support/io.github.nam088.mediadownloader`,
  $APPLOCALDATA: `${HOME}/Library/Application Support/io.github.nam088.mediadownloader`,
  $APPCACHE: `${HOME}/Library/Caches/io.github.nam088.mediadownloader`,
  $APPCONFIG: `${HOME}/Library/Application Support/io.github.nam088.mediadownloader`,
  $APPLOG: `${HOME}/Library/Logs/io.github.nam088.mediadownloader`,
};

/** Tauri substitutes a `$VARIABLE` only in the pattern's first component. */
function resolveScopePattern(pattern: string): string {
  const [first, ...rest] = pattern.split("/");
  if (!first.startsWith("$")) return pattern;
  const base = BASE_DIRECTORIES[first];
  if (base === undefined) {
    throw new Error(`Scope pattern uses an unmapped base directory: ${first}`);
  }
  return [base, ...rest].join("/");
}

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function componentMatches(pattern: string, component: string): boolean {
  if (/[?[\]]/.test(pattern)) {
    throw new Error(`Unsupported glob metacharacter in scope pattern: ${pattern}`);
  }
  if (!pattern.includes("*")) return pattern === component;
  // require_literal_leading_dot: a wildcard never matches a hidden component.
  if (component.startsWith(".")) return false;
  const source = pattern.split("*").map(escapeRegExp).join("[^/]*");
  return new RegExp(`^${source}$`).test(component);
}

/**
 * Path matching as the Rust `glob` crate performs it under the options Tauri
 * passes (`require_literal_separator: true`, `require_literal_leading_dot:
 * true`): `*` stays inside one component, `**` spans one or more components,
 * and neither crosses into a dot-prefixed name. The table in "mirrors the Rust
 * glob crate" below is captured output from that crate, so this translation is
 * verified rather than assumed.
 */
function matchesGlob(pattern: string, path: string): boolean {
  const patternParts = pattern.split("/");
  const pathParts = path.split("/");

  const walk = (p: number, s: number): boolean => {
    if (p === patternParts.length) return s === pathParts.length;
    if (patternParts[p] === "**") {
      for (let end = s; end < pathParts.length; end++) {
        if (pathParts[end].startsWith(".")) return false;
        if (walk(p + 1, end + 1)) return true;
      }
      return false;
    }
    if (s >= pathParts.length) return false;
    return componentMatches(patternParts[p], pathParts[s]) && walk(p + 1, s + 1);
  };

  return walk(0, 0);
}

/** Exactly what `tauri::scope::fs::Scope::is_allowed` computes, on the shipped scope. */
function assetProtocolAllows(path: string): boolean {
  const scope = security.assetProtocol.scope;
  const denied = (scope.deny as string[]).some((p) => matchesGlob(resolveScopePattern(p), path));
  if (denied) return false;
  return scope.allow.some((p) => matchesGlob(resolveScopePattern(p), path));
}

describe("asset protocol glob semantics", () => {
  /*
   * Verbatim output of `glob::Pattern::matches_path_with` (glob 0.3, the crate
   * Tauri 2.11.5 uses) run with Tauri's MatchOptions. Without this table the
   * matcher below would only be checked against itself.
   */
  const RUST_GLOB_RESULTS: Array<[string, string, boolean]> = [
    ["/Users/me/Downloads/**", "/Users/me/Downloads/video.mp4", true],
    ["/Users/me/Downloads/**", "/Users/me/Downloads/sub/video.mp4", true],
    ["/Users/me/Downloads/**", "/Users/me/Downloads/sub/deep/video.mp4", true],
    ["/Users/me/Downloads/**", "/Users/me/Downloads/.hidden.mp4", false],
    ["/Users/me/Downloads/**", "/Users/me/Downloads/.cache/x.mp4", false],
    ["/Users/me/Downloads/**", "/Users/me/Downloads", false],
    ["/Users/me/Downloads/**", "/Users/me/.ssh/id_rsa", false],
    ["/Users/me/Downloads/**", "/Users/me/Documents/tax.pdf", false],
    ["/Users/me/Downloads/**", "/etc/passwd", false],
    ["/Users/me/Downloads/*", "/Users/me/Downloads/video.mp4", true],
    ["/Users/me/Downloads/*", "/Users/me/Downloads/sub/video.mp4", false],
    ["/Users/me/Downloads/*", "/Users/me/Downloads/sub/deep/video.mp4", false],
    ["/Users/me/Downloads/*", "/Users/me/Downloads/.hidden.mp4", false],
    ["/Users/me/Downloads/*", "/Users/me/Downloads/.cache/x.mp4", false],
    ["/Users/me/Downloads/*", "/Users/me/Downloads", false],
    ["/Users/me/Downloads/*", "/Users/me/.ssh/id_rsa", false],
    ["/Users/me/Downloads/*", "/Users/me/Documents/tax.pdf", false],
    ["/Users/me/Downloads/*", "/etc/passwd", false],
    ["/Users/me/Downloads", "/Users/me/Downloads/video.mp4", false],
    ["/Users/me/Downloads", "/Users/me/Downloads", true],
    ["/Users/me/Downloads", "/Users/me/.ssh/id_rsa", false],
    ["/Users/me/**", "/Users/me/Downloads/video.mp4", true],
    ["/Users/me/**", "/Users/me/Downloads/sub/deep/video.mp4", true],
    ["/Users/me/**", "/Users/me/Downloads/.hidden.mp4", false],
    ["/Users/me/**", "/Users/me/Downloads", true],
    ["/Users/me/**", "/Users/me/.ssh/id_rsa", false],
    ["/Users/me/**", "/Users/me/Documents/tax.pdf", true],
    ["/Users/me/**", "/etc/passwd", false],
  ];

  it.each(RUST_GLOB_RESULTS)("mirrors the Rust glob crate: %s vs %s", (pattern, path, expected) => {
    expect(matchesGlob(pattern, path)).toBe(expected);
  });
});

describe("asset protocol scope (FR-313 / SC-306)", () => {
  it("is enabled, so the webview can stream a local file at all", () => {
    expect(security.assetProtocol.enable).toBe(true);
    expect(security.assetProtocol.scope.allow.length).toBeGreaterThan(0);
  });

  it.each([
    `${HOME}/Downloads/clip.mp4`,
    `${HOME}/Downloads/Some Playlist/01 - track.m4a`,
    `${HOME}/Movies/clip.mp4`,
    `${HOME}/Music/song.mp3`,
    `${HOME}/Pictures/gallery/image.jpg`,
  ])("can reach a downloaded file at %s", (path) => {
    expect(assetProtocolAllows(path)).toBe(true);
  });

  it.each([
    "/etc/passwd",
    "/etc/hosts",
    `${HOME}/.ssh/id_rsa`,
    `${HOME}/.aws/credentials`,
    `${HOME}/Documents/passport-scan.pdf`,
    `${HOME}/Desktop/notes.txt`,
    `${HOME}/Library/Application Support/Firefox/profiles.ini`,
    `${HOME}/Library/Keychains/login.keychain-db`,
    `${HOME}/Downloads/.hidden-secret`,
    "/Users/someone-else/Downloads/clip.mp4",
    "/Applications/Safari.app/Contents/Info.plist",
  ])("cannot reach %s", (path) => {
    expect(assetProtocolAllows(path)).toBe(false);
  });
});

describe("content security policy (FR-314)", () => {
  it("is configured instead of disabled", () => {
    expect(security.csp).not.toBeNull();
    expect(security.csp["default-src"]).toBe("'self'");
  });

  it("lets the webview load the asset protocol it now serves media from", () => {
    for (const directive of ["media-src", "img-src"] as const) {
      expect(security.csp[directive]).toContain("asset:");
      expect(security.csp[directive]).toContain("http://asset.localhost");
    }
  });

  it("keeps the existing remote preview thumbnails loadable", () => {
    // DownloadForm, PlaylistDetailPanel and GalleryItemPicker render
    // `<img src={preview.thumbnail_url}>` straight from the source CDN.
    expect(security.csp["img-src"]).toContain("https:");
  });

  it("keeps Tauri's own IPC channel reachable", () => {
    expect(security.csp["connect-src"]).toContain("ipc:");
    expect(security.csp["connect-src"]).toContain("http://ipc.localhost");
  });

  it("does not hand the renderer arbitrary script execution", () => {
    expect(security.csp["script-src"]).toBe("'self'");
    expect(security.csp["object-src"]).toBe("'none'");
  });
});
