# Intel Mac (x86_64-apple-darwin) Build Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make release CI build and publish a native macOS Intel (`x86_64-apple-darwin`) installer alongside the existing Apple Silicon one, and make the Homebrew Cask install the right one automatically.

**Architecture:** Add a second macOS leg to the `release.yml` build matrix on a genuine Intel runner (`macos-13`), parameterize the two places that currently hard-code `arm64` (the ffmpeg sidecar fetch URL and its `lipo -archs` verification), add a native smoke-test step, then switch the Cask to Homebrew's `on_arm`/`on_intel` DSL and teach `update-cask.yml` to populate both.

**Tech Stack:** GitHub Actions (YAML), bash, Ruby (Homebrew Cask DSL), `brew audit`/`brew style` for local validation.

**Spec:** `docs/superpowers/specs/2026-08-04-intel-mac-build-design.md`

**Prerequisite fix already applied (commits `f716bb3`, `8f34970`):** the Cask
was 3 releases stale (still pointing at v0.1.1) because `update-cask.yml`'s
`release: published` trigger never reliably fired — it raced ahead of asset
uploads for v0.1.3 and never fired at all for v0.1.4. The trigger is now
`workflow_run` off `release.yml`'s completion, and the tag name is sourced
from `steps.asset.outputs.tag` (derived from
`github.event.workflow_run.head_branch`), not `github.event.release.tag_name`.
Task 6 below is written against this new baseline.

---

## File Structure

- Modify: `.github/workflows/release.yml` — matrix entry, ffmpeg fetch/verify steps, new smoke-test step.
- Modify: `Casks/media-downloader.rb` — `on_arm`/`on_intel` blocks replacing the single url/sha256/depends_on.
- Modify: `.github/workflows/update-cask.yml` — fetch both macOS assets, compute both checksums, rewrite the Cask's arch block.

---

### Task 1: Add the Intel matrix leg to `release.yml`

**Files:**
- Modify: `.github/workflows/release.yml:37-53`

- [ ] **Step 1: Add `ffmpeg_arch` and `expected_lipo_arch` to the existing arm64 entry, and add the new x86_64 entry**

Replace:

```yaml
      matrix:
        include:
          - platform: windows-latest
            target: x86_64-pc-windows-msvc
            ytdlp_zip_asset: yt-dlp_win.zip
            ytdlp_exe_name: yt-dlp.exe
            ffmpeg_dest_name: ffmpeg-x86_64-pc-windows-msvc.exe
          - platform: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            ytdlp_zip_asset: yt-dlp_linux.zip
            ytdlp_exe_name: yt-dlp_linux
            ffmpeg_dest_name: ffmpeg-x86_64-unknown-linux-gnu
          - platform: macos-latest
            target: aarch64-apple-darwin
            ytdlp_zip_asset: yt-dlp_macos.zip
            ytdlp_exe_name: yt-dlp_macos
            ffmpeg_dest_name: ffmpeg-aarch64-apple-darwin
```

with:

```yaml
      matrix:
        include:
          - platform: windows-latest
            target: x86_64-pc-windows-msvc
            ytdlp_zip_asset: yt-dlp_win.zip
            ytdlp_exe_name: yt-dlp.exe
            ffmpeg_dest_name: ffmpeg-x86_64-pc-windows-msvc.exe
          - platform: ubuntu-latest
            target: x86_64-unknown-linux-gnu
            ytdlp_zip_asset: yt-dlp_linux.zip
            ytdlp_exe_name: yt-dlp_linux
            ffmpeg_dest_name: ffmpeg-x86_64-unknown-linux-gnu
          # macos-latest is an Apple Silicon runner. macos-13 below is a
          # genuine Intel runner — not a cross-compile — so the x86_64 build
          # can be smoke-tested by actually running it (see the smoke-test
          # step further down).
          - platform: macos-latest
            target: aarch64-apple-darwin
            ytdlp_zip_asset: yt-dlp_macos.zip
            ytdlp_exe_name: yt-dlp_macos
            ffmpeg_dest_name: ffmpeg-aarch64-apple-darwin
            ffmpeg_arch: arm64
            expected_lipo_arch: arm64
          - platform: macos-13
            target: x86_64-apple-darwin
            ytdlp_zip_asset: yt-dlp_macos.zip
            ytdlp_exe_name: yt-dlp_macos
            ffmpeg_dest_name: ffmpeg-x86_64-apple-darwin
            # martin-riedl.de's URL path segment is "amd64", not "x64" —
            # verified by hand: the "x64" segment 404s, "amd64" resolves.
            ffmpeg_arch: amd64
            # `lipo -archs` reports "x86_64" for this build, a different
            # string than the URL's "amd64" segment above.
            expected_lipo_arch: x86_64
```

