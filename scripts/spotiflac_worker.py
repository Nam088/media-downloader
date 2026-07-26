#!/usr/bin/env python3
"""spotiflac_worker — bridge process between media-downloader (Rust) and the
SpotiFLAC Python module.

One spawn handles exactly one unit of work:
  * ``preview --url <URL>``  — resolve metadata only, no download
  * ``download --url <track-URL> --output-dir <dir> ...`` — download ONE track

Every structured event is a single stdout line: ``SPOTIFLAC_EVENT::{json}``.
Lines without that prefix are raw module logs (forwarded by Rust at debug
level). Commands from Rust arrive as single JSON lines on stdin
(``{"type":"grant","value":"..."}`` / ``{"type":"cancel"}``).

Protocol contract: specs/006-spotiflac-integration/contracts/spotiflac-worker-protocol.md
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import json
import logging
import os
import queue
import re
import sys
import tempfile
import threading
import time

PROTOCOL_VERSION = 1
SENTINEL = "SPOTIFLAC_EVENT::"

EXIT_OK = 0
EXIT_ERROR = 1
EXIT_USAGE = 2
EXIT_CANCELED = 130

# App-facing tiers → the module's canonical quality names (normalize_quality()
# in SpotiFLAC.core.quality translates these per provider, e.g. Qobuz "6"/"27").
# mp3_320 downloads FLAC 16-bit; the Rust side transcodes with bundled ffmpeg.
TIER_TO_QUALITY = {
    "flac16": "LOSSLESS",
    "flac24": "HI_RES_LOSSLESS",
    "mp3_320": "LOSSLESS",
}

KNOWN_PROVIDERS = (
    "tidal",
    "qobuz",
    "deezer",
    "amazon",
    "soundcloud",
    "youtube",
    "apple",
    "pandora",
    "joox",
    "netease",
    "migu",
    "kuwo",
)

_emit_lock = threading.Lock()


def emit(event: dict) -> None:
    """Write one structured event line. Single-line JSON, always flushed."""
    line = SENTINEL + json.dumps(event, ensure_ascii=False, separators=(",", ":"))
    with _emit_lock:
        sys.stdout.write(line + "\n")
        sys.stdout.flush()


def emit_error(code: str, message: str, provider: str | None = None) -> None:
    payload = {"type": "error", "code": code, "message": message}
    if provider:
        payload["provider"] = provider
    emit(payload)


def classify_exception(exc: BaseException) -> str:
    text = f"{type(exc).__name__}: {exc}".lower()
    network_markers = (
        "timeout",
        "timed out",
        "connection",
        "network",
        "dns",
        "ssl",
        "eof",
        "reset",
        "refused",
        "unreachable",
        "temporarily",
        "503",
        "502",
        "429",
    )
    if any(m in text for m in network_markers):
        return "SPOTIFLAC_NETWORK"
    not_found_markers = ("not found", "no result", "no match", "no provider", "unavailable")
    if any(m in text for m in not_found_markers):
        return "SPOTIFLAC_NO_SOURCE"
    region_markers = ("region", "country", "geo", "not available in")
    if any(m in text for m in region_markers):
        return "SPOTIFLAC_REGION_BLOCKED"
    return "SPOTIFLAC_INTERNAL"


class StdinBridge:
    """Background reader for JSON-line commands from the Rust parent.

    ``cancel`` flips an event the asyncio side polls; ``grant`` values are
    queued for whoever is blocked waiting on a Cloudflare challenge.
    """

    def __init__(self) -> None:
        self.cancel_event = threading.Event()
        self.grants: queue.Queue[str] = queue.Queue()
        self._thread = threading.Thread(target=self._run, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _run(self) -> None:
        for raw in sys.stdin:
            raw = raw.strip()
            if not raw:
                continue
            try:
                cmd = json.loads(raw)
            except json.JSONDecodeError:
                continue
            kind = cmd.get("type")
            if kind == "cancel":
                self.cancel_event.set()
                return
            if kind == "grant":
                value = str(cmd.get("value", "")).strip()
                if value:
                    self.grants.put(value)
        # stdin closed by the parent: treat as cancel so we never linger.
        self.cancel_event.set()


class ProviderLogWatcher(logging.Handler):
    """Derives provider activity from the module's own log stream.

    SpotiFLAC logs provider work with a ``[provider]`` prefix (e.g.
    ``[tidal] ...``, ``[amazon] ...``, ``[ext:tidal-web] ...``). The first
    sighting emits ``track_start``; each change emits ``provider_switch``.
    Non-matching records are forwarded as raw log lines for Rust's debug log.
    """

    _TAG_RE = re.compile(r"\[(ext:[\w.-]+|" + "|".join(KNOWN_PROVIDERS) + r")\]", re.IGNORECASE)

    def __init__(self) -> None:
        super().__init__(level=logging.DEBUG)
        self.current_provider: str | None = None

    def emit(self, record: logging.LogRecord) -> None:  # noqa: A003 - logging API
        try:
            message = record.getMessage()
        except Exception:  # pragma: no cover - defensive, logging API contract
            return
        match = self._TAG_RE.search(message)
        if match:
            provider = match.group(1).lower()
            if self.current_provider is None:
                self.current_provider = provider
                emit({"type": "track_start", "provider": provider})
            elif provider != self.current_provider:
                emit(
                    {
                        "type": "provider_switch",
                        "from": self.current_provider,
                        "to": provider,
                        "reason": "module fallback",
                    },
                )
                self.current_provider = provider
                emit({"type": "track_start", "provider": provider})
        write_log(message)


def install_module_logging(watcher: ProviderLogWatcher) -> None:
    spotiflac_logger = logging.getLogger("SpotiFLAC")
    spotiflac_logger.handlers = [watcher]
    spotiflac_logger.setLevel(logging.INFO)
    spotiflac_logger.propagate = False


# --- Cloudflare challenge -------------------------------------------------
#
# SpotiFLAC resolves a challenge in three escalating modes (see
# SpotiFLAC/core/signed_session_desktop.py::run_community_verification):
#   1. registered GUI handlers, if any
#   2. the automated nodriver/Chrome solver
#   3. _run_manual_terminal_verification(), which prompts on stdin
#
# We deliberately leave 1 unset so the automated solver still gets first
# crack — most challenges never reach the user. Mode 3 is the seam we take
# over: it already receives the fully-formed challenge URL and its only job
# is to produce a grant string.
#
# The Telegram path is ours to implement: the module does not read
# TG_BOT_TOKEN at all (upstream's telegram_wrapper.py is an external process
# that scrapes the CLI's output and pipes a grant into its stdin — exactly the
# role this worker's parent plays here).

TELEGRAM_API = "https://api.telegram.org"
GRANT_WAIT_TIMEOUT_S = 15 * 60
TELEGRAM_POLL_INTERVAL_S = 3.0


def telegram_credentials() -> tuple[str, str] | None:
    token = os.environ.get("TG_BOT_TOKEN", "").strip()
    chat_id = os.environ.get("TG_CHAT_ID", "").strip()
    return (token, chat_id) if token and chat_id else None


def write_log(message: str) -> None:
    """Emits a plain (non-event) line for the parent to keep as debug output.

    Everything written here is redacted first: a failed requests call puts the
    full URL in its exception text, and the bot token lives in that URL — the
    parent stores these lines in its own log buffer, which the user can read
    and paste into a bug report.
    """
    token = os.environ.get("TG_BOT_TOKEN", "").strip()
    if token:
        message = message.replace(token, "***")
    with _emit_lock:
        sys.stdout.write(f"[log] {message}\n")
        sys.stdout.flush()


def telegram_notify(challenge_url: str) -> int | None:
    """Sends the challenge link and returns the update offset to poll from."""
    creds = telegram_credentials()
    if not creds:
        return None
    token, chat_id = creds
    import requests

    text = (
        "SpotiFLAC needs a Cloudflare verification.\n\n"
        f"{challenge_url}\n\n"
        "Complete the check, then reply here with the grant code."
    )
    try:
        requests.post(
            f"{TELEGRAM_API}/bot{token}/sendMessage",
            json={"chat_id": chat_id, "text": text, "disable_web_page_preview": False},
            timeout=15,
        )
        # Start polling past whatever is already in the queue, so an old
        # message never gets mistaken for this challenge's answer.
        resp = requests.get(f"{TELEGRAM_API}/bot{token}/getUpdates", timeout=15)
        updates = resp.json().get("result") or []
        return (updates[-1]["update_id"] + 1) if updates else 0
    except Exception as exc:  # noqa: BLE001 - notification must never be fatal
        write_log(f"telegram notify failed: {exc}")
        return None


def telegram_poll_grant(offset: int) -> tuple[str | None, int]:
    """One non-blocking poll for a reply from the configured chat."""
    creds = telegram_credentials()
    if not creds:
        return None, offset
    token, chat_id = creds
    import requests

    try:
        resp = requests.get(
            f"{TELEGRAM_API}/bot{token}/getUpdates",
            params={"offset": offset, "timeout": 0},
            timeout=15,
        )
        updates = resp.json().get("result") or []
    except Exception:  # noqa: BLE001 - a failed poll is just "nothing yet"
        return None, offset

    for update in updates:
        offset = update["update_id"] + 1
        message = update.get("message") or {}
        # Only the configured chat may supply grants — this is what stops a
        # stranger who finds the bot from hijacking the session.
        if str((message.get("chat") or {}).get("id")) != chat_id:
            continue
        text = (message.get("text") or "").strip()
        if text and not text.startswith("/"):
            return text, offset
    return None, offset


def install_challenge_bridge(bridge: StdinBridge) -> None:
    """Routes the module's manual-verification prompt through our protocol."""
    try:
        from SpotiFLAC.core import signed_session_desktop
    except Exception as exc:  # noqa: BLE001 - older module layout
        write_log(f"challenge bridge unavailable: {exc}")
        return

    def wait_for_grant(challenge_url: str) -> str:
        emit({"type": "cloudflare_challenge", "challenge_url": challenge_url})
        tg_offset = telegram_notify(challenge_url)

        deadline = time.monotonic() + GRANT_WAIT_TIMEOUT_S
        next_tg_poll = 0.0
        while time.monotonic() < deadline:
            if bridge.cancel_event.is_set():
                msg = "verification cancelled"
                raise RuntimeError(msg)
            try:
                return bridge.grants.get(timeout=0.5)
            except queue.Empty:
                pass
            if tg_offset is not None and time.monotonic() >= next_tg_poll:
                grant, tg_offset = telegram_poll_grant(tg_offset)
                next_tg_poll = time.monotonic() + TELEGRAM_POLL_INTERVAL_S
                if grant:
                    return grant
        msg = "no grant code was provided in time"
        raise RuntimeError(msg)

    signed_session_desktop._run_manual_terminal_verification = wait_for_grant  # noqa: SLF001


