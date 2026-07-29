import { useCallback, useRef, useState } from "react";
import type { SyntheticEvent } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { ExternalLink, Pause, Play, Volume2, VolumeX } from "lucide-react";

import { Button } from "@/components/ui/button";
import { formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";

/**
 * FR-312/FR-315/FR-316 — in-app preview of a downloaded file.
 *
 * The file never enters JavaScript as bytes: `convertFileSrc` turns the path
 * into an `asset://localhost/<path>` URL (`http://asset.localhost/<path>` on
 * Windows) that the webview streams itself, honouring range requests so a
 * multi-GB video seeks without being buffered into memory. Which paths that
 * URL may resolve is decided by `app.security.assetProtocol.scope` in
 * `tauri.conf.json` (FR-313) — the UI cannot widen it.
 *
 * Everything the player knows about playback comes from media element events
 * rather than from `play()`/`pause()` return values, so the state shown is the
 * element's real state whether playback was started by our button, by
 * `autoPlay`, or stopped by another player claiming the speakers (FR-316).
 */

export type MediaKind = "audio" | "video";

/** Reason the player gave up, all of which FR-315 requires be said out loud. */
type Failure =
  | "format" // container/codec the webview refuses (known list, or MEDIA_ERR_SRC_NOT_SUPPORTED)
  | "decode" // MEDIA_ERR_DECODE — started, then choked
  | "network" // MEDIA_ERR_NETWORK — asset protocol refused or the read failed
  | "unknown";

const AUDIO_EXTENSIONS = new Set([
  "aac",
  "aiff",
  "alac",
  "ape",
  "flac",
  "m4a",
  "mp3",
  "oga",
  "ogg",
  "opus",
  "ra",
  "wav",
  "weba",
  "wma",
]);

const VIDEO_EXTENSIONS = new Set([
  "3gp",
  "asf",
  "avi",
  "divx",
  "flv",
  "m2ts",
  "m4v",
  "mkv",
  "mov",
  "mp4",
  "mpeg",
  "mpg",
  "ogv",
  "rmvb",
  "ts",
  "vob",
  "webm",
  "wmv",
]);

/**
 * Containers no shipping webview engine plays: WKWebView (macOS), WebView2
 * (Windows) and WebKitGTK (Linux) all decline these, so there is no point
 * mounting a media element that can only fail — say so up front instead
 * (FR-315). Formats whose support is version- or codec-dependent (webm, ogg,
 * opus) are deliberately absent: those get attempted, and the element's own
 * `error` event produces the same fallback if the attempt fails.
 */
const WEBVIEW_UNSUPPORTED_EXTENSIONS = new Set([
  "ape",
  "asf",
  "avi",
  "divx",
  "flv",
  "m2ts",
  "mkv",
  "mpeg",
  "mpg",
  "ra",
  "rmvb",
  "ts",
  "vob",
  "wma",
  "wmv",
]);

function extensionOf(filePath: string): string {
  const name = filePath.split(/[\\/]/).pop() ?? "";
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
}

function detectMediaKind(filePath: string): MediaKind {
  const extension = extensionOf(filePath);
  if (AUDIO_EXTENSIONS.has(extension)) return "audio";
  if (VIDEO_EXTENSIONS.has(extension)) return "video";
  // Unknown extension: a video element also plays audio-only streams, so it is
  // the guess that can still work. If it cannot, `onError` takes over.
  return "video";
}

/** MEDIA_ERR_* → the reason we tell the user about. */
function failureFromMediaError(error: MediaError | null): Failure {
  switch (error?.code) {
    case 2:
      return "network";
    case 3:
      return "decode";
    case 4:
      return "format";
    default:
      return "unknown";
  }
}

/**
 * FR-316 — one item at a time. A module-level reference is what makes this
 * work across separate `MediaPlayer` instances that share no React state: the
 * element that most recently fired `play` owns the speakers, and pausing the
 * previous owner fires its `pause` event, which flips its own UI back.
 */
let activeMedia: HTMLMediaElement | null = null;

function claimPlayback(element: HTMLMediaElement) {
  if (activeMedia && activeMedia !== element) {
    activeMedia.pause();
  }
  activeMedia = element;
}

function releasePlayback(element: HTMLMediaElement) {
  if (activeMedia === element) {
    activeMedia = null;
  }
}

/** Stops whatever is playing, e.g. before the item is deleted or the app closes. */
export function stopActiveMedia() {
  activeMedia?.pause();
  activeMedia = null;
}

export interface MediaPlayerProps {
  /** Absolute path of the downloaded file, as stored in the library index. */
  filePath: string;
  /** Shown above the controls; falls back to the file name. */
  title?: string;
  /** Overrides the extension-based guess. */
  kind?: MediaKind;
  autoPlay?: boolean;
  className?: string;
}

/**
 * Remounts the player whenever the file changes, which resets every piece of
 * playback state — position, volume, and any failure recorded for the previous
 * file — without an effect that has to remember to reset each one. Same
 * `key`-on-identity pattern `PlaylistDetailPanel` uses.
 */
export function MediaPlayer(props: MediaPlayerProps) {
  return <MediaPlayerInner key={props.filePath} {...props} />;
}

function MediaPlayerInner({ filePath, title, kind, autoPlay, className }: MediaPlayerProps) {
  const { t } = useTranslation();
  const mediaRef = useRef<HTMLVideoElement | HTMLAudioElement | null>(null);

  const resolvedKind = kind ?? detectMediaKind(filePath);
  const unplayableExtension = WEBVIEW_UNSUPPORTED_EXTENSIONS.has(extensionOf(filePath));

  const [failure, setFailure] = useState<Failure | null>(unplayableExtension ? "format" : null);
  const [playing, setPlaying] = useState(false);
  const [duration, setDuration] = useState(0);
  const [currentTime, setCurrentTime] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [openFailed, setOpenFailed] = useState(false);

  // Never leave a detached element holding the speakers (FR-316): the item can
  // be closed, deleted, or filtered out of the list while it is playing. A ref
  // cleanup (React 19) runs at exactly the moment the element goes away, which
  // an effect cleanup cannot promise once refs are detached.
  const attachMedia = useCallback((element: HTMLVideoElement & HTMLAudioElement) => {
    mediaRef.current = element;
    return () => {
      element.pause();
      releasePlayback(element);
      if (mediaRef.current === element) mediaRef.current = null;
    };
  }, []);

  const source = convertFileSrc(filePath);

  const togglePlayback = useCallback(() => {
    const element = mediaRef.current;
    if (!element) return;

    if (element.paused) {
      claimPlayback(element);
      const started = element.play();
      // `play()` rejects when the webview cannot decode the source at all;
      // that is the same dead end as an `error` event, so it lands in the
      // same place instead of leaving a button that does nothing.
      if (started && typeof started.catch === "function") {
        started.catch((error: unknown) => {
          setPlaying(false);
          if (!(error instanceof Error) || error.name !== "AbortError") {
            setFailure("format");
          }
        });
      }
    } else {
      element.pause();
    }
  }, []);

  const seek = useCallback((seconds: number) => {
    const element = mediaRef.current;
    if (!element) return;
    element.currentTime = seconds;
    setCurrentTime(seconds);
  }, []);

  const changeVolume = useCallback((next: number) => {
    const element = mediaRef.current;
    if (!element) return;
    element.volume = next;
    element.muted = next === 0;
    setVolume(next);
    setMuted(next === 0);
  }, []);

  const toggleMuted = useCallback(() => {
    const element = mediaRef.current;
    if (!element) return;
    element.muted = !element.muted;
    setMuted(element.muted);
  }, []);

  const openExternally = useCallback(async () => {
    try {
      setOpenFailed(false);
      await invoke("open_file", { path: filePath });
    } catch {
      setOpenFailed(true);
    }
  }, [filePath]);

  const mediaProps = {
    ref: attachMedia,
    src: source,
    preload: "metadata" as const,
    autoPlay,
    "data-testid": "media-player-element",
    onPlay: (event: SyntheticEvent<HTMLMediaElement>) => {
      claimPlayback(event.currentTarget);
      setPlaying(true);
    },
    onPause: () => setPlaying(false),
    onEnded: () => setPlaying(false),
    onLoadedMetadata: (event: SyntheticEvent<HTMLMediaElement>) => {
      const value = event.currentTarget.duration;
      setDuration(Number.isFinite(value) ? value : 0);
    },
    onTimeUpdate: (event: SyntheticEvent<HTMLMediaElement>) =>
      setCurrentTime(event.currentTarget.currentTime),
    onVolumeChange: (event: SyntheticEvent<HTMLMediaElement>) => {
      setVolume(event.currentTarget.volume);
      setMuted(event.currentTarget.muted);
    },
    onError: (event: SyntheticEvent<HTMLMediaElement>) => {
      setPlaying(false);
      setFailure(failureFromMediaError(event.currentTarget.error));
    },
  };

  const heading = title ?? (filePath.split(/[\\/]/).pop() || filePath);

  if (failure !== null) {
    return (
      <div
        className={cn(
          "flex flex-col gap-3 rounded-lg border border-border/80 bg-muted/30 p-4",
          className,
        )}
        data-testid="media-player-fallback"
      >
        <p className="truncate text-sm font-semibold text-foreground/90">{heading}</p>
        <p
          className="text-xs text-muted-foreground"
          data-testid="media-player-failure-reason"
          data-reason={failure}
        >
          {t(`mediaPlayer.cannot_play_${failure}`)}
        </p>
        <div className="flex items-center gap-3">
          <Button
            variant="outline"
            size="sm"
            className="gap-2"
            onClick={() => void openExternally()}
            data-testid="media-player-open-externally"
          >
            <ExternalLink className="h-4 w-4" />
            {t("mediaPlayer.open_in_default_app")}
          </Button>
          {openFailed && (
            <span className="text-xs text-destructive" data-testid="media-player-open-failed">
              {t("mediaPlayer.open_in_default_app_failed")}
            </span>
          )}
        </div>
      </div>
    );
  }

  return (
    <div
      className={cn(
        "flex flex-col gap-3 rounded-lg border border-border/80 bg-muted/30 p-4",
        className,
      )}
      data-testid="media-player"
    >
      <p className="truncate text-sm font-semibold text-foreground/90">{heading}</p>

      {resolvedKind === "video" ? (
        <video {...mediaProps} className="max-h-[60vh] w-full rounded-md bg-black" playsInline />
      ) : (
        <audio {...mediaProps} className="hidden" />
      )}

      <div className="flex items-center gap-3">
        <Button
          variant="outline"
          size="icon"
          onClick={togglePlayback}
          aria-label={playing ? t("mediaPlayer.pause") : t("mediaPlayer.play")}
          data-testid="media-player-toggle"
          data-playing={playing ? "true" : "false"}
        >
          {playing ? <Pause className="h-4 w-4" /> : <Play className="h-4 w-4" />}
        </Button>

        <span
          className="shrink-0 font-mono text-xs text-muted-foreground"
          data-testid="media-player-time"
        >
          {formatDuration(currentTime)} / {formatDuration(duration)}
        </span>

        <input
          type="range"
          min={0}
          max={duration || 0}
          step={0.1}
          value={Math.min(currentTime, duration || 0)}
          onChange={(event) => seek(Number(event.target.value))}
          aria-label={t("mediaPlayer.seek")}
          data-testid="media-player-seek"
          className="h-1 min-w-0 flex-1 cursor-pointer accent-primary"
        />

        <Button
          variant="ghost"
          size="icon"
          onClick={toggleMuted}
          aria-label={muted ? t("mediaPlayer.unmute") : t("mediaPlayer.mute")}
          data-testid="media-player-mute"
        >
          {muted ? <VolumeX className="h-4 w-4" /> : <Volume2 className="h-4 w-4" />}
        </Button>

        <input
          type="range"
          min={0}
          max={1}
          step={0.05}
          value={muted ? 0 : volume}
          onChange={(event) => changeVolume(Number(event.target.value))}
          aria-label={t("mediaPlayer.volume")}
          data-testid="media-player-volume"
          className="h-1 w-24 shrink-0 cursor-pointer accent-primary"
        />
      </div>
    </div>
  );
}
