/// <reference types="vite/client" />
import { describe, expect, it } from "vitest";

/**
 * FR-132: every string the user can read must come from `t()`, so switching
 * language actually switches the whole screen.
 *
 * This is a heuristic, not a TypeScript parser. It works in two stages:
 *
 *   1. A small hand-written scanner walks each file once, tracking whether it
 *      is inside a line comment, a block comment, or a string/template
 *      literal. It yields every string literal with its line number, plus a
 *      "code view" of the file in which comment text and string *contents*
 *      have been replaced by spaces (offsets preserved). Scanning beats a bare
 *      regex here because it never mistakes `https://` inside a URL for a
 *      comment, and because the code view lets the later rules look at
 *      structure without tripping over punctuation that lives inside strings.
 *
 *   2. Five rules run over that output. Three are *positional* — they flag a
 *      literal because of where it sits, whatever it says: a user-facing prop
 *      (`placeholder`, `title`, `aria-label`, ...), an argument to `toast.*`,
 *      and a JSX text node. Two are *textual* — they flag a literal because of
 *      how it reads, wherever it sits: sentence-case prose, and a template
 *      literal that wraps words around a `${...}` value.
 *
 * Why not the diacritics-only check the plan sketched: it keys on Vietnamese
 * accents, so it catches today's Vietnamese leftovers but waves through a new
 * hardcoded English label — which is the far more likely regression, since the
 * reference UI this app was built from is in English. The rules below key on
 * position and on sentence-case prose instead, so English and Vietnamese are
 * caught the same way.
 */

/**
 * Every file under the two directories that render copy for the user, loaded
 * as raw text through Vite rather than `node:fs` — the repo has no
 * `@types/node`, and this keeps the guard resolving paths the same way the app
 * itself does. Patterns must be string literals for Vite to analyse them, so
 * the two directories are listed out rather than looped over.
 */
