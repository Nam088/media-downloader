import { describe, expect, it } from "vitest";

import en from "@/locales/en.json";
import vi from "@/locales/vi.json";

/**
 * FR-133: the build must fail when the locale files drift apart, instead of
 * letting a missing Vietnamese key fall back silently to English text.
 *
 * Every assertion below reports the offending key paths, not just a count —
 * "expected 111 to be 110" tells a reader nothing about what to fix.
 */

const LOCALES = [
  { code: "en", file: "src/locales/en.json", tree: en as unknown },
  { code: "vi", file: "src/locales/vi.json", tree: vi as unknown },
] as const;

/** i18next's default `pluralSeparator`; see src/lib/i18n.ts (it is not overridden). */
const PLURAL_SEPARATOR = "_";

/** The CLDR plural categories i18next appends after `pluralSeparator`. */
const PLURAL_CATEGORIES = ["zero", "one", "two", "few", "many", "other"] as const;
type PluralCategory = (typeof PLURAL_CATEGORIES)[number];

function isPluralCategory(value: string): value is PluralCategory {
  return (PLURAL_CATEGORIES as readonly string[]).includes(value);
}

/**
 * Flatten a nested translation object into `[dotted.key.path, leafValue]` pairs
 * so two locales can be compared regardless of declaration order.
 */
function flattenEntries(value: unknown, prefix = ""): Array<[string, unknown]> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return [[prefix, value]];
  }
  return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
    flattenEntries(child, prefix ? `${prefix}.${key}` : key),
  );
}

/**
 * Split `downloadForm.item_count_other` into base `downloadForm.item_count`
 * plus category `other`. Keys whose trailing segment is not a CLDR category
 * (`gallery_select_none`, `gallery_mode_files`, ...) are left untouched.
 */
function splitPlural(path: string): { base: string; category: PluralCategory | null } {
  const separatorIndex = path.lastIndexOf(PLURAL_SEPARATOR);
  if (separatorIndex === -1) {
    return { base: path, category: null };
  }
  const suffix = path.slice(separatorIndex + 1);
  return isPluralCategory(suffix)
    ? { base: path.slice(0, separatorIndex), category: suffix }
    : { base: path, category: null };
}

/** Plural categories the given language actually uses, straight from CLDR. */
function requiredCategories(languageCode: string): PluralCategory[] {
  return new Intl.PluralRules(languageCode)
    .resolvedOptions()
    .pluralCategories.filter(isPluralCategory);
}

interface LocaleIndex {
  code: string;
  file: string;
  /** Every leaf key path exactly as written in the file. */
  literalKeys: Set<string>;
  /** Leaf key paths with any plural suffix stripped. */
  baseKeys: Set<string>;
  /** Base key path -> plural categories declared for it in this file. */
  pluralForms: Map<string, Set<PluralCategory>>;
  entries: Array<[string, unknown]>;
}

const indexes: LocaleIndex[] = LOCALES.map(({ code, file, tree }) => {
  const entries = flattenEntries(tree);
  const pluralForms = new Map<string, Set<PluralCategory>>();
  const baseKeys = new Set<string>();

  for (const [path] of entries) {
    const { base, category } = splitPlural(path);
    baseKeys.add(base);
    if (category !== null) {
      const forms = pluralForms.get(base) ?? new Set<PluralCategory>();
      forms.add(category);
      pluralForms.set(base, forms);
    }
  }

  return {
    code,
    file,
    literalKeys: new Set(entries.map(([path]) => path)),
    baseKeys,
    pluralForms,
    entries,
  };
});

/** Base keys that any locale declares in plural form — checked per language below. */
const pluralBases = new Set(indexes.flatMap((index) => [...index.pluralForms.keys()]));

function indexFor(code: string): LocaleIndex {
  const found = indexes.find((index) => index.code === code);
  if (!found) throw new Error(`No locale indexed for "${code}"`);
  return found;
}

describe("locale parity (FR-133)", () => {
  /*
   * Parity is asserted on *base* keys, not on the literal suffixed keys.
   * English pluralises (`one`, `other`); Vietnamese has no grammatical plural
   * (`other` only). Demanding a byte-identical key set would force a dead
   * `_one` entry into vi.json that i18next can never select. Plural coverage
   * is therefore checked separately, per language, against CLDR.
   */
  it("has no key present in English but missing in Vietnamese", () => {
    const missing = [...indexFor("en").baseKeys]
      .filter((key) => !indexFor("vi").baseKeys.has(key))
      .sort();
    expect(missing, `Keys in en.json with no counterpart in vi.json: ${missing.join(", ")}`).toEqual(
      [],
    );
  });

  it("has no key present in Vietnamese but missing in English", () => {
    const missing = [...indexFor("vi").baseKeys]
      .filter((key) => !indexFor("en").baseKeys.has(key))
      .sort();
    expect(missing, `Keys in vi.json with no counterpart in en.json: ${missing.join(", ")}`).toEqual(
      [],
    );
  });

  it("declares every plural form the language's own CLDR rules require", () => {
    const problems: string[] = [];

    for (const index of indexes) {
      const required = requiredCategories(index.code);

      for (const base of pluralBases) {
        if (!index.baseKeys.has(base)) continue; // reported by the parity tests above

        const declared = index.pluralForms.get(base) ?? new Set<PluralCategory>();

        for (const category of required) {
          if (!declared.has(category)) {
            problems.push(`${index.file}: "${base}" is missing plural form "_${category}"`);
          }
        }

        for (const category of declared) {
          // `_zero` is an i18next opt-in extra that CLDR may not list; allow it.
          if (category !== "zero" && !required.includes(category)) {
            problems.push(
              `${index.file}: "${base}" declares plural form "_${category}", which ` +
                `"${index.code}" never selects (uses: ${required.join(", ")})`,
            );
          }
        }

        // A bare key sitting next to plural forms is dead weight: i18next
        // resolves the "_<category>" form first and only falls back to the
        // bare key when that form is absent.
        if (declared.size > 0 && index.literalKeys.has(base)) {
          problems.push(
            `${index.file}: "${base}" is declared both bare and with plural suffixes; ` +
              `the bare entry is unreachable`,
          );
        }
      }
    }

    expect(problems, problems.join("\n")).toEqual([]);
  });

  it("has no empty translation strings", () => {
    const empties = indexes
      .flatMap((index) =>
        index.entries
          .filter(([, value]) => typeof value !== "string" || value.trim() === "")
          .map(([path]) => `${index.file}: "${path}"`),
      )
      .sort();

    expect(empties, `Empty or non-string translations: ${empties.join(", ")}`).toEqual([]);
  });
});