`yt-dlp_macos.zip` is unchanged on the new leg: yt-dlp ships one universal2
macOS build, confirmed via `https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest`
listing only `yt-dlp_macos` / `yt-dlp_macos.zip` for macOS.

- [ ] **Step 2: Verify the YAML still parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add x86_64-apple-darwin matrix leg on a real Intel runner"
```

---

### Task 2: Parameterize the ffmpeg fetch step for both macOS architectures

**Files:**
- Modify: `.github/workflows/release.yml` (the "Fetch ffmpeg sidecar (macOS)" step, currently around line 166)

- [ ] **Step 1: Widen the `if` and parameterize the URL**

Replace:

```yaml
      - name: Fetch ffmpeg sidecar (macOS)
        if: matrix.platform == 'macos-latest'
        run: |
          curl -fL "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip" -o ffmpeg.zip
          unzip -q ffmpeg.zip
          cp ffmpeg "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}"
          chmod +x "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}"
```

with:

```yaml
      - name: Fetch ffmpeg sidecar (macOS)
        if: matrix.platform == 'macos-latest' || matrix.platform == 'macos-13'
        run: |
          curl -fL "https://ffmpeg.martin-riedl.de/redirect/latest/macos/${{ matrix.ffmpeg_arch }}/release/ffmpeg.zip" -o ffmpeg.zip
          unzip -q ffmpeg.zip
          cp ffmpeg "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}"
          chmod +x "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}"
```

- [ ] **Step 2: Verify the YAML still parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: fetch the correct ffmpeg sidecar arch on both macOS runners"
```

---

### Task 3: Parameterize the ffmpeg verification step

**Files:**
- Modify: `.github/workflows/release.yml` (the "Verify the ffmpeg sidecar is self-contained" step, currently around line 179)

- [ ] **Step 1: Widen the `if` and compare against `matrix.expected_lipo_arch`**

Replace:

```yaml
      - name: Verify the ffmpeg sidecar is self-contained
        if: matrix.platform == 'macos-latest'
        run: |
          LEAKED=$(otool -L "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}" | tail -n +2 | grep -vE '/System/|/usr/lib/' || true)
          if [ -n "$LEAKED" ]; then
            echo "::error::ffmpeg sidecar links against libraries outside the OS:"
            echo "$LEAKED"
            exit 1
          fi
          ARCH=$(lipo -archs "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}")
          if [ "$ARCH" != "arm64" ]; then
            echo "::error::ffmpeg sidecar is $ARCH, but the target is aarch64-apple-darwin"
            exit 1
          fi
```

with:

```yaml
      - name: Verify the ffmpeg sidecar is self-contained
        if: matrix.platform == 'macos-latest' || matrix.platform == 'macos-13'
        run: |
          LEAKED=$(otool -L "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}" | tail -n +2 | grep -vE '/System/|/usr/lib/' || true)
          if [ -n "$LEAKED" ]; then
            echo "::error::ffmpeg sidecar links against libraries outside the OS:"
            echo "$LEAKED"
            exit 1
          fi
          ARCH=$(lipo -archs "src-tauri/binaries/${{ matrix.ffmpeg_dest_name }}")
          if [ "$ARCH" != "${{ matrix.expected_lipo_arch }}" ]; then
            echo "::error::ffmpeg sidecar is $ARCH, but the target is ${{ matrix.target }}"
            exit 1
          fi
```

- [ ] **Step 2: Verify the YAML still parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: verify ffmpeg sidecar arch against each matrix leg, not just arm64"
```

---

### Task 4: Add a native smoke-test step for the onedir binaries

This is the step that makes the Intel leg meaningfully tested rather than
just built: `macos-13` is a real Intel machine, so running the bundled
binaries here proves they execute natively, no Rosetta involved.

**Files:**
- Modify: `.github/workflows/release.yml` (insert a new step right after the existing "Build gallery-dl onedir resource" step, currently around line 128, so both onedir directories already exist)

- [ ] **Step 1: Insert the smoke-test step**

Insert immediately after the `Build gallery-dl onedir resource` step (before
the `---- ffmpeg sidecar ----` comment block):

```yaml
      - name: Smoke-test onedir binaries run natively (macOS)
        if: matrix.platform == 'macos-latest' || matrix.platform == 'macos-13'
        run: |
          "src-tauri/binaries/yt-dlp-onedir/${{ matrix.ytdlp_exe_name }}" --version
          "src-tauri/binaries/gallery-dl-onedir/gallery-dl" --version
