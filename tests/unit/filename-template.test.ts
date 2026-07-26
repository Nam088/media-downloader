import { describe, expect, it } from "vitest";

import {
  DEFAULT_TEMPLATE,
  MAX_FILENAME_BYTES,
  TEMPLATE_FALLBACKS,
  TEMPLATE_FIELDS,
  UNTITLED,
  renderTemplatePreview,
  sanitizeFilename,
  type TemplateSource,
} from "@/lib/filename-template";

/**
 * Ô xem trước phải chạy đúng luật mà bộ tải chạy, nên phần lớn các trường hợp
 * dưới đây là bản sao nguyên văn của `#[cfg(test)] mod tests` trong
 * `src-tauri/src/downloader/filename.rs`. Đổi kỳ vọng ở một bên mà quên bên kia
 * là đúng cái lỗi "ô xem trước nói dối" mà FR-213 muốn tránh.
 */

const byteLength = (value: string) => new TextEncoder().encode(value).length;

const FULL_SOURCE: TemplateSource = {
  title: "Chúng ta của tương lai",
  channel: "Sơn Tùng M-TP",
  playlistIndex: 3,
  uploadDate: "20260726",
  resolution: "1080p",
  ext: "mp3",
};

const render = (template: string, source: TemplateSource = FULL_SOURCE) =>
  renderTemplatePreview(template, source).filename;

describe("sanitizeFilename", () => {
  it("replaces every character Windows forbids", () => {
    for (const forbidden of ["\\", "/", ":", "*", "?", '"', "<", ">", "|"]) {
      expect(sanitizeFilename(`a${forbidden}b`), forbidden).toBe("a_b");
    }
  });

  it("replaces all forbidden characters in one name", () => {
    expect(sanitizeFilename('a\\b/c:d*e?f"g<h>i|j')).toBe("a_b_c_d_e_f_g_h_i_j");
  });

  it("replaces control characters", () => {
    expect(sanitizeFilename("dòng một\ndòng hai")).toBe("dòng một_dòng hai");
    expect(sanitizeFilename("tab\there")).toBe("tab_here");
  });

  it("guards bare reserved device names", () => {
    expect(sanitizeFilename("CON")).toBe("CON_");
    expect(sanitizeFilename("NUL")).toBe("NUL_");
    expect(sanitizeFilename("COM1")).toBe("COM1_");
    expect(sanitizeFilename("LPT9")).toBe("LPT9_");
  });

  it("guards reserved device names that carry an extension", () => {
    expect(sanitizeFilename("CON.mp3")).toBe("CON_.mp3");
    expect(sanitizeFilename("con.mp3")).toBe("con_.mp3");
    expect(sanitizeFilename("Aux.TXT")).toBe("Aux_.TXT");
    expect(sanitizeFilename("COM9.tar.gz")).toBe("COM9_.tar.gz");
  });

  it("leaves names that only look reserved", () => {
    expect(sanitizeFilename("CONSOLE")).toBe("CONSOLE");
    expect(sanitizeFilename("COM10")).toBe("COM10");
    expect(sanitizeFilename("MyCON")).toBe("MyCON");
  });

  it("trims a trailing dot and a trailing space", () => {
    expect(sanitizeFilename("Tên bài.")).toBe("Tên bài");
    expect(sanitizeFilename("Tên bài ")).toBe("Tên bài");
    expect(sanitizeFilename("Tên bài. . .")).toBe("Tên bài");
    expect(sanitizeFilename("  Tên bài  ")).toBe("Tên bài");
  });

  it("falls back when nothing meaningful survives", () => {
    expect(sanitizeFilename("///")).toBe(UNTITLED);
    expect(sanitizeFilename('\\/:*?"<>|')).toBe(UNTITLED);
    expect(sanitizeFilename("")).toBe(UNTITLED);
    expect(sanitizeFilename("   ")).toBe(UNTITLED);
    expect(sanitizeFilename("..")).toBe(UNTITLED);
  });

  it("strips path separators so a template cannot escape the output directory", () => {
    expect(sanitizeFilename("../../etc/passwd")).toBe(".._.._etc_passwd");
    expect(sanitizeFilename("..\\..\\Windows\\System32")).toBe(".._.._Windows_System32");
  });

  it("never splits a multi-byte character when truncating", () => {
    // "ề" là 3 byte: trần 10 byte rơi vào giữa ký tự thứ 4 nếu cắt thô.
    expect(byteLength("ềềềề")).toBe(12);
    const truncated = sanitizeFilename("ềềềề", 10);
    expect(truncated).toBe("ềềề");
    expect(byteLength(truncated)).toBeLessThanOrEqual(10);

    // Emoji là 4 byte / 2 đơn vị UTF-16 — cắt theo `.length` sẽ ra nửa cặp thay thế.
    expect(byteLength("🎵🎵🎵")).toBe(12);
    expect(sanitizeFilename("🎵🎵🎵", 10)).toBe("🎵🎵");
  });

  it("keeps the extension when truncating", () => {
    const truncated = sanitizeFilename(`${"a".repeat(300)}.mp3`);
    expect(truncated.endsWith(".mp3")).toBe(true);
    expect(byteLength(truncated)).toBe(MAX_FILENAME_BYTES);
    expect(truncated).toBe(`${"a".repeat(251)}.mp3`);
  });

  it("keeps the extension when the stem is multi-byte", () => {
    const truncated = sanitizeFilename(`${"ề".repeat(100)}.mp3`);
    expect(truncated.endsWith(".mp3")).toBe(true);
    expect(byteLength(truncated)).toBeLessThanOrEqual(MAX_FILENAME_BYTES);
    // 255 - 4 = 251 byte cho phần thân → 83 chữ (249 byte); chữ thứ 84 không lọt.
    expect(truncated).toBe(`${"ề".repeat(83)}.mp3`);
  });

  it("does not leave a trailing dot or space after truncating", () => {
    const truncated = sanitizeFilename(`${"a".repeat(253)} . x`);
    expect(truncated.endsWith(".")).toBe(false);
    expect(truncated.endsWith(" ")).toBe(false);
  });

  it("does not mistake a long parenthetical for an extension", () => {
    const truncated = sanitizeFilename(`${"a".repeat(250)} (2026. Deluxe Edition)`);
    expect(byteLength(truncated)).toBeLessThanOrEqual(MAX_FILENAME_BYTES);
    expect(truncated.startsWith("aaa")).toBe(true);
  });

  it("leaves short names untouched", () => {
    expect(sanitizeFilename("Chúng ta của tương lai")).toBe("Chúng ta của tương lai");
    expect(sanitizeFilename("01 - Bài hát 🎵.mp3")).toBe("01 - Bài hát 🎵.mp3");
  });
});

