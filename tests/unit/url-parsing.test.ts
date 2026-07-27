import { describe, expect, it } from "vitest";

import { dedupeUrls, extractUrlsFromText, isValidUrl } from "@/lib/url-parsing";

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

describe("dedupeUrls", () => {
  it("reports which urls were dropped as duplicates", () => {
    const result = dedupeUrls(["https://a.example/1", "https://a.example/1", "https://b.example/2"]);
    expect(result.unique).toEqual(["https://a.example/1", "https://b.example/2"]);
    expect(result.duplicateCount).toBe(1);
  });
});
