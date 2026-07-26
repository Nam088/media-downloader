import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Bookmark, Info, Star } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ErrorBanner } from "@/components/ErrorBanner";
import { presetAdjustmentMessage, reconcilePresetOptions } from "@/lib/preset-reconcile";
import type { PresetAdjustment } from "@/lib/preset-reconcile";
import { PRESET_COMMANDS } from "@/types/preset";
import type { Preset } from "@/types/preset";
import type { AppError, MediaSource, MediaType, OutputOptions } from "@/types/download";

/** Value of the "no preset" entry in the picker. Not `""`: an empty option
 * value is indistinguishable from a preset whose id failed to load. */
const NO_PRESET = "__none__";

type NameEditor = { kind: "create" } | { kind: "rename"; preset: Preset } | null;

/** One backend call plus the refresh that follows it. Written as a call
 * signature rather than an inline arrow type so the source carries no `=>
 * Promise<...>` for the hard-coded-string scanner to read as JSX text. */
interface PresetAction {
  (): Promise<void>;
}

/** The saved presets, already sorted by the backend. An older backend — or a
 * command mock — can answer with nothing, and an array is the only shape worth
 * putting into state. */
async function fetchPresets(): Promise<Preset[]> {
  const list = await invoke<Preset[]>(PRESET_COMMANDS.list);
  return Array.isArray(list) ? list : [];
}

export interface PresetManagerProps {
  /** The options currently in the picker — what "save" and "update" write. */
  value: OutputOptions;
  /** Applies a preset's (reconciled) options to the picker. */
  onApply: (next: OutputOptions) => void;
  /** Decides which halves of a preset can even be reconciled (FR-231). */
  mediaType: MediaType;
  /** The link on screen. `null` for a batch of several links, which has no
   * single format list — a preset then applies verbatim, and the default
   * preset is not auto-applied because there is no "newly previewed link". */
  source?: MediaSource | null;
}

/**
 * Saved output presets (FR-228 → FR-233).
 *
 * Lives inside the collapsed advanced section, per the spec's own assumption
 * that ordinary users never open it. Everything here is one round-trip to the
 * backend, which owns uniqueness of names and the "exactly one default" rule —
 * this component never decides either, it just renders what came back.
 */