def track_to_preview_entry(track) -> dict:
    url = track.external_url or (
        f"https://open.spotify.com/track/{track.id}" if track.id else ""
    )
    return {
        "url": url,
        "title": track.title,
        "artist": track.artists,
        "album": track.album,
        "duration_seconds": (track.duration_ms or 0) // 1000 or None,
        "track_number": track.track_number or None,
        "thumbnail_url": track.cover_url or None,
    }


def normalize_kind(raw_kind: str | None, track_count: int) -> str:
    kind = (raw_kind or "").lower()
    if kind in ("artist", "artist_discography"):
        return "artist"
    if kind in ("track", "album", "playlist"):
        return kind
    return "track" if track_count == 1 else "playlist"


async def run_preview(args: argparse.Namespace) -> int:
    from SpotiFLAC import AsyncSpotiFLAC

    with tempfile.TemporaryDirectory(prefix="spotiflac-preview-") as tmp:
        async with AsyncSpotiFLAC(
            output_dir=tmp,
            sync_extensions=False,
            log_level=logging.WARNING,
        ) as client:
            info, tracks = await client.get_playlist(args.url)

    if not tracks:
        emit_error("SPOTIFLAC_NO_SOURCE", f"No tracks resolved for URL: {args.url}")
        return EXIT_ERROR

    kind = normalize_kind(info.get("type"), len(tracks))
    first = tracks[0]
    emit(
        {
            "type": "preview_result",
            "kind": kind,
            "title": info.get("name") or first.title,
            "artist": first.artists,
            "album": first.album or None,
            "thumbnail_url": first.cover_url or None,
            "tracks": [track_to_preview_entry(t) for t in tracks],
        },
    )
    return EXIT_OK