const SOURCES: Record<string, string> = {
  ...import.meta.glob("/src/components/**/*.{ts,tsx}", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
  ...import.meta.glob("/src/pages/**/*.{ts,tsx}", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
};

/**
 * shadcn/ui primitives are vendored, unmodified building blocks (Button,
 * Dialog, ...). They carry no copy of their own — every word they render is
 * passed in as children by the callers we do scan.
 */
const EXCLUDED_DIRECTORY_SEGMENT = "/src/components/ui/";

/**
 * Props and object keys whose value is read by a human: rendered as text,
 * announced by a screen reader, or shown in a tooltip. A literal here is
 * user-facing no matter how short or how lowercase it is.
 */
const USER_FACING_PROPS = new Set([
  "placeholder",
  "title",
  "alt",
  "label",
  "hint",
  "heading",
  "caption",
  "aria-label",
  "ariaLabel",
  "aria-description",
  "ariaDescription",
]);

/** `toast.success("...")` and friends put their argument straight on screen. */
const TOAST_CALL = /^toast(\.\w+)?$/;

/**
 * Props whose value is consumed by a machine, never read by a person. Skipped
 * outright, because a Tailwind class list is multi-word lowercase text that
 * would otherwise be indistinguishable from lowercase copy.
 */
const TECHNICAL_PROPS = new Set(["className", "id", "key", "htmlFor", "role", "dir", "type"]);

/**
 * Characters that sentence-case UI copy is built from. Everything a developer
 * writes that is *not* prose — Tailwind class lists (`bg-primary/5`), codec
 * specs (`MP4 / H264 / AAC`), template-literal ids (`quality-${x}`), and stray
 * code fragments that the JSX-text rule may capture (`(null); return (`) —
 * carries at least one character from outside this set. Notably absent:
 * brackets of every kind, `/`, `\`, `_`, `$`, `=`, `;`, `|`, `"` and backtick.
 */
const PROSE_CHARACTERS = /^[\p{L}\p{N}\s.,:!?'’‘…\-–—%]+$/u;

/**
 * Marks where a `${...}` interpolation was, so it can be scored as a word.
 * NUL is the sentinel because no TypeScript source ever contains one, so it
 * cannot collide with real literal text. Written as an escape rather than a
 * raw byte so this file stays plain text that git can diff and review.
 */
const INTERPOLATION = "\u0000";

interface StringLiteral {
  /** Literal text, with each `${...}` collapsed to `INTERPOLATION`. */
  value: string;
  /** Byte offset of the opening quote. */
  start: number;
  line: number;
}

interface ScannedFile {
  literals: StringLiteral[];
  /** Source with comment text and string contents blanked to spaces. */
  code: string;
}

/** Offset -> 1-based line number, via a precomputed table of line starts. */
function lineIndex(source: string): (offset: number) => number {
  const starts: number[] = [0];
  for (let i = 0; i < source.length; i++) {
    if (source[i] === "\n") starts.push(i + 1);
  }
  return (offset) => {
    let low = 0;
    let high = starts.length - 1;
    while (low < high) {
      const mid = Math.ceil((low + high) / 2);
      if (starts[mid] <= offset) low = mid;
      else high = mid - 1;
    }
    return low + 1;
  };
}

/**
 * Single pass over the source, tracking comment and string state.
 *
 * Known limitation: a regular-expression literal containing a quote character
 * (`/["']/`) would be misread as the start of a string. No such regex exists
 * in the scanned directories today; if one is added, the guard fails loudly on
 * a nonsense literal rather than silently passing, which is the safe direction.
 */
function scan(source: string): ScannedFile {
  const literals: StringLiteral[] = [];
  const masked = source.split("");
  const lineOf = lineIndex(source);
  const blank = (from: number, to: number) => {
    for (let k = from; k < to && k < source.length; k++) {
      if (source[k] !== "\n") masked[k] = " ";
    }
  };

  let i = 0;
  while (i < source.length) {
    const char = source[i];
    const next = source[i + 1];

    if (char === "/" && next === "/") {
      const end = source.indexOf("\n", i);
      const stop = end === -1 ? source.length : end;
      blank(i, stop);
      i = stop;
      continue;
    }

    if (char === "/" && next === "*") {
      const end = source.indexOf("*/", i + 2);
      const stop = end === -1 ? source.length : end + 2;
      blank(i, stop);
      i = stop;
      continue;
    }

    if (char !== '"' && char !== "'" && char !== "`") {
      i += 1;
      continue;
    }

    const quote = char;
    const start = i;
    let value = "";
    let cursor = i + 1;
    let closed = false;

    while (cursor < source.length) {
      const c = source[cursor];
      if (c === "\\") {
        value += source[cursor + 1] ?? "";
        cursor += 2;
        continue;
      }
      if (c === quote) {
        closed = true;
        break;
      }
      if (quote !== "`" && c === "\n") break;
      if (quote === "`" && c === "$" && source[cursor + 1] === "{") {
        let depth = 0;
        cursor += 1;
        while (cursor < source.length) {
          if (source[cursor] === "{") depth += 1;
          else if (source[cursor] === "}") {
            depth -= 1;
            if (depth === 0) {
              cursor += 1;
              break;
            }
          }
          cursor += 1;
        }
        value += INTERPOLATION;
        continue;
      }
      value += c;
      cursor += 1;
    }

    if (!closed) {
      // An apostrophe in JSX text (`isn't`) or an unterminated quote: not a
      // string literal. Step over just the quote so the rest still scans.
      i = start + 1;
      continue;
    }

    blank(start + 1, cursor);
    literals.push({ value, start, line: lineOf(start) });
    i = cursor + 1;
  }

  return { literals, code: masked.join("") };
}

/**
 * Name of the call whose argument list directly encloses `offset`, e.g. `"t"`
 * for `t("key")` and `"toast.success"` for `toast.success("done")`; `null` at
 * the top level. Runs over the blanked code view, so parentheses inside string
 * contents cannot skew the depth count.
 */
function enclosingCall(code: string, offset: number): string | null {
  let depth = 0;
  for (let i = offset - 1; i >= 0; i--) {
    const c = code[i];
    if (c === ")") {
      depth += 1;
    } else if (c === "(") {
      if (depth > 0) {
        depth -= 1;
        continue;
      }
      let end = i;
      while (end > 0 && /\s/.test(code[end - 1])) end -= 1;
      let begin = end;
      while (begin > 0 && /[\w$.]/.test(code[begin - 1])) begin -= 1;
      const name = code.slice(begin, end);
      return name.length > 0 ? name : null;
    }
  }
  return null;
}

/** Prop or object key this literal is the value of, e.g. `placeholder="x"`. */
const PROPERTY_BEFORE = /(?:^|[\s{,;(])((?:aria-)?[A-Za-z][\w-]*)\s*[:=]\s*\{?\s*$/;

function propertyName(code: string, offset: number): string | null {
  const window = code.slice(Math.max(0, offset - 80), offset);
  return PROPERTY_BEFORE.exec(window)?.[1] ?? null;
}

function letterCount(text: string): number {
  return (text.match(/\p{L}/gu) ?? []).length;
}

/**
 * True when the text reads as UI copy rather than as a value some machine
 * consumes. Two signals must both hold:
 *
 *  - it uses only prose characters (see `PROSE_CHARACTERS`), and
 *  - it is sentence-case *and* multi-word, or it carries a Vietnamese accent.
 *
 * The sentence-case-plus-multi-word test is what separates copy from code in
 * this codebase: every Tailwind class list, status enum, media type and DOM
 * key name is lowercase or a single word, while every line of copy in
 * `en.json` starts with a capital and contains a space. Single capitalised
 * words that are genuinely UI copy ("Completed", "Retry") are still caught,
 * but by the positional rules rather than this one.
 */
function looksLikeProse(value: string): boolean {
  const literalOnly = value.split(INTERPOLATION).join("");
  if (letterCount(literalOnly) < 2) return false;

  // Interpolations become a stand-in word so `${count} files` counts as prose
  // while `${a}:${b}` — which has no literal letters at all — does not.
  const display = value.split(INTERPOLATION).join(" Xx ").trim();
  if (!PROSE_CHARACTERS.test(display)) return false;

  if (/[àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ]/i.test(literalOnly)) {
    return true;
  }

  const firstLetter = /\p{L}/u.exec(literalOnly)?.[0] ?? "";
  return firstLetter === firstLetter.toUpperCase() && /\s/.test(display);
}

/**
 * A template literal that mixes a `${...}` value into several words of text,
 * e.g. `` `${count} links ready` ``. `looksLikeProse` deliberately misses this
 * shape: after the interpolation the copy often continues in lower case, and
 * demanding a capital is the only thing that keeps Tailwind class lists out.
 *
 * Restricting this rule to *interpolated* literals is what makes dropping the
 * capital safe. A technical value assembled from a variable is an id, a key or
 * a path — `quality-${x}`, `queue.status.${x}` — and those are single tokens
 * with no space in their literal text. Class lists reach here as `className`
 * templates and are filtered out by `TECHNICAL_PROPS` before this runs.
 */
function looksLikeInterpolatedCopy(value: string): boolean {
  if (!value.includes(INTERPOLATION)) return false;
  const literalOnly = value.split(INTERPOLATION).join("");
  if (letterCount(literalOnly) < 2) return false;
  if (!/\S\s+\S/.test(literalOnly.trim())) return false;
  return PROSE_CHARACTERS.test(value.split(INTERPOLATION).join(" Xx ").trim());
}

interface Offence {
  file: string;
  line: number;
  text: string;
  rule: string;
}

function findOffences(file: string, source: string): Offence[] {
  const { literals, code } = scan(source);
  const lineOf = lineIndex(source);
  const offences: Offence[] = [];
  const record = (line: number, text: string, rule: string) =>
    offences.push({ file, line, text, rule });

  for (const literal of literals) {
    const call = enclosingCall(code, literal.start);
    if (call === "t") continue; // already translated

    const readable = literal.value.split(INTERPOLATION).join("${…}");
    if (letterCount(literal.value) === 0) continue;

    const prop = propertyName(code, literal.start);
    if (prop !== null && TECHNICAL_PROPS.has(prop)) continue;

    if (prop !== null && USER_FACING_PROPS.has(prop)) {
      record(literal.line, readable, `\`${prop}\` is shown to the user`);
      continue;
    }

    if (call !== null && TOAST_CALL.test(call)) {
      record(literal.line, readable, `\`${call}()\` puts its argument on screen`);
      continue;
    }

    if (looksLikeProse(literal.value)) {
      record(literal.line, readable, "reads as UI copy");
      continue;
    }

    if (looksLikeInterpolatedCopy(literal.value)) {
      record(literal.line, readable, "reads as UI copy built around a value");
    }
  }

  // Rule 4: JSX text nodes — the words between a `>` and the next `<`. The
  // code view has string contents blanked, so attribute values cannot leak in
  // here; `{`/`}` end the run, so JSX expressions are left to the rules above.
  for (const match of code.matchAll(/>([^<>{}]*)</g)) {
    const text = match[1].trim();
    if (letterCount(text) < 2 || !PROSE_CHARACTERS.test(text)) continue;
    const offset = (match.index ?? 0) + 1 + match[1].indexOf(text.slice(0, 1));
    record(lineOf(offset), text, "JSX text node");
  }

  return offences;
}

/** `[path, source]` for every scanned file, path relative to the repo root. */
const FILES: Array<[string, string]> = Object.entries(SOURCES)
  .filter(([path]) => !path.includes(EXCLUDED_DIRECTORY_SEGMENT))
  .map(([path, source]): [string, string] => [path.replace(/^\//, ""), source])
  .sort(([a], [b]) => a.localeCompare(b));

function format(offences: Offence[]): string {
  return offences
    .sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line)
    .map((o) => `  ${o.file}:${o.line}  ${JSON.stringify(o.text)}  — ${o.rule}`)
    .join("\n");
}

describe("no hard-coded UI strings (FR-132)", () => {
  it("scans every UI source file", () => {
    // A silent zero here would make every assertion below vacuously true: an
    // `it.each([])` reports no failures at all.
    expect(FILES.length, "no source files found under src/components or src/pages").toBeGreaterThan(
      10,
    );
    expect(FILES.map(([path]) => path)).toContain("src/components/DownloadForm.tsx");
    expect(FILES.map(([path]) => path)).toContain("src/pages/History.tsx");
    // The exclusion must drop the shadcn primitives and nothing else.
    expect(FILES.filter(([path]) => path.includes("/ui/"))).toEqual([]);
    expect(FILES.every(([, source]) => source.length > 0)).toBe(true);
  });

  it.each(FILES)("%s routes all user-facing text through t()", (path, source) => {
    const offences = findOffences(path, source);

    expect(
      offences,
      offences.length === 0
        ? ""
        : `Hard-coded user-facing text (move it into src/locales/*.json and call t()):\n${format(offences)}\n`,
    ).toEqual([]);
  });

  it("has no t() call carrying a defaultValue that duplicates en.json", () => {
    const offences: Offence[] = [];

    for (const [path, source] of FILES) {
      const { literals, code } = scan(source);

      for (const literal of literals) {
        if (enclosingCall(code, literal.start) !== "t") continue;
        if (propertyName(code, literal.start) !== "defaultValue") continue;
        offences.push({
          file: path,
          line: literal.line,
          text: literal.value.split(INTERPOLATION).join("${…}"),
          rule: "inline defaultValue shadows en.json — delete it and let the key resolve",
        });
      }
    }

    expect(
      offences,
      offences.length === 0
        ? ""
        : `en.json must be the only source of English copy, but these t() calls carry their own:\n${format(offences)}\n`,
    ).toEqual([]);
  });
});
