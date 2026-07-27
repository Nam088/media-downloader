cask "media-downloader" do
  # `version` tracks the release tag, NOT the filename below: the app's own
  # tauri.conf.json version and the git tag can drift (a real release has
  # shipped a tag bump with no matching app-version bump), so the dmg's
  # embedded version number in its filename cannot be trusted to match the
  # tag. The auto-update workflow (.github/workflows/update-cask.yml) looks
  # up the real asset name and sha256 from the release itself each time
  # instead of reconstructing either from this version string.
  version "0.1.1"
  sha256 "47c8cea32ac14004eb6df19f3f8c3f8ce52930b20cecfbf88bf98de1c4019918"

  url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_0.1.0_aarch64.dmg"
  name "Media Downloader"
  desc "Download video and audio from YouTube and 1000+ other sites"
  homepage "https://github.com/Nam088/media-downloader"

  # Release CI only builds aarch64-apple-darwin today — installing this cask
  # on an Intel Mac would silently ship a binary that can't run at all, so
  # fail the install with a clear reason instead.
  depends_on arch: :arm64

  app "Media Downloader.app"

  uninstall quit: "io.github.nam088.mediadownloader"

  zap trash: [
    "~/Library/Application Support/io.github.nam088.mediadownloader",
    "~/Library/Caches/io.github.nam088.mediadownloader",
    "~/Library/Preferences/io.github.nam088.mediadownloader.plist",
    "~/Library/Saved Application State/io.github.nam088.mediadownloader.savedState",
  ]
end
