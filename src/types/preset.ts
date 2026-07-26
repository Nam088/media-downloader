import type { OutputOptions } from "./download";

/** A saved, named `OutputOptions` with a flag marking one of them as the
 * default applied to every newly previewed link (FR-228 → FR-233).
 *
 * `output_options` is the **same** type a job carries, not a reduced copy of
 * it — a preset is a job's output options plus a name. Applying one is a plain
 * assignment, and adding an output option in a later slice needs no change
 * here. The backend stores it as the same JSON blob
 * `download_jobs.output_options` uses, so the two can never drift apart, and a
 * preset saved by an older version still loads with any newer option at its
 * default value (FR-233).
 *
 * The record comes back exactly as it was saved, never filtered against the
 * current source. That is what makes FR-231 possible: reconciling a preset's
 * quality with the format list the current source actually offers — picking
 * the nearest available and telling the user what changed — is the caller's
 * job, and it needs the unmodified saved value to compare against.
 *
 * Timestamps are RFC 3339 strings, same as `DownloadJob`. */
export interface Preset {
  id: string;
  name: string;
  output_options: OutputOptions;
  /** At most one preset in the list has this set. Enforced by a partial unique
   * index in the database, not by whichever call site last wrote it — and
   * `set_default_preset` clears the previous one in the same transaction.
   *
   * No preset being the default is a valid state: it is what a fresh install
   * looks like, and what remains after the default preset is deleted (deleting
   * it never promotes another one). Handle the "none" case. */
  is_default: boolean;
  created_at: string;
  updated_at: string;
}

/** Tauri commands backing the preset list. Argument names are as seen from JS
 * — Tauri camelCases them, so Rust's `preset_id` is `presetId` here:
 *
 * - `list_presets` — no args → `Preset[]`, sorted by name.
 * - `create_preset` — `{ name, options }` → `Preset`. Never the default;
 *   promoting it is a separate, explicit act.
 * - `rename_preset` — `{ presetId, name }` → `Preset`.
 * - `update_preset` — `{ presetId, options }` → `Preset`. Overwrites the whole
 *   options blob rather than patching fields.
 * - `delete_preset` — `{ presetId }` → `void`.
 * - `set_default_preset` — `{ presetId }` → `Preset`.
 *
 * Error codes worth translating: `PRESET_NAME_TAKEN` (names are unique;
 * surrounding whitespace is trimmed before the check), `PRESET_NAME_REQUIRED`
 * (empty or whitespace-only name), `NOT_FOUND` (the preset was deleted from
 * under this window). */
export const PRESET_COMMANDS = {
  list: "list_presets",
  create: "create_preset",
  rename: "rename_preset",
  update: "update_preset",
  delete: "delete_preset",
  setDefault: "set_default_preset",
} as const;
