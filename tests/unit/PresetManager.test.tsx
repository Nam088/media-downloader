import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";

import { PresetManager } from "@/components/PresetManager";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type { MediaSource, MediaType, OutputOptions } from "@/types/download";
import type { Preset } from "@/types/preset";

function presetWith(overrides: Partial<Preset> = {}): Preset {
  return {
    id: "preset-1",
    name: "Archive 320",
    output_options: { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 320 } },
    is_default: false,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    ...overrides,
  };
}

/** A link that offers 128 kbps and nothing else — so a 320 kbps preset cannot
 * apply verbatim and FR-231 has something real to report. */
function sourceWith(overrides: Partial<MediaSource> = {}): MediaSource {
  return {
    source_url: "https://youtube.com/watch?v=abc",
    title: "Sample",
    thumbnail_url: null,
    duration_seconds: 300,
    platform: "youtube",
    is_playlist: false,
    playlist_item_count: null,
    available_video_qualities: [],
    available_audio_formats: [{ bitrate_kbps: 128, codec: "opus", filesize_bytes: null }],
    is_gallery: false,
    gallery_items: [],
    playlist_entries: [],
    subtitles: [],
    chapters: [],
    ...overrides,
  };
}

function Harness({
  source = sourceWith(),
  mediaType = "audio",
  initial = NEW_JOB_OUTPUT_OPTIONS,
  onApply,
}: {
  source?: MediaSource | null;
  mediaType?: MediaType;
  initial?: OutputOptions;
  onApply?: (next: OutputOptions) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <PresetManager
      value={value}
      mediaType={mediaType}
      source={source}
      onApply={(next) => {
        setValue(next);
        onApply?.(next);
      }}
    />
  );
}

/** Answers `list_presets` with `presets`, and every write with the row the
 * caller would have got back. */
function mockBackend(presets: Preset[], overrides: Record<string, unknown> = {}) {
  vi.mocked(invoke).mockImplementation((cmd: string) => {
    if (cmd === "list_presets") return Promise.resolve(presets);
    if (cmd in overrides) {
      const answer = overrides[cmd];
      return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
    }
    return Promise.resolve(undefined);
  });
}

function argsFor(command: string): Record<string, unknown> | undefined {
  const call = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === command);
  return call?.[1] as Record<string, unknown> | undefined;
}

