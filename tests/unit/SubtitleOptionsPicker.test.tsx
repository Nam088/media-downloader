import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SubtitleOptionsPicker } from "@/components/SubtitleOptionsPicker";
import { NEW_JOB_SUBTITLE_OPTIONS } from "@/types/download";
import type { SubtitleOptions, SubtitleTrackPreview } from "@/types/download";

const TRACKS: SubtitleTrackPreview[] = [
  { language: "vi", label: "Vietnamese", auto_generated: false },
  { language: "en", label: "English", auto_generated: false },
  { language: "ja", label: null, auto_generated: true },
];

const TRIGGER_TEST_ID = "subtitle-language-trigger";

/** The language checklist lives behind the select-style trigger now (FR-217
 * still holds — this just opens the popover that reveals it). */
async function openPicker(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByTestId(TRIGGER_TEST_ID));
}

/** Controlled component: without somewhere to keep what it emits, a click
 * would report a change that never comes back as a new `value`. */
function Harness({
  tracks,
  initial = NEW_JOB_SUBTITLE_OPTIONS,
  embedSupported = true,
  embedBlockedReasonKey = null,
  onChange,
}: {
  tracks: SubtitleTrackPreview[] | null | undefined;
  initial?: SubtitleOptions;
  embedSupported?: boolean;
  embedBlockedReasonKey?: string | null;
  onChange?: (next: SubtitleOptions) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <SubtitleOptionsPicker
      tracks={tracks}
      value={value}
      embedSupported={embedSupported}
      embedBlockedReasonKey={embedBlockedReasonKey}
      onChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
    />
  );
}

