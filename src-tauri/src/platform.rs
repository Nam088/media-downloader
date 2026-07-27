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
];

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

}