describe("renderTemplatePreview", () => {
  it("renders every supported field", () => {
    expect(
      render("{playlist_index} - {channel} - {title} ({upload_date}) [{resolution}].{ext}"),
    ).toBe("03 - Sơn Tùng M-TP - Chúng ta của tương lai (2026-07-26) [1080p].mp3");
  });

  it("gives each missing field its own fallback", () => {
    expect(render("{title}", {})).toBe(TEMPLATE_FALLBACKS.title);
    expect(render("{channel}", {})).toBe(TEMPLATE_FALLBACKS.channel);
    expect(render("{playlist_index}", {})).toBe(TEMPLATE_FALLBACKS.playlist_index);
    expect(render("{upload_date}", {})).toBe(TEMPLATE_FALLBACKS.upload_date);
    expect(render("{resolution}", {})).toBe(TEMPLATE_FALLBACKS.resolution);
    expect(render("{ext}", {})).toBe(TEMPLATE_FALLBACKS.ext);
  });

  it("treats an explicit null the same as a missing field", () => {
    expect(render("{title}", { title: null, playlistIndex: null })).toBe(
      TEMPLATE_FALLBACKS.title,
    );
    expect(render("{playlist_index}", { playlistIndex: null })).toBe(
      TEMPLATE_FALLBACKS.playlist_index,
    );
  });

  it("treats a present but blank field as missing", () => {
    expect(render("{title}", { title: "   " })).toBe(TEMPLATE_FALLBACKS.title);
    expect(render("{channel}", { channel: "" })).toBe(TEMPLATE_FALLBACKS.channel);
  });

  it("never renders an empty name, even when nothing is known", () => {
    // FR-216.
    expect(render("{title}", {})).not.toBe("");
    expect(render("", {})).toBe(UNTITLED);
  });

  it("pads the playlist index to two digits but never clips it", () => {
    expect(render("{playlist_index}", { playlistIndex: 1 })).toBe("01");
    expect(render("{playlist_index}", { playlistIndex: 137 })).toBe("137");
  });

  it("keeps an already dashed upload date as is", () => {
    expect(render("{upload_date}", { uploadDate: "2026-07-26" })).toBe("2026-07-26");
  });

  it("leaves an unknown field visible and reports it instead of swallowing it", () => {
    const preview = renderTemplatePreview("{titel} - x", FULL_SOURCE);
    expect(preview.filename).toBe("{titel} - x");
    expect(preview.unknownFields).toEqual(["titel"]);
  });

  it("reports each unknown field once, in order", () => {
    const preview = renderTemplatePreview("{a}{b}{a}{title}", FULL_SOURCE);
    expect(preview.unknownFields).toEqual(["a", "b"]);
  });

  it("reports no unknown fields for a template that only uses real ones", () => {
    expect(renderTemplatePreview("{title} - {channel}", FULL_SOURCE).unknownFields).toEqual([]);
  });

  it("leaves an unclosed brace as literal text", () => {
    expect(render("{title")).toBe("{title");
  });

  it("sanitizes field values that came from the source", () => {
    expect(render("{title}", { title: "AC/DC: Back in Black?" })).toBe("AC_DC_ Back in Black_");
  });

  it("cannot build a subdirectory or climb out of the output directory", () => {
    const rendered = render("{channel}/../../{title}", { channel: "kênh", title: "bài" });
    expect(rendered).not.toContain("/");
    expect(rendered).not.toContain("\\");
    expect(rendered).toBe("kênh_.._.._bài");
  });

  it("reproduces the old behaviour with the default template", () => {
    expect(render(DEFAULT_TEMPLATE)).toBe("Chúng ta của tương lai");
  });

  it("keeps the advertised field list and the fallback table in step", () => {
    // Mọi trường được quảng cáo phải thật sự thay thế được và phải có giá trị
    // dự phòng — nếu không FR-216 hở đúng ở trường mới thêm.
    for (const field of TEMPLATE_FIELDS) {
      const preview = renderTemplatePreview(`{${field}}`, {});
      expect(preview.unknownFields, field).toEqual([]);
      expect(preview.filename, field).toBe(TEMPLATE_FALLBACKS[field]);
    }
  });
});