describe("SubtitleOptionsPicker (FR-217 → FR-221)", () => {
  /*
   * The whole reason `MediaSource.subtitles` is nullable. "Nobody checked" and
   * "checked, there are none" are different claims about the world, and an
   * empty list of checkboxes reads as neither — it reads as "still loading".
   */
  it("tells the three states of the subtitle list apart", async () => {
    const user = userEvent.setup();

    const unchecked = render(<Harness tracks={null} />);
    expect(screen.getByText(/never checked/i)).toBeInTheDocument();
    expect(screen.queryByTestId(TRIGGER_TEST_ID)).not.toBeInTheDocument();
    expect(screen.queryByText(/offers no subtitles/i)).not.toBeInTheDocument();
    unchecked.unmount();

    const none = render(<Harness tracks={[]} />);
    expect(screen.getByText(/offers no subtitles/i)).toBeInTheDocument();
    expect(screen.queryByTestId(TRIGGER_TEST_ID)).not.toBeInTheDocument();
    expect(screen.queryByText(/never checked/i)).not.toBeInTheDocument();
    none.unmount();

    render(<Harness tracks={TRACKS} />);
    expect(screen.queryByText(/never checked/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/offers no subtitles/i)).not.toBeInTheDocument();
    await openPicker(user);
    expect(screen.getAllByRole("checkbox")).toHaveLength(3);
  });

  // `undefined` reaches this component from an older preview fixture and from
  // the batch flow, where there is no single source to ask.
  it("treats a missing list the same as an unchecked one", () => {
    render(<Harness tracks={undefined} />);

    expect(screen.getByText(/never checked/i)).toBeInTheDocument();
    expect(screen.queryByTestId(TRIGGER_TEST_ID)).not.toBeInTheDocument();
  });

  it("lists only the languages the source really has (FR-217)", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    expect(screen.getByRole("checkbox", { name: /Vietnamese/ })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /English/ })).toBeInTheDocument();
    // No name is invented for a track the source left unnamed — the raw code
    // stands in (FR-211).
    expect(screen.getByRole("checkbox", { name: /ja/ })).toBeInTheDocument();
    // A language nobody offered must not be selectable from a fixed list.
    expect(screen.queryByRole("checkbox", { name: /French/i })).not.toBeInTheDocument();
  });

  it("marks author-provided and machine-made subtitles differently (FR-217)", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    expect(screen.getByRole("checkbox", { name: /Vietnamese.*From the author/i })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /ja.*Auto-generated/i })).toBeInTheDocument();
  });

  it("selects several languages at once (FR-218)", async () => {
    const changes: SubtitleOptions[] = [];
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} onChange={(next) => changes.push(next)} />);
    await openPicker(user);

    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    await user.click(screen.getByRole("checkbox", { name: /English/ }));

    expect(changes[changes.length - 1].languages).toEqual(["vi", "en"]);
    expect(screen.getByRole("checkbox", { name: /Vietnamese/ })).toBeChecked();
    expect(screen.getByRole("checkbox", { name: /English/ })).toBeChecked();
  });

  it("removes a language that is clicked a second time", async () => {
    const changes: SubtitleOptions[] = [];
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} onChange={(next) => changes.push(next)} />);
    await openPicker(user);

    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));

    expect(changes[changes.length - 1].languages).toEqual([]);
  });

  /*
   * The flag is one switch over the whole `--sub-langs` list, so it has to be
   * derived from the selection rather than set once and left: picking an
   * author-provided language after a machine-made one must not keep asking for
   * automatic captions.
   */
  it("asks for automatic captions only while a machine-made language is picked (FR-217)", async () => {
    const changes: SubtitleOptions[] = [];
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} onChange={(next) => changes.push(next)} />);
    await openPicker(user);

    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    expect(changes[changes.length - 1].include_auto_generated).toBe(false);

    await user.click(screen.getByRole("checkbox", { name: /ja/ }));
    expect(changes[changes.length - 1].include_auto_generated).toBe(true);

    await user.click(screen.getByRole("checkbox", { name: /ja/ }));
    expect(changes[changes.length - 1].include_auto_generated).toBe(false);
  });

  it("shows a placeholder on the trigger until a language is picked, then a count", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);

    expect(screen.getByTestId(TRIGGER_TEST_ID)).toHaveTextContent(/choose/i);

    await openPicker(user);
    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));

    expect(screen.getByTestId(TRIGGER_TEST_ID)).toHaveTextContent(/1/);

    await user.click(screen.getByRole("checkbox", { name: /English/ }));

    expect(screen.getByTestId(TRIGGER_TEST_ID)).toHaveTextContent(/2/);
  });

  it("filters the language list as the user types (search)", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    await user.type(screen.getByRole("textbox", { name: /search/i }), "vi");

    expect(screen.getByRole("checkbox", { name: /Vietnamese/ })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /English/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /^ja/ })).not.toBeInTheDocument();
  });

  it("matches by language code as well as by label (search)", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    await user.type(screen.getByRole("textbox", { name: /search/i }), "ja");

    expect(screen.getByRole("checkbox", { name: /^ja/ })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox", { name: /Vietnamese/ })).not.toBeInTheDocument();
  });

  it("shows a message when no language matches the search", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    await user.type(screen.getByRole("textbox", { name: /search/i }), "zzz-none");

    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    expect(screen.getByText(/no language matches/i)).toBeInTheDocument();
  });

  it("keeps a selection checked after the search that found it is cleared", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);
    await openPicker(user);

    const search = screen.getByRole("textbox", { name: /search/i });
    await user.type(search, "vi");
    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    await user.clear(search);

    expect(screen.getByRole("checkbox", { name: /Vietnamese/ })).toBeChecked();
  });

  it("has no trigger or search box when there is no language list to filter", () => {
    render(<Harness tracks={null} />);
    expect(screen.queryByTestId(TRIGGER_TEST_ID)).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /search/i })).not.toBeInTheDocument();

    cleanup();

    render(<Harness tracks={[]} />);
    expect(screen.queryByTestId(TRIGGER_TEST_ID)).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: /search/i })).not.toBeInTheDocument();
  });

  it("keeps the delivery choice out of the way until a language is picked", async () => {
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} />);

    expect(screen.queryByRole("radio", { name: /^separate files/i })).not.toBeInTheDocument();

    await openPicker(user);
    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));

    expect(screen.getByRole("radio", { name: /^separate files/i })).toBeInTheDocument();
  });

  it("lets the user choose embedded subtitles when the output can hold them (FR-219)", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Harness tracks={TRACKS} onChange={onChange} />);
    await openPicker(user);

    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));
    await user.click(screen.getByRole("radio", { name: /inside the file/i }));

    expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ delivery: "embedded", languages: ["vi"] }),
    );
  });

  // FR-220/SC-209: disabled with the reason attached, not hidden. A control
  // that vanishes leaves the user hunting for a feature the app does have.
  it("disables embedding with an explanation instead of hiding it (FR-220)", async () => {
    const user = userEvent.setup();
    render(
      <Harness
        tracks={TRACKS}
        embedSupported={false}
        embedBlockedReasonKey="downloadForm.subtitles_embed_unavailable_audio"
      />,
    );
    await openPicker(user);

    await user.click(screen.getByRole("checkbox", { name: /Vietnamese/ }));

    const embedded = screen.getByRole("radio", { name: /inside the file/i });
    expect(embedded).toBeInTheDocument();
    expect(embedded).toBeDisabled();
    expect(screen.getByText(/no subtitle track to hold them/i)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: /^separate files/i })).toBeChecked();
  });

  // A preset can carry `delivery: "embedded"` into a job whose output cannot
  // hold it; the picker must not draw that as the live choice.
  it("shows separate files as the live choice when embedding is impossible", () => {
    render(
      <Harness
        tracks={TRACKS}
        initial={{ languages: ["vi"], delivery: "embedded", include_auto_generated: false }}
        embedSupported={false}
        embedBlockedReasonKey="downloadForm.subtitles_embed_unavailable_source"
      />,
    );

    expect(screen.getByRole("radio", { name: /^separate files/i })).toBeChecked();
    expect(screen.getByRole("radio", { name: /inside the file/i })).not.toBeChecked();
  });
});
