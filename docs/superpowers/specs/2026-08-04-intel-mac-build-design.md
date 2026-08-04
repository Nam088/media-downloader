# Intel Mac (x86_64-apple-darwin) build support

> **Superseded during implementation (2026-08-04):** this spec's design used
> `macos-13` as a genuine Intel runner. That plan hit reality during Task 7's
> live CI validation: `macos-13` was retired by GitHub on 2025-12-04 (its
> replacement, `macos-15-intel`, only lasts until 2027-08 before GitHub drops
> Intel macOS runners entirely, per Apple ending Intel support). The build
> was reworked to cross-compile `x86_64-apple-darwin` from the same
> `macos-latest` (Apple Silicon) runner via Rosetta 2, which has no
> expiration tied to GitHub's runner lineup. Every mention of `macos-13`
> below is the original (now-abandoned) design — kept for the record of what
> was tried and why it didn't survive contact with real CI, not as current
> instructions. See `.github/workflows/release.yml`'s own comments for the
> actual current design.

## Problem

Release CI (`.github/workflows/release.yml`) only builds macOS for
`aarch64-apple-darwin`. The Homebrew Cask (`Casks/media-downloader.rb`)
enforces `depends_on arch: :arm64` and fails outright on Intel Macs, and
`update-cask.yml` only ever looks for one macOS asset per release. Intel Mac
users cannot install the app via the Cask, and there is no built artifact for
them at all.

## Goals

- CI builds and publishes a native `x86_64-apple-darwin` `.dmg`/`.app`
  alongside the existing `aarch64-apple-darwin` build, for every tagged
  release.
- The Homebrew Cask installs the correct build for the Mac's actual
  architecture (Intel or Apple Silicon), with no manual step.
- The Intel leg is verified as a real, runnable native build in CI, not just
  a cross-compiled artifact nobody has executed.

## Non-goals

- No universal (fat) binary. Two separate builds, matching the existing
  arm64-only approach.
- No change to Windows/Linux matrix legs.
- No physical Intel Mac test by the author (none available) — CI-native
  execution on `macos-13` stands in for that, plus `brew audit`/`brew style`
  for the Cask DSL. A real `brew install --cask` on physical Intel hardware
  is called out as a follow-up manual check after the first release ships.

## Design

### 1. `release.yml` — new macOS matrix leg

Add a second macOS entry to `jobs.build.strategy.matrix.include`:

- `platform: macos-13` — a genuine Intel GitHub-hosted runner (not
  `macos-latest`, which is Apple Silicon). Native build, no cross-compilation
  and no Rosetta needed to run anything mid-build.
- `target: x86_64-apple-darwin`
- `ytdlp_zip_asset: yt-dlp_macos.zip`, `ytdlp_exe_name: yt-dlp_macos` — same
  as the arm64 leg. Confirmed via the yt-dlp release API that this asset is
  the only macOS build yt-dlp ships (universal2), so it's already correct for
  both architectures.
- `ffmpeg_dest_name: ffmpeg-x86_64-apple-darwin`
- New matrix field `ffmpeg_arch: amd64` (arm64 leg gets `ffmpeg_arch: arm64`)
  — selects the martin-riedl.de URL path segment. Confirmed by direct
  download: `https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip`
  resolves (the `x64` segment does not — 404), and the resulting binary is
  genuine `x86_64` (`lipo -archs`) and self-contained (`otool -L` only lists
  `/System/` and `/usr/lib/` paths, same as the existing arm64 build).

The "Fetch ffmpeg sidecar (macOS)" step's URL becomes
`.../macos/${{ matrix.ffmpeg_arch }}/release/ffmpeg.zip` instead of the
hardcoded `arm64`.

The "Verify the ffmpeg sidecar is self-contained" step's `lipo -archs`
comparison changes from the hardcoded `"arm64"` to a value derived from
`matrix.target` (`x86_64-apple-darwin` → `x86_64`, `aarch64-apple-darwin` →
`arm64`), so it correctly gates both legs instead of only ever checking for
arm64.

The gallery-dl onedir build script and the artifact upload step need no
changes: the build script has no arch-specific logic (confirmed by
inspection), and the upload step already parameterizes paths by
`matrix.target`, so the two legs naturally produce separately named
artifacts (`media-downloader-x86_64-apple-darwin`,
`media-downloader-aarch64-apple-darwin`).

### 2. Smoke-test step (new, macOS only)

After the build step, add a step that runs on both macOS legs: execute the
bundled yt-dlp and gallery-dl onedir binaries with `--version` (or
equivalent) directly on the runner. Because `macos-13` is genuinely Intel,
this proves the x86_64 bundle actually runs natively in CI, not merely that
it was produced. Fail the job if either invocation errors.

### 3. `Casks/media-downloader.rb` — per-arch url/sha256

Replace the single `url`/`sha256` pair and `depends_on arch: :arm64` with
Homebrew's standard per-architecture DSL:

```ruby
# CASK_ARCH_URLS_START
on_arm do
  url "..."
  sha256 "..."
end

on_intel do
  url "..."
  sha256 "..."
end
# CASK_ARCH_URLS_END
```

The `CASK_ARCH_URLS_START`/`END` marker comments exist so `update-cask.yml`
can rewrite this block wholesale (it currently does single-line `sed`
replacement, which doesn't work once the file has two url/sha256 pairs
inside conditional blocks).

### 4. `update-cask.yml` — publish both assets

- Look up both release assets: the existing `aarch64.dmg` suffix match, plus
  a second match for the Intel asset. The exact suffix Tauri gives the
  `x86_64-apple-darwin` bundle is unknown until the first CI build runs;
  confirm it from the actual release artifact list before writing the
  match pattern (expected to be something like `_x64.dmg`, needs
  verification against the real filename, not assumed).
- Fail loudly (as today) if either asset is missing — a release with only
  one macOS arch built should not silently publish a Cask that's broken for
  the other arch.
- Download both, compute both sha256 checksums.
- Rewrite the file between the `CASK_ARCH_URLS_START`/`END` markers with
  fresh `on_arm`/`on_intel` blocks (e.g. via `perl -0777 -pi -e` or an
  equivalent block replace), rather than the current per-line `sed`.

### 5. Verification / testing plan

- CI (automatic, every release build): `lipo -archs` + `otool -L` gate on
  both macOS legs (extended from arm64-only today); the new smoke-test step
  actually executing both onedir binaries natively on `macos-13`.
- Pre-merge (manual, once): `brew audit --cask ./Casks/media-downloader.rb`
  and `brew style` locally against the edited Cask file to catch DSL syntax
  errors before they reach a real release.
- Post-first-release (manual, one-time): confirm the real Intel `.dmg` asset
  filename matches what `update-cask.yml` expects; ask someone with a
  physical Intel Mac to `brew install --cask media-downloader` since no
  Intel hardware is available to the author directly.

## Risks / open questions

- `macos-13` is an older GitHub-hosted image; GitHub has signaled eventual
  deprecation of Intel macOS runners industry-wide. When that happens this
  leg will need a different Intel-capable runner or fall back to
  cross-compilation + `otool`/`lipo`-only verification (no native smoke
  test). Not a blocker today.
- The Intel `.dmg` asset's exact filename/suffix from Tauri is unverified
  until the first CI run produces it — `update-cask.yml`'s asset-matching
  pattern must be checked against the real name before relying on it.