class BroadcastListener:
    """Subscribes to the module's DownloadBroadcaster and re-emits protocol
    events (progress / track_done) from the per-item stats it streams."""

    def __init__(self, watcher: ProviderLogWatcher) -> None:
        self.queue: asyncio.Queue = asyncio.Queue()
        self.watcher = watcher
        self.completed_path: str | None = None
        self.failed_message: str | None = None

    async def attach(self) -> None:
        from SpotiFLAC.core.progress import DownloadBroadcaster

        await DownloadBroadcaster().subscribe(self.queue)

    async def pump(self) -> None:
        while True:
            stats = await self.queue.get()
            items = stats.get("items") or []
            if not items:
                continue
            # One spawn == one track: the last item is the one we care about.
            item = items[-1]
            status = item.get("status")
            progress_mb = float(item.get("progress") or 0.0)
            total_mb = float(item.get("total_size") or 0.0)
            speed_mbps = float(item.get("speed") or 0.0)

            if status == "downloading":
                percent = None
                if total_mb > 0:
                    percent = max(0.0, min(100.0, progress_mb / total_mb * 100.0))
                emit(
                    {
                        "type": "progress",
                        "percent": percent,
                        "downloaded_bytes": int(progress_mb * 1024 * 1024),
                        "speed_bps": int(speed_mbps * 1024 * 1024),
                    },
                )
            elif status == "completed":
                self.completed_path = item.get("file_path") or self.completed_path
            elif status == "failed":
                self.failed_message = item.get("error_message") or "download failed"


