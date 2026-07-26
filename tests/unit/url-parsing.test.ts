import { describe, expect, it } from "vitest";

import { dedupeUrls, extractUrlsFromText, isMusicUrl, isValidUrl } from "@/lib/url-parsing";

describe("isValidUrl", () => {
  it("accepts http and https", () => {
    expect(isValidUrl("https://example.com/v")).toBe(true);
    expect(isValidUrl("http://example.com/v")).toBe(true);
  });

  it("rejects other schemes and plain text", () => {
    expect(isValidUrl("ftp://example.com/v")).toBe(false);
    expect(isValidUrl("file:///etc/passwd")).toBe(false);
    expect(isValidUrl("just some words")).toBe(false);
    expect(isValidUrl("")).toBe(false);
  });
});

describe("extractUrlsFromText", () => {
  it("pulls urls out of surrounding prose", () => {
    expect(extractUrlsFromText("xem https://a.example/1 nhé")).toEqual(["https://a.example/1"]);
  });

  it("handles one url per line", () => {
    expect(extractUrlsFromText("https://a.example/1\nhttps://b.example/2")).toEqual([
      "https://a.example/1",
      "https://b.example/2",
    ]);
  });

  it("strips trailing punctuation that is clearly not part of the url", () => {
    expect(extractUrlsFromText("(https://a.example/1),")).toEqual(["https://a.example/1"]);
  });

  it("removes duplicates while keeping first-seen order", () => {
    expect(
      extractUrlsFromText("https://b.example/2 https://a.example/1 https://b.example/2"),
    ).toEqual(["https://b.example/2", "https://a.example/1"]);
  });

  it("returns an empty list when there is nothing url-like", () => {
    expect(extractUrlsFromText("không có link nào ở đây")).toEqual([]);
  });
});

describe("isMusicUrl (T013)", () => {
  it("recognises every SpotiFLAC-routed host", () => {
    expect(isMusicUrl("https://open.spotify.com/track/abc")).toBe(true);
    expect(isMusicUrl("https://spotify.com/track/abc")).toBe(true);
    expect(isMusicUrl("https://listen.tidal.com/album/1")).toBe(true);
    expect(isMusicUrl("https://tidal.com/browse/track/1")).toBe(true);
    expect(isMusicUrl("https://music.apple.com/vn/album/x/1")).toBe(true);
    expect(isMusicUrl("https://pandora.com/artist/x")).toBe(true);
    expect(isMusicUrl("https://pandora.app.link/abc")).toBe(true);
  });

  it("tolerates a www prefix and mixed case", () => {
    expect(isMusicUrl("https://www.pandora.com/artist/x")).toBe(true);
    expect(isMusicUrl("HTTPS://OPEN.SPOTIFY.COM/track/abc")).toBe(true);
  });

  it("parses the real hostname instead of matching substrings", () => {
    // A music host appearing in the *path* or as a subdomain of another
    // domain must not fool the check.
    expect(isMusicUrl("https://evil.com/open.spotify.com")).toBe(false);
    expect(isMusicUrl("https://open.spotify.com.evil.com/track/abc")).toBe(false);
    expect(isMusicUrl("https://example.com/?u=https://tidal.com")).toBe(false);
  });

  it("leaves the yt-dlp platforms to the existing engines", () => {
    expect(isMusicUrl("https://soundcloud.com/artist/track")).toBe(false);
    expect(isMusicUrl("https://youtube.com/watch?v=abc")).toBe(false);
    expect(isMusicUrl("https://music.youtube.com/watch?v=abc")).toBe(false);
  });

  it("rejects things that are not http(s) urls at all", () => {
    expect(isMusicUrl("ftp://open.spotify.com/track/abc")).toBe(false);
    expect(isMusicUrl("open.spotify.com/track/abc")).toBe(false);
    expect(isMusicUrl("")).toBe(false);
  });
});

describe("dedupeUrls", () => {
  it("reports which urls were dropped as duplicates", () => {
    const result = dedupeUrls(["https://a.example/1", "https://a.example/1", "https://b.example/2"]);
    expect(result.unique).toEqual(["https://a.example/1", "https://b.example/2"]);
    expect(result.duplicateCount).toBe(1);
  });
});
