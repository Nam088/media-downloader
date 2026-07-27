# Media Downloader

*[Tiếng Việt](README.vi.md)*

A desktop app for downloading video, audio, and lossless music from a wide range of platforms — runs on Windows, macOS, and Linux, ships every tool it needs, and requires no Python, Node.js, or other dependency from the user.

Built on [Tauri 2](https://tauri.app/) (Rust + React), using [yt-dlp](https://github.com/yt-dlp/yt-dlp) and [gallery-dl](https://github.com/mikf/gallery-dl) as the video/image download engines, and [SpotiFLAC](https://github.com/BartolomeoRusso9/SpotiFLAC-Module-Version) as the lossless music engine.

## Features

### Video & audio downloads
- Paste one or many links at once (typed in or imported from a list file), with automatic platform detection and a preview (title, thumbnail, duration) before anything downloads.
- Supports YouTube, TikTok, Facebook, Instagram, X (Twitter), SoundCloud, and roughly 1,600 other sites yt-dlp knows about, with automatic fallback to gallery-dl for image/gallery posts (Pixiv, Reddit, TikTok slideshows...) that yt-dlp can't read.
- Choose audio-only or full video; the quality list always comes straight from the source's real available formats — never a fabricated option that doesn't actually exist.
- Download entire playlists, or pick individual videos with their own quality per video.

### A queue that holds up
- A real, disk-backed queue — survives app restarts, never loses a pending job.
- Drag-and-drop reordering, adjustable concurrent download count, pause/resume/cancel per job or in bulk.
- Automatic retry on transient network errors with increasing backoff, clearly separated from permanent failures (private content, removed videos) so those are never retried pointlessly.
- Download rate limiting, background mode with a system tray icon, and notifications when a job finishes.

### Flexible output
- Multiple audio formats (MP3, M4A, Opus, WAV, FLAC) and video formats (MP4, MKV, or keep the source's original container), with codec preference tuned for compatibility or quality.
- Automatic metadata (title, artist, album) and cover art embedding wherever the output format supports it.
- Custom filename templates, subtitle downloads, trimming a specific segment, and splitting a video into per-chapter files.
- Save a full output configuration as a reusable preset.

### Lossless music — SpotiFLAC
- Paste a Spotify, Tidal, Apple Music, or Pandora link to get the track as **genuine lossless FLAC** (16-bit or 24-bit Hi-Res) sourced from Tidal, Qobuz, Deezer, or Amazon Music — no account required for any of them.
- Automatically tries providers in a user-configured priority order, with JS-extension fallback when the primary source has trouble (requires Node.js).
- An MP3 320kbps tier is available too (downloads the lossless source, then transcodes with the bundled ffmpeg).
- Handles Cloudflare verification gracefully: an in-app dialog for entering the verification code, plus optional Telegram Bot notifications for remote verification.
- Albums, playlists, and artist discographies are split into independent per-track downloads in the same queue.

### Library
- Every downloaded file is indexed automatically — browse as a grid or list, search and filter by content type, platform, or format.
- Play or preview right inside the app; rename, delete, or reveal a file's folder without leaving the app.
- Automatically detects files removed outside the app, gives an overview of total size and download counts, and exports M3U playlists.

### Cross-platform, multilingual
- One consistent feature set on Windows, macOS, and Linux — nothing is platform-exclusive.
- Vietnamese and English UI, switchable from within the app.

## Installation

Grab the installer for your OS from the [Releases](../../releases) page — every release ships:

| OS | Formats |
|---|---|
| Windows | `.msi`, `.exe` (NSIS) |
| macOS (Apple Silicon) | `.dmg` |
| Linux | `.deb`, `.rpm`, `.AppImage` |

No need to separately install Python, Node.js, or ffmpeg — everything is bundled in the installer.

> **macOS**: since the app isn't signed with an Apple Developer certificate, the first launch may show "is damaged and can't be opened" from Gatekeeper. Run `xattr -cr "/Applications/Media Downloader.app"` to clear the quarantine flag, then open it again.

## Development

Requires: Node.js 22+, pnpm, Rust stable, Python 3 (build-time only, for the sidecar tools).

```bash
pnpm install
bash scripts/fetch-dev-binaries.sh   # fetches/builds yt-dlp, gallery-dl, spotiflac-worker, ffmpeg for local dev
pnpm tauri dev
```

Tests:

```bash
pnpm test              # frontend (vitest)
cd src-tauri && cargo test   # backend (Rust)
```

## Architecture

- **Frontend**: React 19 + TypeScript, Zustand for state, i18next for localization.
- **Backend**: Rust (Tauri 2), SQLite (rusqlite) for the queue, history, library, and settings.
- **Download engines**: three engines run side by side depending on content type — `yt-dlp` (general video/audio), `gallery-dl` (images/galleries), `spotiflac-worker` (lossless music, a thin wrapper this project wrote around the SpotiFLAC Python module).
- Each Python-based tool ships as a PyInstaller onedir build and is unpacked into the app's data directory on first run — no Python installation required on the user's machine.

Detailed design docs for each feature live under [`specs/`](specs/), following a spec-driven development approach.
