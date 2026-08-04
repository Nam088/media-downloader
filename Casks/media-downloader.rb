cask "media-downloader" do
  # `version` tracks the release tag, NOT the filename below: the app's own
  # tauri.conf.json version and the git tag can drift (a real release has
  # shipped a tag bump with no matching app-version bump), so the dmg's
  # embedded version number in its filename cannot be trusted to match the
  # tag. The auto-update workflow (.github/workflows/update-cask.yml) looks
  # up the real asset name and sha256 from the release itself each time
  # instead of reconstructing either from this version string.
  version "0.1.4"

  # update-cask.yml rewrites everything between these two markers wholesale
  # on every published release — don't hand-edit the URLs/checksums here,
  # they'll be overwritten on the next release anyway. on_intel is a
  # placeholder pointing at the arm64 asset until release CI actually builds
  # an Intel dmg (see docs/superpowers/plans/2026-08-04-intel-mac-build.md).
  # CASK_ARCH_URLS_START
  on_arm do
    sha256 "544dc8de2ed1af8f1e392794774ecd253bff26463aafb98a696cf44f28cf5d32"

    url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.4_aarch64.dmg"
  end
  on_intel do
    sha256 "544dc8de2ed1af8f1e392794774ecd253bff26463aafb98a696cf44f28cf5d32"

    url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.4_aarch64.dmg"
  end

  # CASK_ARCH_URLS_END

  name "Media Downloader"
  desc "Download video and audio from YouTube and 1000+ other sites"
  homepage "https://github.com/Nam088/media-downloader"

  depends_on :macos

  app "Media Downloader.app"

  uninstall quit: "io.github.nam088.mediadownloader"

  zap trash: [
    "~/Library/Application Support/io.github.nam088.mediadownloader",
    "~/Library/Caches/io.github.nam088.mediadownloader",
    "~/Library/Preferences/io.github.nam088.mediadownloader.plist",
    "~/Library/Saved Application State/io.github.nam088.mediadownloader.savedState",
  ]
end