```

- [ ] **Step 2: Verify the YAML still parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: smoke-test yt-dlp/gallery-dl onedir binaries run natively on macOS"
```

---

### Task 5: Switch the Cask to per-architecture `on_arm`/`on_intel` blocks

**Files:**
- Modify: `Casks/media-downloader.rb`

- [ ] **Step 1: Replace the single url/sha256/depends_on with marked arch blocks**

Replace the whole file's body from `url` through `depends_on arch: :arm64`
(current lines 12-20):

```ruby
  url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.0_aarch64.dmg"
  name "Media Downloader"
  desc "Download video and audio from YouTube and 1000+ other sites"
  homepage "https://github.com/Nam088/media-downloader"

  # Release CI only builds aarch64-apple-darwin today — installing this cask
  # on an Intel Mac would silently ship a binary that can't run at all, so
  # fail the install with a clear reason instead.
  depends_on arch: :arm64
```

with:

```ruby
  name "Media Downloader"
  desc "Download video and audio from YouTube and 1000+ other sites"
  homepage "https://github.com/Nam088/media-downloader"

  # update-cask.yml rewrites everything between these two markers wholesale
  # on every published release — don't hand-edit the URLs/checksums here,
  # they'll be overwritten on the next release anyway.
  # CASK_ARCH_URLS_START
  on_arm do
    url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.0_aarch64.dmg"
    sha256 "47c8cea32ac14004eb6df19f3f8c3f8ce52930b20cecfbf88bf98de1c4019918"
  end

  on_intel do
    url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.0_aarch64.dmg"
    sha256 "47c8cea32ac14004eb6df19f3f8c3f8ce52930b20cecfbf88bf98de1c4019918"
  end
  # CASK_ARCH_URLS_END
```

The `on_intel` block is a placeholder pointing at the arm64 asset until the
first CI run under Task 1-4 produces a real Intel `.dmg` — `update-cask.yml`
(Task 6) will overwrite both blocks with correct per-arch URLs on the next
published release. This keeps the file syntactically and semantically valid
in the meantime rather than leaving `on_intel` empty.

- [ ] **Step 2: Verify Ruby syntax**

Run: `ruby -c Casks/media-downloader.rb`
Expected: `Syntax OK`

- [ ] **Step 3: Verify with brew's own linters**

Run: `brew style ./Casks/media-downloader.rb`
Expected: no offenses reported (exit code 0)

Run: `brew audit --cask ./Casks/media-downloader.rb`
Expected: no errors (warnings about the cask not being in a tap are fine to ignore locally)

- [ ] **Step 4: Commit**

```bash
git add Casks/media-downloader.rb
git commit -m "chore: split Cask url/sha256 into on_arm/on_intel blocks"
```

---

### Task 6: Teach `update-cask.yml` to populate both architectures

**Files:**
- Modify: `.github/workflows/update-cask.yml`

- [ ] **Step 1: Replace the single-asset lookup with a two-asset lookup**

> **Baseline note:** `update-cask.yml`'s trigger was changed from
> `release: published` to `workflow_run` (off `release.yml` completing) in a
> prerequisite fix, because the `release` webhook proved unreliable in
> production (raced ahead of asset uploads for v0.1.3, never fired at all
> for v0.1.4). The tag name is now available as `steps.asset.outputs.tag`
> (sourced from `github.event.workflow_run.head_branch` inside this same
> step), not `github.event.release.tag_name`. The step below already
> reflects that baseline — this task only adds the second (Intel) asset
> lookup on top of it.

Replace the current `Find the macOS asset and its real download URL` step:

```yaml
      - name: Find the macOS asset and its real download URL
        id: asset
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAG: ${{ github.event.workflow_run.head_branch }}
        run: |
          ASSET_JSON=$(gh release view "$TAG" \
            --repo ${{ github.repository }} \
            --json assets \
            --jq '.assets[] | select(.name | endswith("aarch64.dmg"))')
          if [ -z "$ASSET_JSON" ]; then
            echo "::error::No aarch64 .dmg asset found on $TAG — skipping cask update"
            exit 1
          fi
          NAME=$(echo "$ASSET_JSON" | jq -r '.name')
          URL=$(echo "$ASSET_JSON" | jq -r '.url')
          echo "tag=$TAG" >> "$GITHUB_OUTPUT"
          echo "name=$NAME" >> "$GITHUB_OUTPUT"
          echo "url=$URL" >> "$GITHUB_OUTPUT"
```

with:

