# Building the Windows `.exe`

Windows builds cannot be produced on macOS or Linux. Three independent reasons,
and fixing one does not help with the others:

1. **Tauri does not support cross-compiling to Windows.** The target needs the
   MSVC toolchain and the WebView2 loader, and the NSIS/WiX installers only run
   on Windows.
2. **The Rust target is not installed** on the mac (`aarch64-apple-darwin`
   only). Easy to add, but irrelevant given point 1.
3. **`yt-dlp` and `gallery-dl` ship as PyInstaller "onedir" bundles**, which
   embed the Python runtime *of the machine that built them*. A Windows bundle
   can only be produced on Windows. This is the one that cannot be worked
   around.

So: build on a Windows machine, or let `.github/workflows/release.yml` do it on
a `windows-latest` runner. This document covers the first.

## Prerequisites

| Tool | Notes |
|---|---|
| **Visual Studio Build Tools** | "Desktop development with C++" workload. Supplies the MSVC linker Rust needs. Rust will not link without it. |
| **Rust** | <https://rustup.rs> — the default `x86_64-pc-windows-msvc` toolchain is correct. |
| **Node.js 20+** and **pnpm** | `npm i -g pnpm` |
| **Python 3** | Needed to build the `gallery-dl` onedir bundle. |
| **Git for Windows** | Provides **Git Bash**, which the two setup scripts require — they are `bash` scripts and already branch on `MINGW*/MSYS*/CYGWIN*`. |
| **WebView2 Runtime** | Preinstalled on Windows 11 and current Windows 10. Only install manually if the built app refuses to open a window. |

## Getting the code across

There is no git remote on this repository. To move it with full history and
without publishing anything, use a bundle — one file, copy it by USB or network
share:

```bash
# on the mac
git bundle create media-downloader.bundle --all
```

```powershell
# on Windows, after copying the file across
git clone media-downloader.bundle media-downloader
cd media-downloader
git checkout feature/download-power-phase1
```

Copying the working folder directly also works, but delete `node_modules/` and
`src-tauri/target/` first — they contain macOS binaries and are large.

## Build steps

Run everything below in **Git Bash**, not PowerShell — the scripts are `bash`.

```bash
pnpm install

# yt-dlp onedir + ffmpeg sidecar.
# Picks yt-dlp_win.zip automatically from `uname -s`.
bash scripts/fetch-dev-binaries.sh

# gallery-dl onedir. On Windows the interpreter is usually `python`, not
# `python3`, which is what the script defaults to.
PYTHON_BIN=python bash scripts/build-gallery-dl-onedir.sh

pnpm tauri build
```

Output lands in `src-tauri/target/release/bundle/` — an `.msi` and an
`.exe` (NSIS) installer.

### The ffmpeg caveat — read this before shipping

`fetch-dev-binaries.sh` copies **the system ffmpeg** if it finds one. That is
fine for local development but wrong for a release: a system ffmpeg is
dynamically linked and will not run on a machine that lacks its DLLs, which
silently breaks the FR-018 promise that one installer is enough.

For a build you intend to give to someone else, fetch the pinned static build
the CI pipeline uses instead:

```bash
curl -fL "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip" -o ffmpeg.zip
unzip -q ffmpeg.zip
cp ffmpeg-*-win64-gpl/bin/ffmpeg.exe src-tauri/binaries/ffmpeg-x86_64-pc-windows-msvc.exe
```

The sidecar filename must end in the exact target triple —
`ffmpeg-x86_64-pc-windows-msvc.exe` — or Tauri will not find it at runtime and
every download that needs post-processing fails.

## Verifying the build

```bash
cd src-tauri && cargo test     # 300 tests
cd .. && pnpm test             # 422 tests
pnpm exec tsc --noEmit -p tsconfig.json
```

Then run the built installer and check the things no test covers:

1. **The Content Security Policy is only applied in a bundled build** —
   `tauri dev` loads the page from Vite and applies no CSP at all. Open the
   webview devtools and confirm there are no CSP violations. The failure that
   matters is `connect-src`: Tauri's IPC goes over `http://ipc.localhost` on
   Windows, and if that is blocked then every `invoke()` fails and the whole
   app is dead, not just one feature.
2. **Toast styling** — `sonner` injects a `<style>` element at runtime, which
   is why `style-src` carries `'unsafe-inline'`.
3. **Thumbnails** from source CDNs still load (`img-src` allows `https:`; plain
   `http:` is deliberately blocked).
4. **Media playback** in the Library tab, and the "open in default app"
   fallback for a format the webview cannot decode.
5. **Downloading actually works** — that exercises all three bundled sidecars.

## Code signing

Windows will show a SmartScreen warning for an unsigned executable. Signing
needs a code-signing certificate and is configured under `bundle.windows` in
`src-tauri/tauri.conf.json`. The macOS build has the same issue: it is ad-hoc
signed, so the `.dmg` is not distributable without an Apple Developer ID and
notarization.

## The alternative: let CI do it

`.github/workflows/release.yml` already builds all three platforms and does the
Windows part correctly — it fetches the pinned static ffmpeg and builds the
`gallery-dl` onedir bundle on the Windows runner itself. It has never run,
because the repository has no remote. Pushing to GitHub and tagging a release
would produce the `.exe`, `.dmg` and `.AppImage` without needing a Windows
machine at all.
