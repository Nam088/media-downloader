#!/usr/bin/env bash
# Builds a standalone "onedir" spotiflac-worker (executable + pre-unpacked
# Python runtime folder) via PyInstaller, and places the result at
# src-tauri/binaries/spotiflac-onedir/ — the exact layout
# downloader::spotiflac_binary::resolve_spotiflac_executable expects.
#
# SpotiFLAC publishes official standalone executables, but they speak
# human-oriented CLI output with no structured progress and no stdin channel
# for Cloudflare grant injection. This project instead bundles its own thin
# worker (scripts/spotiflac_worker.py) around the pip module, speaking the
# JSON-line protocol documented in
# specs/006-spotiflac-integration/contracts/spotiflac-worker-protocol.md —
# same approach as the gallery-dl onedir build next to this script.
#
# Must be run once per target OS (PyInstaller cannot cross-compile) — see
# .github/workflows/release.yml for the per-platform CI invocation.
set -euo pipefail

REPO_ROOT="$(CDPATH="" cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src-tauri/binaries/spotiflac-onedir"
BUILD_ROOT="$(mktemp -d -t spotiflac-build.XXXXXX)"
trap 'rm -rf "$BUILD_ROOT"' EXIT

# Pinned module version. The worker protocol (hello.protocol == 1) is written
# against this release; when bumping, re-verify the DownloadBroadcaster stats
# shape and the provider log tags spotiflac_worker.py relies on, then update
# the pin here and PROTOCOL notes in the contract if anything changed.
SPOTIFLAC_VERSION="${SPOTIFLAC_VERSION:-1.5.5}"

echo "Building spotiflac-worker onedir (SpotiFLAC $SPOTIFLAC_VERSION, target: $(uname -s))..."

PYTHON_BIN="${PYTHON_BIN:-python3}"
"$PYTHON_BIN" -m venv "$BUILD_ROOT/venv"

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*) VENV_BIN="$BUILD_ROOT/venv/Scripts" ;;
  *) VENV_BIN="$BUILD_ROOT/venv/bin" ;;
esac

"$VENV_BIN/pip" install --quiet --upgrade pip
# `nodriver` is installed explicitly on purpose: SpotiFLAC's own
# requirements.txt lists nodriver>=0.36, but its published wheel metadata
# omits it, so `pip install SpotiFLAC` alone leaves it out — and
# SpotiFLAC/core/solver.py imports it at module scope, which makes a plain
# `import SpotiFLAC` fail outright. It is also what the automated Cloudflare
# solver runs on (research.md R4, layer 1), so it is a real dependency here,
# not just an import-time formality.
"$VENV_BIN/pip" install --quiet "SpotiFLAC==$SPOTIFLAC_VERSION" "nodriver>=0.36" pyinstaller

# SpotiFLAC imports its providers/extensions dynamically in places; collect
# the whole package (code + any data files) so PyInstaller's static analysis
# can't miss submodules.
"$VENV_BIN/pyinstaller" \
  --onedir --console --name spotiflac-worker \
  --collect-all SpotiFLAC \
  --collect-all nodriver \
  --exclude-module pkg_resources \
  --distpath "$BUILD_ROOT/dist" \
  --workpath "$BUILD_ROOT/build" \
  --specpath "$BUILD_ROOT/build" \
  "$REPO_ROOT/scripts/spotiflac_worker.py"

rm -rf "$DEST_DIR"
mkdir -p "$(dirname "$DEST_DIR")"
cp -R "$BUILD_ROOT/dist/spotiflac-worker" "$DEST_DIR"

echo "spotiflac-worker onedir build ready at $DEST_DIR"