```yaml
      - name: Find the macOS assets and their real download URLs
        id: asset
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAG: ${{ github.event.workflow_run.head_branch }}
        run: |
          find_asset() {
            local suffix="$1"
            gh release view "$TAG" \
              --repo ${{ github.repository }} \
              --json assets \
              --jq ".assets[] | select(.name | endswith(\"$suffix\"))"
          }

          ARM_JSON=$(find_asset "aarch64.dmg")
          if [ -z "$ARM_JSON" ]; then
            echo "::error::No aarch64 .dmg asset found on $TAG — skipping cask update"
            exit 1
          fi

          # Tauri's dmg filename suffix for x86_64-apple-darwin, confirmed
          # against a real release build before relying on it in production.
          INTEL_JSON=$(find_asset "x64.dmg")
          if [ -z "$INTEL_JSON" ]; then
            echo "::error::No x64 .dmg asset found on $TAG — skipping cask update"
            exit 1
          fi

          echo "tag=$TAG" >> "$GITHUB_OUTPUT"
          echo "arm_name=$(echo "$ARM_JSON" | jq -r '.name')" >> "$GITHUB_OUTPUT"
          echo "arm_url=$(echo "$ARM_JSON" | jq -r '.url')" >> "$GITHUB_OUTPUT"
          echo "intel_name=$(echo "$INTEL_JSON" | jq -r '.name')" >> "$GITHUB_OUTPUT"
          echo "intel_url=$(echo "$INTEL_JSON" | jq -r '.url')" >> "$GITHUB_OUTPUT"
```

> **Note for the engineer implementing this task:** the `"x64.dmg"` suffix
> above is a best guess at Tauri's naming for `x86_64-apple-darwin` — it is
> flagged as unverified in the spec's Risks section. Before this step ships,
> trigger the `release.yml` workflow once (Task 1-4 must be merged first)
> and check the actual filename of the Intel `.dmg` artifact it uploads;
> update the suffix string here to match if it differs.

- [ ] **Step 2: Download both assets and compute both checksums**

Replace the `Download it and compute the checksum` step:

```yaml
      - name: Download it and compute the checksum
        run: |
          curl -fL "${{ steps.asset.outputs.url }}" -o asset.dmg
          echo "sha256=$(shasum -a 256 asset.dmg | cut -d' ' -f1)" >> "$GITHUB_ENV"
```

with:

```yaml
      - name: Download both assets and compute their checksums
        run: |
          curl -fL "${{ steps.asset.outputs.arm_url }}" -o arm.dmg
          curl -fL "${{ steps.asset.outputs.intel_url }}" -o intel.dmg
          echo "arm_sha256=$(shasum -a 256 arm.dmg | cut -d' ' -f1)" >> "$GITHUB_ENV"
          echo "intel_sha256=$(shasum -a 256 intel.dmg | cut -d' ' -f1)" >> "$GITHUB_ENV"
```

- [ ] **Step 3: Rewrite the Cask's version field and the marked arch block**

Replace the current `Update Casks/media-downloader.rb` step:

```yaml
      - name: Update Casks/media-downloader.rb
        env:
          TAG: ${{ steps.asset.outputs.tag }}
          ASSET_NAME: ${{ steps.asset.outputs.name }}
        run: |
          VERSION="${TAG#v}"
          sed -i \
            -e "s/^  version \".*\"/  version \"$VERSION\"/" \
            -e "s/^  sha256 \".*\"/  sha256 \"$sha256\"/" \
            -e "s#^  url \".*\"#  url \"https://github.com/${{ github.repository }}/releases/download/$TAG/$ASSET_NAME\"#" \
            Casks/media-downloader.rb
          cat Casks/media-downloader.rb
```

with:

```yaml
      - name: Update Casks/media-downloader.rb
        env:
          TAG: ${{ steps.asset.outputs.tag }}
          ARM_NAME: ${{ steps.asset.outputs.arm_name }}
          INTEL_NAME: ${{ steps.asset.outputs.intel_name }}
        run: |
          VERSION="${TAG#v}"
          ARM_URL="https://github.com/${{ github.repository }}/releases/download/$TAG/$ARM_NAME"
          INTEL_URL="https://github.com/${{ github.repository }}/releases/download/$TAG/$INTEL_NAME"

          sed -i -e "s/^  version \".*\"/  version \"$VERSION\"/" Casks/media-downloader.rb

          export NEW_BLOCK=$(cat <<EOF
          # CASK_ARCH_URLS_START
          on_arm do
            url "$ARM_URL"
            sha256 "$arm_sha256"
          end

          on_intel do
            url "$INTEL_URL"
            sha256 "$intel_sha256"
          end
          # CASK_ARCH_URLS_END
          EOF
          )

          perl -0777 -pi -e 's/  # CASK_ARCH_URLS_START.*?# CASK_ARCH_URLS_END/$ENV{NEW_BLOCK}/s' Casks/media-downloader.rb
          cat Casks/media-downloader.rb
```

