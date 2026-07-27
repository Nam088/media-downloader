cask "media-downloader" do
  version "0.1.0"
  sha256 "12306fc5301417ee81c486bf5a5ab437b09392d6aab02e5b86f67c27723a7b2a"

  url "https://github.com/Nam088/media-downloader/releases/download/v#{version}/Media.Downloader_#{version}_aarch64.dmg"
  name "Media Downloader"
  desc "Download video, audio, and lossless music from YouTube, Spotify, Tidal, and more"
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