async def run_download(args: argparse.Namespace, bridge: StdinBridge) -> int:
    from SpotiFLAC import AsyncSpotiFLAC

    quality = TIER_TO_QUALITY[args.tier]
    services = [s.strip() for s in args.services.split(",") if s.strip()]
    watcher = ProviderLogWatcher()
    install_module_logging(watcher)
    install_challenge_bridge(bridge)

    listener = BroadcastListener(watcher)

    async def watch_cancel() -> None:
        while not bridge.cancel_event.is_set():
            await asyncio.sleep(0.2)
        raise asyncio.CancelledError

    use_extensions = not args.no_extensions_fallback
    async with AsyncSpotiFLAC(
        output_dir=args.output_dir,
        services=services,
        quality=quality,
        allow_fallback=True,
        timeout_s=args.timeout_s,
        sync_extensions=use_extensions,
        use_extensions_fallback=use_extensions,
        log_level=logging.INFO,
    ) as client:
        await listener.attach()
        pump_task = asyncio.create_task(listener.pump())
        cancel_task = asyncio.create_task(watch_cancel())
        download_task = asyncio.create_task(client.download_track(args.url))
        try:
            done, _ = await asyncio.wait(
                {download_task, cancel_task},
                return_when=asyncio.FIRST_COMPLETED,
            )
            if cancel_task in done:
                download_task.cancel()
                with contextlib.suppress(BaseException):
                    await download_task
                return EXIT_CANCELED
            # Propagate any download exception.
            download_task.result()
            # Give the broadcaster a beat to flush the final "completed" event.
            for _ in range(20):
                if listener.completed_path or listener.failed_message:
                    break
                await asyncio.sleep(0.1)
        finally:
            pump_task.cancel()
            cancel_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await pump_task
            with contextlib.suppress(asyncio.CancelledError):
                await cancel_task

    if listener.completed_path:
        emit(
            {
                "type": "track_done",
                "file_path": os.path.abspath(listener.completed_path),
                "provider": watcher.current_provider or (services[0] if services else None),
            },
        )
        return EXIT_OK

    message = listener.failed_message or "no provider delivered the track"
    emit_error(
        "SPOTIFLAC_NO_SOURCE" if "provider" in message.lower() or not listener.failed_message
        else classify_exception(RuntimeError(message)),
        message,
        provider=watcher.current_provider,
    )
    return EXIT_ERROR


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="spotiflac-worker")
    sub = parser.add_subparsers(dest="mode", required=True)

    preview = sub.add_parser("preview", help="resolve metadata without downloading")
    preview.add_argument("--url", required=True)

    download = sub.add_parser("download", help="download exactly one track")
    download.add_argument("--url", required=True)
    download.add_argument("--output-dir", required=True)
    download.add_argument("--services", default="tidal,qobuz,deezer,amazon")
    download.add_argument("--tier", choices=sorted(TIER_TO_QUALITY), default="flac16")
    download.add_argument("--no-extensions-fallback", action="store_true")
    download.add_argument("--timeout-s", type=int, default=None)
    return parser


def main() -> int:
    try:
        args = build_parser().parse_args()
    except SystemExit as exc:
        return EXIT_USAGE if exc.code not in (0, None) else EXIT_OK

    try:
        import SpotiFLAC  # noqa: F401 - import check before emitting hello

        module_version = getattr(SpotiFLAC, "__version__", "unknown")
    except Exception as exc:  # pragma: no cover - broken bundle
        print(f"failed to import SpotiFLAC module: {exc}", file=sys.stderr)
        return EXIT_USAGE

    emit({"type": "hello", "protocol": PROTOCOL_VERSION, "module_version": module_version})

    bridge = StdinBridge()
    bridge.start()

    try:
        if args.mode == "preview":
            return asyncio.run(run_preview(args))
        return asyncio.run(run_download(args, bridge))
    except KeyboardInterrupt:
        return EXIT_CANCELED
    except asyncio.CancelledError:
        return EXIT_CANCELED
    except Exception as exc:  # noqa: BLE001 - single funnel to protocol error
        emit_error(classify_exception(exc), f"{type(exc).__name__}: {exc}")
        return EXIT_ERROR


if __name__ == "__main__":
    sys.exit(main())