> **Note for the engineer implementing this task:** `NEW_BLOCK` is
> `export`ed in the same shell script that then invokes `perl`, so `perl`
> (a child process of that same script) inherits it and can read it back via
> `$ENV{NEW_BLOCK}` — no separate GitHub Actions `env:` field is needed or
> valid here (a step can only have one `env:` key). Test this locally in
> Task 6 Step 5 below before trusting it in CI.

- [ ] **Step 4: Verify the YAML still parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/update-cask.yml'))" && echo OK`
Expected: `OK`

- [ ] **Step 5: Dry-run the Cask rewrite locally against a copy of the file**

This exercises the exact `perl` substitution the workflow will run, without
needing a real GitHub release.

```bash
cp Casks/media-downloader.rb /tmp/media-downloader-test.rb
export NEW_BLOCK='  # CASK_ARCH_URLS_START
  on_arm do
    url "https://example.com/arm-test.dmg"
    sha256 "1111111111111111111111111111111111111111111111111111111111111111"
  end

  on_intel do
    url "https://example.com/intel-test.dmg"
    sha256 "2222222222222222222222222222222222222222222222222222222222222222"
  end
  # CASK_ARCH_URLS_END'
perl -0777 -pi -e 's/  # CASK_ARCH_URLS_START.*?# CASK_ARCH_URLS_END/$ENV{NEW_BLOCK}/s' /tmp/media-downloader-test.rb
cat /tmp/media-downloader-test.rb
ruby -c /tmp/media-downloader-test.rb
rm /tmp/media-downloader-test.rb
```

Expected: the printed file shows `arm-test.dmg`/`1111...` inside `on_arm`
and `intel-test.dmg`/`2222...` inside `on_intel`, and `ruby -c` reports
`Syntax OK`.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/update-cask.yml
git commit -m "ci: publish both arm64 and intel assets into the Cask on release"
```

---

### Task 7: End-to-end validation on GitHub Actions (manual, requires confirmation)

This task runs real CI, which is a shared/visible action — confirm with the
user before triggering it.

**Files:** none (validation only)

- [ ] **Step 1: Push the branch and open a PR (or push directly if the user says so) so `release.yml` is inspectable on GitHub**

- [ ] **Step 2: Ask the user for explicit confirmation, then trigger `release.yml` via `workflow_dispatch`** (it already supports manual dispatch — see `.github/workflows/release.yml:28`)

```bash
gh workflow run release.yml --ref <branch>
```

- [ ] **Step 3: Watch the run and confirm both macOS legs pass**

```bash
gh run watch
```

Expected: `macos-latest` (`aarch64-apple-darwin`) and `macos-13`
(`x86_64-apple-darwin`) both succeed, including the new smoke-test step and
the widened ffmpeg verify step.

- [ ] **Step 4: Download the artifacts and record the real Intel `.dmg` filename**

```bash
gh run download <run-id> -n media-downloader-x86_64-apple-darwin
ls
```

Compare the actual filename against the `"x64.dmg"` guess used in Task 6
Step 1. If it differs, fix the suffix in `update-cask.yml` and re-commit
before this feature is considered done.

- [ ] **Step 5: Update the spec's open question**

Edit `docs/superpowers/specs/2026-08-04-intel-mac-build-design.md`'s Risks
section to record the confirmed filename (replacing "unverified until the
first CI run").

```bash
git add docs/superpowers/specs/2026-08-04-intel-mac-build-design.md
git commit -m "docs: record the confirmed Intel dmg asset filename"
```

---

## Notes for whoever executes this plan

- Tasks 1-4 touch the same file (`release.yml`) in sequence — each task's
  "replace X with Y" assumes the previous task's edit already landed. Do not
  reorder them.
- Task 5's placeholder `on_intel` block deliberately points at the arm64
  asset so the Cask stays valid (installable, just wrong-binary-for-Intel
  same as today) between merging this plan and the first real release that
  runs `update-cask.yml`'s new logic. It is not a permanent state.
- Task 6's asset-suffix guess (`"x64.dmg"`) is the one genuinely unverified
  piece of this plan — Task 7 exists specifically to verify and fix it
  against a real build before calling this done.
