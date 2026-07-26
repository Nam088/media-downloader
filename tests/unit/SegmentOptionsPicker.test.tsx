import { useState } from "react";
import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { SegmentOptionsPicker } from "@/components/SegmentOptionsPicker";
import { parseTimeInput, trimErrorFor } from "@/lib/trim-input";
import { NEW_JOB_OUTPUT_OPTIONS, NEW_JOB_SEGMENT_MODE } from "@/types/download";
import type { ChapterPreview, SegmentMode } from "@/types/download";

const CHAPTERS: ChapterPreview[] = [
  { title: "Intro", start_seconds: 0, end_seconds: 30 },
  { title: null, start_seconds: 30, end_seconds: 90 },
  { title: "Outro", start_seconds: 90, end_seconds: 120 },
];

function Harness({
  chapters,
  durationSeconds,
  initial = NEW_JOB_SEGMENT_MODE,
  onChange,
}: {
  chapters: ChapterPreview[] | null | undefined;
  durationSeconds?: number | null;
  initial?: SegmentMode;
  onChange?: (next: SegmentMode) => void;
}) {
  const [value, setValue] = useState(initial);
  return (
    <SegmentOptionsPicker
      value={value}
      chapters={chapters}
      durationSeconds={durationSeconds}
      onChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
    />
  );
}

async function chooseTrim(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("radio", { name: /just one part/i }));
}

describe("SegmentOptionsPicker (FR-222 → FR-227)", () => {
  // FR-225. Three states again: a count when there are chapters, and two
  // different explanations for the two ways there can be none.
  it("offers the chapter split only when the source really has chapters (FR-225)", () => {
    const withChapters = render(<Harness chapters={CHAPTERS} />);
    expect(screen.getByRole("radio", { name: /split into 3 chapters/i })).toBeEnabled();
    withChapters.unmount();

    const noChapters = render(<Harness chapters={[]} />);
    const disabled = screen.getByRole("radio", { name: /one file per chapter/i });
    expect(disabled).toBeInTheDocument();
    expect(disabled).toBeDisabled();
    expect(screen.getByText(/no chapter list/i)).toBeInTheDocument();
    noChapters.unmount();

    render(<Harness chapters={null} />);
    expect(screen.getByRole("radio", { name: /one file per chapter/i })).toBeDisabled();
    // Not the same sentence: "we didn't look" is not "there are none".
    expect(screen.getByText(/never checked for this link/i)).toBeInTheDocument();
    expect(screen.queryByText(/no chapter list/i)).not.toBeInTheDocument();
  });

  it("shows the number of chapters it would produce (FR-225)", () => {
    render(<Harness chapters={CHAPTERS} />);

    expect(screen.getByRole("radio", { name: /split into 3 chapters/i })).toBeInTheDocument();
  });

  it("emits the chapter split when it is picked", async () => {
    const user = userEvent.setup();
    const changes: SegmentMode[] = [];
    render(<Harness chapters={CHAPTERS} onChange={(next) => changes.push(next)} />);

    await user.click(screen.getByRole("radio", { name: /split into 3 chapters/i }));

    expect(changes).toEqual([{ mode: "split_chapters" }]);
  });

  /*
   * FR-226 is enforced by the type, not by this component — `SegmentMode` has
   * nowhere to put a range and a chapter split at once. What the UI owes is
   * that picking one visibly puts the other away, so the rule is legible.
   */
  it("swaps the trim fields out when the chapter split is picked (FR-226)", async () => {
    const user = userEvent.setup();
    render(<Harness chapters={CHAPTERS} />);

    await chooseTrim(user);
    expect(screen.getByLabelText(/start at/i)).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /split into 3 chapters/i }));
    expect(screen.queryByLabelText(/start at/i)).not.toBeInTheDocument();
  });

  it("accepts m:ss, h:mm:ss and plain seconds for a bound (FR-222)", async () => {
    const user = userEvent.setup();
    const changes: SegmentMode[] = [];
    render(<Harness chapters={null} onChange={(next) => changes.push(next)} />);

    await chooseTrim(user);
    await user.type(screen.getByLabelText(/start at/i), "1:30");
    await user.type(screen.getByLabelText(/end at/i), "2:00");

    expect(changes[changes.length - 1]).toMatchObject({
      mode: "trim",
      start_seconds: 90,
      end_seconds: 120,
    });
  });

  it("names the reason a range is unusable, at the field (FR-223)", async () => {
    const user = userEvent.setup();
    render(<Harness chapters={null} />);

    await chooseTrim(user);
    // Nothing typed yet: an empty range is not a range.
    expect(screen.getByRole("alert")).toHaveTextContent(/enter a start time/i);

    await user.type(screen.getByLabelText(/start at/i), "2:00");
    await user.type(screen.getByLabelText(/end at/i), "1:00");
    expect(screen.getByRole("alert")).toHaveTextContent(/end time must come after/i);

    await user.clear(screen.getByLabelText(/end at/i));
    await user.type(screen.getByLabelText(/end at/i), "3:00");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("rejects a start time past the end of the content (FR-223)", async () => {
    const user = userEvent.setup();
    render(<Harness chapters={null} durationSeconds={100} />);

    await chooseTrim(user);
    await user.type(screen.getByLabelText(/start at/i), "3:00");

    expect(screen.getByRole("alert")).toHaveTextContent(/past the end of this content/i);
  });

  it("keeps text that is not a time visible and blocks on it", async () => {
    const user = userEvent.setup();
    const changes: SegmentMode[] = [];
    render(<Harness chapters={null} onChange={(next) => changes.push(next)} />);

    await chooseTrim(user);
    await user.type(screen.getByLabelText(/start at/i), "soon");

    expect(screen.getByLabelText(/start at/i)).toHaveValue("soon");
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(trimErrorFor({ ...NEW_JOB_OUTPUT_OPTIONS, segment: changes[changes.length - 1] })).not.toBeNull();
  });

  // FR-224 — the option exists, and its cost is stated on the control itself.
  it("warns that an exact cut takes longer (FR-224)", async () => {
    const user = userEvent.setup();
    const changes: SegmentMode[] = [];
    render(<Harness chapters={null} onChange={(next) => changes.push(next)} />);

    await chooseTrim(user);
    expect(screen.getByText(/noticeably longer/i)).toBeInTheDocument();

    await user.click(screen.getByRole("switch", { name: /cut exactly/i }));

    expect(changes[changes.length - 1]).toMatchObject({ mode: "trim", accurate_cut: true });
  });

  it("shows a range that came in as a prop, so an applied preset is visible", () => {
    render(
      <Harness
        chapters={null}
        initial={{ mode: "trim", start_seconds: 65, end_seconds: 3723, accurate_cut: false }}
      />,
    );

    expect(screen.getByLabelText(/start at/i)).toHaveValue("01:05");
    expect(screen.getByLabelText(/end at/i)).toHaveValue("1:02:03");
  });
});