describe("PresetManager (FR-228 → FR-233)", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("lists the saved presets", async () => {
    mockBackend([presetWith(), presetWith({ id: "preset-2", name: "Phone MP4" })]);
    render(<Harness />);

    expect(await screen.findByRole("option", { name: "Archive 320" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Phone MP4" })).toBeInTheDocument();
  });

  it("saves the options on screen under a name (FR-228)", async () => {
    mockBackend([], { create_preset: presetWith({ id: "new-1", name: "My preset" }) });
    const user = userEvent.setup();
    render(<Harness initial={{ ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "flac" } }} />);

    await user.click(screen.getByRole("button", { name: /save as preset/i }));
    await user.type(screen.getByLabelText(/preset name/i), "My preset");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    await waitFor(() => expect(argsFor("create_preset")).toBeDefined());
    expect(argsFor("create_preset")).toEqual({
      name: "My preset",
      options: { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "flac" } },
    });
  });

  it("explains a duplicate name and keeps the box open (FR-229)", async () => {
    mockBackend([presetWith()], {
      create_preset: Object.assign(new Error("taken"), {
        code: "PRESET_NAME_TAKEN",
        message: "taken",
      }),
    });
    const user = userEvent.setup();
    render(<Harness />);

    await user.click(screen.getByRole("button", { name: /save as preset/i }));
    await user.type(screen.getByLabelText(/preset name/i), "Archive 320");
    await user.click(screen.getByRole("button", { name: /^save$/i }));

    expect(await screen.findByText(/preset with that name already exists/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/preset name/i)).toHaveValue("Archive 320");
  });

  it("renames, updates, sets a default and deletes (FR-229/FR-230)", async () => {
    mockBackend([presetWith()], {
      rename_preset: presetWith({ name: "Renamed" }),
      update_preset: presetWith(),
      set_default_preset: presetWith({ is_default: true }),
    });
    const user = userEvent.setup();
    render(<Harness />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    await user.click(screen.getByRole("button", { name: /update with what is on screen/i }));
    await waitFor(() => expect(argsFor("update_preset")).toBeDefined());
    expect(argsFor("update_preset")).toEqual({
      presetId: "preset-1",
      // The whole blob, not a patch — that is the command's contract. And it
      // is what is *on screen* (128 kbps, after this link forced the fallback),
      // not the 320 kbps still stored under this name.
      options: { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 128 } },
    });

    await user.click(screen.getByRole("button", { name: /make default/i }));
    await waitFor(() => expect(argsFor("set_default_preset")).toEqual({ presetId: "preset-1" }));

    await user.click(screen.getByRole("button", { name: /^rename$/i }));
    await user.clear(screen.getByLabelText(/preset name/i));
    await user.type(screen.getByLabelText(/preset name/i), "Renamed");
    await user.click(screen.getByRole("button", { name: /save new name/i }));
    await waitFor(() =>
      expect(argsFor("rename_preset")).toEqual({ presetId: "preset-1", name: "Renamed" }),
    );

    await user.click(screen.getByRole("button", { name: /delete/i }));
    await waitFor(() => expect(argsFor("delete_preset")).toEqual({ presetId: "preset-1" }));
  });

  it("applies a preset's options to the picker (SC-208)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([
      presetWith({
        output_options: { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 128 } },
      }),
    ]);
    const user = userEvent.setup();
    render(<Harness onApply={(next) => applied.push(next)} />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(applied).toEqual([
      { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 128 } },
    ]);
  });

  /*
   * FR-231. The backend hands presets back exactly as they were saved, so this
   * is the only place that can notice a preset asking for a quality this link
   * does not have. Applying 320 kbps to a 128-kbps-only link and saying nothing
   * would leave a preset called "Archive 320" quietly producing something else.
   */
  it("falls back to the nearest available quality and says what it changed (FR-231)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([presetWith()]); // 320 kbps preset, 128 kbps link
    const user = userEvent.setup();
    render(<Harness onApply={(next) => applied.push(next)} />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(applied[0].audio).toEqual({ format: "mp3", bitrate_kbps: 128 });
    expect(screen.getByText(/did not fit this link exactly/i)).toBeInTheDocument();
    expect(
      screen.getByText(/does not offer 320 kbps, so the closest it has — 128 kbps — was used/i),
    ).toBeInTheDocument();
  });

  it("says nothing about changes when the preset fitted the link (FR-231)", async () => {
    mockBackend([
      presetWith({
        output_options: { ...NEW_JOB_OUTPUT_OPTIONS, audio: { format: "mp3", bitrate_kbps: 128 } },
      }),
    ]);
    const user = userEvent.setup();
    render(<Harness />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(screen.queryByText(/did not fit this link exactly/i)).not.toBeInTheDocument();
  });

  it("reports a dropped subtitle language as well as a quality (FR-231)", async () => {
    mockBackend([
      presetWith({
        output_options: {
          ...NEW_JOB_OUTPUT_OPTIONS,
          audio: { format: "mp3", bitrate_kbps: 128 },
          subtitles: {
            languages: ["fr"],
            delivery: "separate_files",
            include_auto_generated: false,
          },
        },
      }),
    ]);
    const user = userEvent.setup();
    render(
      <Harness
        source={sourceWith({
          subtitles: [{ language: "vi", label: "Vietnamese", auto_generated: false }],
        })}
      />,
    );

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(screen.getByText(/no subtitles in fr/i)).toBeInTheDocument();
  });

  // FR-230: the default is applied to a newly previewed link on its own, with
  // no click at all.
  it("applies the default preset to a newly previewed link (FR-230)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([presetWith({ is_default: true })]);
    render(<Harness onApply={(next) => applied.push(next)} />);

    await waitFor(() => expect(applied).toHaveLength(1));
    expect(applied[0].audio).toEqual({ format: "mp3", bitrate_kbps: 128 });
  });

  it("does not re-apply the default over edits made to the same link (FR-230)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([presetWith({ is_default: true })]);
    const { rerender } = render(<Harness onApply={(next) => applied.push(next)} />);

    await waitFor(() => expect(applied).toHaveLength(1));
    rerender(<Harness onApply={(next) => applied.push(next)} />);

    await waitFor(() => expect(applied).toHaveLength(1));
  });

  it("applies nothing on its own when no preset is the default (FR-230)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([presetWith()]);
    render(<Harness onApply={(next) => applied.push(next)} />);

    expect(await screen.findByRole("option", { name: "Archive 320" })).toBeInTheDocument();
    expect(applied).toEqual([]);
  });

  // Deleting the default leaves no default at all, which the backend allows on
  // purpose. It has to read as a state, not as a blank space.
  it("says so when nothing is the default", async () => {
    mockBackend([presetWith()]);
    render(<Harness />);

    expect(await screen.findByText(/no preset is the default/i)).toBeInTheDocument();
  });

  it("stops saying so once a preset is the default", async () => {
    mockBackend([presetWith({ is_default: true })]);
    render(<Harness />);

    expect(await screen.findByRole("option", { name: /Archive 320 \(default\)/ })).toBeInTheDocument();
    expect(screen.queryByText(/no preset is the default/i)).not.toBeInTheDocument();
  });

  it("shows nothing but the empty state before anything is saved", async () => {
    mockBackend([]);
    render(<Harness />);

    expect(await screen.findByRole("option", { name: /no presets saved yet/i })).toBeInTheDocument();
    expect(screen.queryByText(/no preset is the default/i)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /delete/i })).not.toBeInTheDocument();
  });

  // A preset saved before a later option existed comes back without it, and
  // must still apply — the missing option simply keeps its default (FR-233).
  it("applies a preset saved without the newer options (FR-233)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([
      presetWith({
        output_options: {
          audio: { format: "m4a" },
          video_container: "mp4",
          codec_preference: "compatibility",
          embed_metadata: true,
          embed_thumbnail: true,
        },
      }),
    ]);
    const user = userEvent.setup();
    render(<Harness onApply={(next) => applied.push(next)} />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(applied[0]).toEqual({
      audio: { format: "m4a" },
      video_container: "mp4",
      codec_preference: "compatibility",
      embed_metadata: true,
      embed_thumbnail: true,
    });
    expect(applied[0].subtitles).toBeUndefined();
    expect(applied[0].segment).toBeUndefined();
  });

  // FR-232: the batch has no single link, so there is nothing to reconcile
  // against and the preset must land unchanged rather than being fitted to a
  // source that isn't there.
  it("applies a preset verbatim for a batch of links (FR-232)", async () => {
    const applied: OutputOptions[] = [];
    mockBackend([presetWith()]);
    const user = userEvent.setup();
    render(<Harness source={null} onApply={(next) => applied.push(next)} />);

    await user.selectOptions(await screen.findByLabelText(/presets/i), "preset-1");

    expect(applied[0].audio).toEqual({ format: "mp3", bitrate_kbps: 320 });
    expect(screen.queryByText(/did not fit this link exactly/i)).not.toBeInTheDocument();
  });
});
