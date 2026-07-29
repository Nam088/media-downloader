#!/usr/bin/env bash
# Builds a standalone "onedir" gallery-dl (executable + pre-unpacked Python
# runtime folder) via PyInstaller, and places the result at
# src-tauri/binaries/gallery-dl-onedir/ — the exact layout
# downloader::gallery_dl_binary::resolve_gallery_dl_executable expects.
#
# gallery-dl only publishes official standalone binaries for Windows/Linux,
# built with PyInstaller's `--onefile` (see mikf/gallery-dl's own
# scripts/pyinstaller.py) — onefile re-extracts its whole bundled Python
# runtime into a fresh temp dir on every single launch, the same performance
# bug this project already fixed for yt-dlp. There is also no official macOS
# build at all. Building our own `--onedir` binary for every platform, using
# gallery-dl's own official PyInstaller hook (vendored at
# scripts/pyinstaller-hooks/hook-gallery_dl.py — REQUIRED, since gallery-dl's
# ~282 extractor modules are imported dynamically and PyInstaller's static
# analysis can't discover them on its own), sidesteps both problems: same
# ~0.3s-after-first-run startup as yt-dlp's onedir build, on all 3 platforms,
# with no dependency on an official binary that doesn't exist for one of them.
#
# Must be run once per target OS (PyInstaller cannot cross-compile) — see
# .github/workflows/release.yml for the per-platform CI invocation.
set -euo pipefail

REPO_ROOT="$(CDPATH="" cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src-tauri/binaries/gallery-dl-onedir"
BUILD_ROOT="$(mktemp -d -t gallery-dl-build.XXXXXX)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

GALLERY_DL_VERSION="${GALLERY_DL_VERSION:-1.32.8}"

echo "Building gallery-dl $GALLERY_DL_VERSION onedir binary (target: $(uname -s))..."

PYTHON_BIN="${PYTHON_BIN:-python3}"
"$PYTHON_BIN" -m venv "$BUILD_ROOT/venv"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) VENV_BIN="$BUILD_ROOT/venv/Scripts" ;;
  *) VENV_BIN="$BUILD_ROOT/venv/bin" ;;
esac

# `python -m pip`, never `pip` directly: on Windows the console script is
# pip.exe, and Windows will not let a running executable replace itself, so
# `pip install --upgrade pip` fails outright with "To modify pip, please run
# the following command". It works on macOS and Linux, which is exactly why
# this only ever broke on the Windows CI runner.
"$VENV_BIN/python" -m pip install --quiet --upgrade pip
"$VENV_BIN/python" -m pip install --quiet "gallery-dl==$GALLERY_DL_VERSION" pyinstaller

GALLERY_DL_PKG_DIR="$("$VENV_BIN/python" -c 'import gallery_dl, os; print(os.path.dirname(gallery_dl.__file__))')"

"$VENV_BIN/pyinstaller" \
  --onedir --windowed --name gallery-dl \
  --exclude-module pkg_resources \
  --additional-hooks-dir "$REPO_ROOT/scripts/pyinstaller-hooks" \
  --distpath "$BUILD_ROOT/dist" \
  --workpath "$BUILD_ROOT/build" \
  --specpath "$BUILD_ROOT/build" \
  "$GALLERY_DL_PKG_DIR/__main__.py"

rm -rf "$DEST_DIR"
mkdir -p "$(dirname "$DEST_DIR")"
cp -R "$BUILD_ROOT/dist/gallery-dl" "$DEST_DIR"

echo "gallery-dl onedir build ready at $DEST_DIR"