export function PresetManager({ value, onApply, mediaType, source }: PresetManagerProps) {
  const { t } = useTranslation();
  const [presets, setPresets] = useState<Preset[]>([]);
  const [selectedId, setSelectedId] = useState<string>(NO_PRESET);
  const [adjustments, setAdjustments] = useState<PresetAdjustment[]>([]);
  const [editor, setEditor] = useState<NameEditor>(null);
  const [nameDraft, setNameDraft] = useState("");
  const [error, setError] = useState<AppError | null>(null);
  const [busy, setBusy] = useState(false);

  const selected = presets.find((preset) => preset.id === selectedId) ?? null;
  // No preset being the default is a real state — a fresh install, or what is
  // left after the default one is deleted — so it gets its own line rather
  // than an empty space that reads as a rendering bug.
  const hasDefault = presets.some((preset) => preset.is_default);

  const refresh = useCallback(async () => {
    setPresets(await fetchPresets());
  }, []);

  /** FR-231 lives here: a preset is stored verbatim, so what the user actually
   * gets is the preset *fitted to this link*, plus a note about every place
   * the two disagreed. */
  const apply = useCallback(
    (preset: Preset, announce: boolean) => {
      const reconciled = reconcilePresetOptions(preset.output_options, { mediaType, source });
      onApply(reconciled.options);
      setSelectedId(preset.id);
      setAdjustments(reconciled.adjustments);
      if (announce) {
        toast.success(t("downloadForm.presets_applied", { name: preset.name }));
      }
    },
    [mediaType, onApply, source, t],
  );

  // Read through a ref by the effect below, so re-rendering with a fresh
  // `onApply` callback cannot restart the fetch — an effect that reloads the
  // list on every render would loop through its own `setPresets`.
  const applyRef = useRef(apply);
  useEffect(() => {
    applyRef.current = apply;
  }, [apply]);

  // FR-230 — the default preset is applied to every newly previewed link, and
  // to each link only once, so it never overwrites edits the user made after
  // it landed. Keyed on the URL rather than on a mount, because this component
  // stays mounted across previews; the list is re-read at the same time, which
  // also picks up presets saved since the last look.
  const autoAppliedFor = useRef<string | null>(null);
  const previewedUrl = source?.source_url ?? null;
  useEffect(() => {
    let cancelled = false;
    fetchPresets().then(
      (list) => {
        if (cancelled) return;
        setPresets(list);
        if (previewedUrl === null || autoAppliedFor.current === previewedUrl) return;
        const fallback = list.find((preset) => preset.is_default);
        if (!fallback) return;
        autoAppliedFor.current = previewedUrl;
        applyRef.current(fallback, false);
      },
      (error) => {
        if (!cancelled) setError(error as AppError);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [previewedUrl]);

  async function run(action: PresetAction) {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (err) {
      setError(err as AppError);
    } finally {
      setBusy(false);
    }
  }

  function handleSelect(id: string) {
    if (id === NO_PRESET) {
      setSelectedId(NO_PRESET);
      setAdjustments([]);
      return;
    }
    const preset = presets.find((candidate) => candidate.id === id);
    if (preset) apply(preset, true);
  }

  function startCreate() {
    setEditor({ kind: "create" });
    setNameDraft("");
    setError(null);
  }

  function startRename(preset: Preset) {
    setEditor({ kind: "rename", preset });
    setNameDraft(preset.name);
    setError(null);
  }

  async function commitName() {
    if (!editor) return;
    await run(async () => {
      if (editor.kind === "create") {
        const created = await invoke<Preset>(PRESET_COMMANDS.create, {
          name: nameDraft,
          options: value,
        });
        await refresh();
        setSelectedId(created.id);
        setAdjustments([]);
        toast.success(t("downloadForm.presets_saved", { name: created.name }));
      } else {
        const renamed = await invoke<Preset>(PRESET_COMMANDS.rename, {
          presetId: editor.preset.id,
          name: nameDraft,
        });
        await refresh();
        toast.success(t("downloadForm.presets_renamed", { name: renamed.name }));
      }
      // Closed only on success: a rejected name (taken, empty) leaves the box
      // open with the text still in it, next to the reason.
      setEditor(null);
    });
  }

  async function handleUpdate() {
    if (!selected) return;
    await run(async () => {
      await invoke<Preset>(PRESET_COMMANDS.update, { presetId: selected.id, options: value });
      await refresh();
      setAdjustments([]);
      toast.success(t("downloadForm.presets_updated", { name: selected.name }));
    });
  }

  async function handleDelete() {
    if (!selected) return;
    await run(async () => {
      await invoke(PRESET_COMMANDS.delete, { presetId: selected.id });
      await refresh();
      setSelectedId(NO_PRESET);
      setAdjustments([]);
      toast.success(t("downloadForm.presets_deleted", { name: selected.name }));
    });
  }

  async function handleSetDefault() {
    if (!selected) return;
    await run(async () => {
      await invoke<Preset>(PRESET_COMMANDS.setDefault, { presetId: selected.id });
      await refresh();
      toast.success(t("downloadForm.presets_default_set", { name: selected.name }));
    });
  }

  return (
    <div className="flex flex-col gap-2.5 rounded-md border border-border/70 bg-muted/20 p-3">
      <div className="flex items-center gap-2">
        <Bookmark className="h-4 w-4 shrink-0 text-primary" />
        <Label htmlFor="preset-select" className="text-xs font-semibold tracking-tight text-foreground/80">
          {t("downloadForm.presets_label")}
        </Label>
      </div>

      <ErrorBanner error={error} onDismiss={() => setError(null)} />

      <div className="flex flex-wrap items-center gap-2">
        <select
          id="preset-select"
          value={selectedId}
          disabled={busy}
          onChange={(event) => handleSelect(event.target.value)}
          className="h-9 min-w-40 flex-1 rounded-md border border-border/80 bg-card px-2.5 text-xs"
        >
          <option value={NO_PRESET}>
            {presets.length === 0
              ? t("downloadForm.presets_none_saved")
              : t("downloadForm.presets_choose")}
          </option>
          {presets.map((preset) => (
            <option key={preset.id} value={preset.id}>
              {preset.is_default
                ? t("downloadForm.presets_option_default", { name: preset.name })
                : preset.name}
            </option>
          ))}
        </select>

        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={startCreate}
          className="h-9 rounded-md text-xs font-semibold"
        >
          {t("downloadForm.presets_save_button")}
        </Button>
      </div>

      {selected && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <button
            type="button"
            disabled={busy}
            onClick={() => void handleUpdate()}
            className="text-xs font-semibold text-primary hover:underline disabled:opacity-50"
          >
            {t("downloadForm.presets_update_button")}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() => startRename(selected)}
            className="text-xs font-semibold text-primary hover:underline disabled:opacity-50"
          >
            {t("downloadForm.presets_rename_button")}
          </button>
          {!selected.is_default && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void handleSetDefault()}
              className="inline-flex items-center gap-1 text-xs font-semibold text-primary hover:underline disabled:opacity-50"
            >
              <Star className="h-3 w-3" />
              {t("downloadForm.presets_set_default_button")}
            </button>
          )}
          <button
            type="button"
            disabled={busy}
            onClick={() => void handleDelete()}
            className="text-xs font-semibold text-destructive hover:underline disabled:opacity-50"
          >
            {t("downloadForm.presets_delete_button")}
          </button>
        </div>
      )}

      {editor && (
        <div className="flex flex-wrap items-center gap-2">
          <Input
            aria-label={t("downloadForm.presets_name_label")}
            value={nameDraft}
            autoFocus
            onChange={(event) => setNameDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void commitName();
              if (event.key === "Escape") setEditor(null);
            }}
            placeholder={t("downloadForm.presets_name_placeholder")}
            className="h-9 min-w-40 flex-1"
          />
          <Button
            type="button"
            size="sm"
            disabled={busy}
            onClick={() => void commitName()}
            className="h-9 rounded-md text-xs font-semibold"
          >
            {editor.kind === "create"
              ? t("downloadForm.presets_confirm_save")
              : t("downloadForm.presets_confirm_rename")}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setEditor(null)}
            className="h-9 rounded-md text-xs font-semibold"
          >
            {t("common.cancel")}
          </Button>
        </div>
      )}

      {presets.length > 0 && !hasDefault && (
        <p className="text-xs text-muted-foreground">{t("downloadForm.presets_no_default_hint")}</p>
      )}

      {/* FR-231 — what the preset asked for, and what this link could give.
          Sticks around after the toast has gone, because it describes the
          options now on screen. */}
      {adjustments.length > 0 && (
        <div className="flex flex-col gap-1 rounded-md border border-amber-500/40 bg-amber-500/10 p-2.5">
          <span className="flex items-center gap-1.5 text-xs font-semibold text-amber-700 dark:text-amber-400">
            <Info className="h-3.5 w-3.5 shrink-0" />
            {t("downloadForm.presets_adjusted_title")}
          </span>
          <ul className="flex list-disc flex-col gap-0.5 pl-6 text-xs text-foreground/80">
            {adjustments.map((adjustment) => (
              <li key={adjustment.kind}>{presetAdjustmentMessage(t, adjustment)}</li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
