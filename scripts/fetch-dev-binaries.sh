#!/usr/bin/env bash
# Dev-only helper: populate src-tauri/binaries/ so `pnpm tauri dev` can use the
# yt-dlp onedir resource and ffmpeg sidecar locally. Release builds do NOT use
# this script — the CI release pipeline (see .github/workflows/release.yml,
# task T046) fetches pinned, verified binaries for every target platform and
# bundles them into the installer directly, so end users never run this
# script or install anything themselves (FR-018).
set -euo pipefail

REPO_ROOT="$(CDPATH="" cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/binaries"
YTDLP_ONEDIR_DEST="$BIN_DIR/yt-dlp-onedir"
mkdir -p "$BIN_DIR"

TARGET_TRIPLE="$(rustc --print host-tuple)"
echo "Target triple: $TARGET_TRIPLE"

FFMPEG_DEST="$BIN_DIR/ffmpeg-$TARGET_TRIPLE"

# The "onedir" build (an executable next to an already-unpacked `_internal/`
# runtime folder) starts in ~0.3s; the single-file "onefile" build this
# script used to fetch re-extracts its whole bundled Python runtime into a
# fresh temp dir on *every* launch (~14s, measured), which is what made
# preview/download feel slow. `ytdlp_binary::resolve_ytdlp_executable` (the
# Rust side that consumes this folder) expects it unpacked exactly as the
# release zip lays it out, at `src-tauri/binaries/yt-dlp-onedir/`.
case "$(uname -s)" in
  Darwin) YTDLP_ZIP_ASSET="yt-dlp_macos.zip"; YTDLP_EXE_NAME="yt-dlp_macos" ;;
  Linux) YTDLP_ZIP_ASSET="yt-dlp_linux.zip"; YTDLP_EXE_NAME="yt-dlp_linux" ;;
  MINGW*|MSYS*|CYGWIN*) YTDLP_ZIP_ASSET="yt-dlp_win.zip"; YTDLP_EXE_NAME="yt-dlp.exe" ;;
  *)
    echo "Unsupported OS for this dev script: $(uname -s)" >&2
    exit 1
    ;;
esac

if [[ ! -x "$YTDLP_ONEDIR_DEST/$YTDLP_EXE_NAME" ]]; then
  echo "Downloading yt-dlp onedir build ($YTDLP_ZIP_ASSET) ..."
  TMP_ZIP="$(mktemp -t yt-dlp-onedir.XXXXXX.zip)"
  curl -fL "https://github.com/yt-dlp/yt-dlp/releases/latest/download/$YTDLP_ZIP_ASSET" -o "$TMP_ZIP"
  rm -rf "$YTDLP_ONEDIR_DEST"
  mkdir -p "$YTDLP_ONEDIR_DEST"
  unzip -q "$TMP_ZIP" -d "$YTDLP_ONEDIR_DEST"
  rm -f "$TMP_ZIP"
  chmod +x "$YTDLP_ONEDIR_DEST/$YTDLP_EXE_NAME"
else
  echo "yt-dlp onedir build already present at $YTDLP_ONEDIR_DEST"
fi

GALLERY_DL_ONEDIR_DEST="$BIN_DIR/gallery-dl-onedir"
GALLERY_DL_EXE_NAME="gallery-dl"
if [[ "$(uname -s)" =~ ^(MINGW|MSYS|CYGWIN) ]]; then
  GALLERY_DL_EXE_NAME="gallery-dl.exe"
fi

# No official standalone gallery-dl build exists for macOS, and the official
# Windows/Linux ones are PyInstaller `--onefile` (same slow-relaunch problem
# yt-dlp had) — so this is always a local PyInstaller `--onedir` build rather
# than a download. See scripts/build-gallery-dl-onedir.sh for the full
# rationale; requires python3 + pip (both already needed for nothing else in
# this project, only for this build step).
if [[ ! -x "$GALLERY_DL_ONEDIR_DEST/$GALLERY_DL_EXE_NAME" ]]; then
  echo "Building gallery-dl onedir locally (first run only; needs python3+pip)..."
  bash "$REPO_ROOT/scripts/build-gallery-dl-onedir.sh"
else
  echo "gallery-dl onedir build already present at $GALLERY_DL_ONEDIR_DEST"
fi

if [[ ! -x "$FFMPEG_DEST" ]]; then
  if command -v ffmpeg >/dev/null 2>&1; then
    echo "Using system ffmpeg found at $(command -v ffmpeg) for local dev (release builds bundle a proper static build instead)"
    cp "$(command -v ffmpeg)" "$FFMPEG_DEST"
    chmod +x "$FFMPEG_DEST"
  else
    cat >&2 <<'EOF'
No local ffmpeg found and this dev script does not download one automatically
(third-party static-build URLs vary and go stale). Install ffmpeg locally for
development, e.g.:
  macOS:   brew install ffmpeg
  Linux:   sudo apt install ffmpeg   (or your distro's equivalent)
  Windows: winget install ffmpeg

Then re-run this script. This only affects local `tauri dev` — the release
CI pipeline (T046) fetches a pinned static ffmpeg build for every platform and
bundles it into the installer, so end users never need to do this (FR-018).
EOF
    exit 1
  fi
else
  echo "ffmpeg sidecar already present at $FFMPEG_DEST"
fi

echo "Dev binaries ready in $BIN_DIR"