describe("parseTimeInput", () => {
  it("reads the three shapes a user might type", () => {
    expect(parseTimeInput("90")).toBe(90);
    expect(parseTimeInput("1:30")).toBe(90);
    expect(parseTimeInput("1:02:03")).toBe(3723);
    expect(parseTimeInput(" 2:00 ")).toBe(120);
  });

  it("reports an empty box as no bound at all, not as zero", () => {
    // Zero is a real start time ("from the beginning, explicitly"); an empty
    // box means the bound is simply not set, and `validateTrimRange` treats a
    // range with neither bound as "the whole thing".
    expect(parseTimeInput("")).toBeNull();
    expect(parseTimeInput("   ")).toBeNull();
    expect(parseTimeInput("0")).toBe(0);
  });

  it("refuses text that is not a time", () => {
    for (const raw of ["soon", "1:2:3:4", "-5", "1:", "half past"]) {
      expect(Number.isNaN(parseTimeInput(raw) as number)).toBe(true);
    }
  });
});

describe("trimErrorFor", () => {
  it("is silent for every mode that carries no range", () => {
    expect(trimErrorFor(NEW_JOB_OUTPUT_OPTIONS)).toBeNull();
    expect(trimErrorFor({ ...NEW_JOB_OUTPUT_OPTIONS, segment: { mode: "whole" } })).toBeNull();
    expect(
      trimErrorFor({ ...NEW_JOB_OUTPUT_OPTIONS, segment: { mode: "split_chapters" } }),
    ).toBeNull();
  });

  it("reports the same reason the picker shows", () => {
    expect(
      trimErrorFor({
        ...NEW_JOB_OUTPUT_OPTIONS,
        segment: { mode: "trim", start_seconds: 120, end_seconds: 60 },
      }),
    ).toBe("end_before_start");
    expect(
      trimErrorFor(
        { ...NEW_JOB_OUTPUT_OPTIONS, segment: { mode: "trim", start_seconds: 300 } },
        100,
      ),
    ).toBe("beyond_duration");
  });
});
