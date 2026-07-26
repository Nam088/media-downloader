use url::Url;

/// The 6 platforms FR-014 requires as the minimum v1 set. Matching is done on
/// the parsed hostname (not a substring of the raw URL) so a malicious/odd
/// URL like `https://evil.com/youtube.com` cannot spoof a supported platform.
const PLATFORM_HOSTS: &[(&str, &[&str])] = &[
    ("youtube", &["youtube.com", "youtu.be"]),
    ("tiktok", &["tiktok.com"]),
    ("facebook", &["facebook.com", "fb.watch"]),
    ("instagram", &["instagram.com"]),
    ("twitter_x", &["twitter.com", "x.com"]),
    ("soundcloud", &["soundcloud.com"]),
    // Bốn nền tảng nhạc lossless của engine SpotiFLAC (specs/006, FR-001).
    // SoundCloud/YouTube CỐ Ý không nằm trong nhóm này: hai nguồn đó vốn
    // không có lossless và tiếp tục đi qua yt-dlp (research.md R2).
    ("spotify", &["open.spotify.com", "spotify.com"]),
    ("tidal", &["listen.tidal.com", "tidal.com"]),
    ("apple_music", &["music.apple.com"]),
    ("pandora", &["pandora.com", "pandora.app.link"]),
];

/// Các platform được định tuyến sang engine SpotiFLAC thay vì yt-dlp.
/// `commands::media::preview_media` hỏi hàm này TRƯỚC khi thử yt-dlp, vì
/// yt-dlp không lỗi "sạch" với link Spotify (nó trả về một preview rỗng thay
/// vì UNSUPPORTED_PLATFORM, nên cơ chế fallback hiện có không bắt được).
pub fn is_music_platform(platform: &str) -> bool {
    matches!(platform, "spotify" | "tidal" | "apple_music" | "pandora")
}

/// Tiện ích gộp: URL có thuộc về engine SpotiFLAC không.
pub fn is_music_url(source_url: &str) -> bool {
    detect_platform(source_url).is_some_and(is_music_platform)
}

pub fn detect_platform(source_url: &str) -> Option<&'static str> {
    let url = Url::parse(source_url).ok()?;
    let host = url.host_str()?.to_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);

    for (platform, domains) in PLATFORM_HOSTS {
        if domains
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
        {
            return Some(platform);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_platforms() {
        assert_eq!(
            detect_platform("https://www.youtube.com/watch?v=abc"),
            Some("youtube")
        );
        assert_eq!(detect_platform("https://youtu.be/abc"), Some("youtube"));
        assert_eq!(
            detect_platform("https://www.tiktok.com/@user/video/123"),
            Some("tiktok")
        );
        assert_eq!(detect_platform("https://x.com/user/status/1"), Some("twitter_x"));
    }

    #[test]
    fn rejects_unsupported_or_spoofed_hosts() {
        assert_eq!(detect_platform("https://example.com/video"), None);
        assert_eq!(detect_platform("https://evil.com/youtube.com"), None);
        assert_eq!(detect_platform("not a url"), None);
    }

    #[test]
    fn detects_music_platforms_for_the_spotiflac_engine() {
        assert_eq!(
            detect_platform("https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"),
            Some("spotify")
        );
        assert_eq!(
            detect_platform("https://listen.tidal.com/album/364272512"),
            Some("tidal")
        );
        assert_eq!(
            detect_platform("https://music.apple.com/us/album/x/123?i=456"),
            Some("apple_music")
        );
        assert_eq!(
            detect_platform("https://pandora.app.link/abc"),
            Some("pandora")
        );

        for url in [
            "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M",
            "https://music.apple.com/us/song/9",
        ] {
            assert!(is_music_url(url), "{url} phải đi qua engine SpotiFLAC");
        }
    }

    #[test]
    fn soundcloud_and_youtube_stay_on_ytdlp() {
        // research.md R2: hai nguồn không lossless này giữ nguyên pipeline
        // yt-dlp — is_music_url phải trả false dù SpotiFLAC "biết" chúng.
        assert!(!is_music_url("https://soundcloud.com/artist/track"));
        assert!(!is_music_url("https://www.youtube.com/watch?v=abc"));
    }
}
