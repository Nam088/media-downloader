import { describe, expect, it } from "vitest";

import { formatDuration, formatEta, formatFileSize, formatSpeed } from "@/lib/format";

describe("formatDuration", () => {
  it("shows minutes and seconds below an hour", () => {
    expect(formatDuration(75)).toBe("1:15");
  });

  it("shows hours once past one", () => {
    expect(formatDuration(3725)).toBe("1:02:05");
  });

  it("pads seconds so 1:05 never renders as 1:5", () => {
    expect(formatDuration(65)).toBe("1:05");
  });

  it("returns a placeholder when the source gave no duration", () => {
    expect(formatDuration(null)).toBe("--:--");
    expect(formatDuration(undefined)).toBe("--:--");
  });
});

describe("formatFileSize", () => {
  it("scales to the largest unit that keeps the number readable", () => {
    expect(formatFileSize(999)).toBe("999 B");
    expect(formatFileSize(1024)).toBe("1.0 KB");
    expect(formatFileSize(1024 * 1024 * 5.5)).toBe("5.5 MB");
    expect(formatFileSize(1024 * 1024 * 1024 * 2)).toBe("2.0 GB");
  });

  it("returns a placeholder when the size is unknown", () => {
    expect(formatFileSize(null)).toBe("--");
  });
});

describe("formatSpeed", () => {
  it("appends a per-second suffix", () => {
    expect(formatSpeed(1024 * 1024)).toBe("1.0 MB/s");
  });

  it("returns a placeholder when no speed has been reported yet", () => {
    expect(formatSpeed(null)).toBe("--");
  });
});

describe("formatEta", () => {
  it("reuses the duration format", () => {
    expect(formatEta(90)).toBe("1:30");
  });

  it("returns a placeholder when unknown", () => {
    expect(formatEta(null)).toBe("--:--");
  });
});
