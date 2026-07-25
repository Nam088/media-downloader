// Sample links used by the SC-002 platform-coverage smoke test (T049).
//
// Intentionally left as placeholders: picking a specific real video/track
// URL on someone's behalf risks pointing at content that's private,
// deleted, region-locked, or simply not this project's to reference.
// Fill each one in with a short, public, stable link you've checked
// yourself (a channel's own test upload works well) before running this
// suite — see quickstart.md "Kiểm tra nhanh các yêu cầu bổ sung".
export const PLATFORM_SAMPLE_LINKS: Record<string, string> = {
  youtube: "TODO: public YouTube video URL",
  tiktok: "TODO: public TikTok video URL",
  facebook: "TODO: public Facebook video URL",
  instagram: "TODO: public Instagram video/reel URL",
  twitter_x: "TODO: public X/Twitter video URL",
  soundcloud: "TODO: public SoundCloud track URL",
};

export const SC001_MAX_AUDIO_READY_MS = 30_000; // SC-001: <30s for a <10min video
