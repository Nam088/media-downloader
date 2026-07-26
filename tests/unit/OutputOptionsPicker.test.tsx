import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { OutputOptionsPicker } from "@/components/OutputOptionsPicker";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type { MediaType, OutputOptions } from "@/types/download";

/** The picker is controlled, so every test needs something to hold the state
 * it emits — otherwise clicking a radio would report a change that never
 * comes back as a new `value`, and half of these assertions would be testing a
 * component frozen on its initial props. */
function Harness({
  mediaType = "audio",
  initial = NEW_JOB_OUTPUT_OPTIONS,
  onChange,
}: {
  mediaType?: MediaType;
  initial?: OutputOptions;
  onChange?: (next: OutputOptions) => void;
}) {
  const [value, setValue] = useState<OutputOptions>(initial);
  return (
    <OutputOptionsPicker
      mediaType={mediaType}
      value={value}
      onChange={(next) => {
        setValue(next);
        onChange?.(next);
      }}
    />
  );
}

async function openPanel(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: /output options/i }));
}

describe("OutputOptionsPicker", () => {
  it("offers every audio format the spec requires (FR-201)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await openPanel(user);

    for (const label of [/^MP3$/, /M4A \/ AAC/, /^Opus$/, /^WAV$/, /^FLAC$/, /keep source/i]) {
      expect(screen.getByRole("radio", { name: label })).toBeInTheDocument();
    }
  });

  // FR-203. The bitrate control is driven by `supportsBitrate`, which is the
  // same predicate the type itself encodes (WAV/FLAC/source have no
  // `bitrate_kbps` field at all), so this cannot drift into "shown but
  // ignored".
  it("drops the bitrate control once a lossless format is chosen (FR-203)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await openPanel(user);

    // MP3 is lossy: the control is there to begin with, so its later absence
    // means something.
    expect(screen.getByRole("radio", { name: /320 kbps/i })).toBeInTheDocument();

    await user.click(screen.getByRole("radio", { name: /^FLAC$/ }));

    expect(screen.queryByRole("radio", { name: /320 kbps/i })).not.toBeInTheDocument();
    expect(screen.queryByText(/^Bitrate$/)).not.toBeInTheDocument();
    // The format choice itself is still on screen — the panel didn't just
    // collapse out from under the assertion above.
    expect(screen.getByRole("radio", { name: /^FLAC$/ })).toBeChecked();
  });

  it("keeps the bitrate control for every lossy format (FR-203)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await openPanel(user);

    await user.click(screen.getByRole("radio", { name: /^Opus$/ }));
    expect(screen.getByRole("radio", { name: /192 kbps/i })).toBeInTheDocument();
  });

  it("carries a chosen bitrate across a change of lossy format", async () => {
    const changes: OutputOptions[] = [];
    const user = userEvent.setup();
    render(<Harness onChange={(next) => changes.push(next)} />);
    await openPanel(user);

    await user.click(screen.getByRole("radio", { name: /192 kbps/i }));
    await user.click(screen.getByRole("radio", { name: /M4A \/ AAC/ }));

    expect(changes[changes.length - 1].audio).toEqual({ format: "m4a", bitrate_kbps: 192 });
  });

  // FR-207. Both directions are asserted: a warning that is always on screen
  // tells the user nothing.
  it("warns that the file will be converted, and stops warning for keep-source (FR-207)", async () => {
    const user = userEvent.setup();
    render(<Harness />);

    // Collapsed on purpose — the warning has to reach the users who never
    // open the advanced section.
    expect(screen.getByText(/will be converted to MP3/i)).toBeInTheDocument();
    expect(screen.queryByText(/no conversion runs/i)).not.toBeInTheDocument();

    await openPanel(user);
    await user.click(screen.getByRole("radio", { name: /keep source/i }));

    expect(screen.queryByText(/will be converted/i)).not.toBeInTheDocument();
    expect(screen.getByText(/no conversion runs/i)).toBeInTheDocument();
  });

  it("warns that a video will be repackaged into the chosen container (FR-207)", async () => {
    const user = userEvent.setup();
    render(<Harness mediaType="video" />);

    expect(screen.getByText(/repackaged into a MP4 container/i)).toBeInTheDocument();

    await openPanel(user);
    await user.click(screen.getByRole("radio", { name: /keep source/i }));

    expect(screen.queryByText(/repackaged/i)).not.toBeInTheDocument();
    expect(screen.getByText(/no conversion runs/i)).toBeInTheDocument();
  });

  // FR-210/SC-209. The control stays on screen and explains itself; hiding it
  // is what makes a dropped option look like a bug.
  it("explains why cover art cannot be embedded into WAV instead of hiding it (FR-210)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await openPanel(user);

    // MP3 supports cover art: the reason must not be on screen yet.
    expect(screen.queryByText(/no tag area/i)).not.toBeInTheDocument();
    expect(screen.getByRole("switch", { name: /embed cover art/i })).toBeEnabled();

    await user.click(screen.getByRole("radio", { name: /^WAV$/ }));

    const coverArt = screen.getByRole("switch", { name: /embed cover art/i });
    expect(coverArt).toBeInTheDocument();
    expect(coverArt).toBeDisabled();
    expect(screen.getByText(/WAV has no tag area/i)).toBeInTheDocument();
    expect(screen.getByText(/download still succeeds/i)).toBeInTheDocument();
  });

  it("explains that keep-source leaves the cover-art container unknown (FR-210)", async () => {
    const user = userEvent.setup();
    render(<Harness mediaType="video" />);
    await openPanel(user);

    await user.click(screen.getByRole("radio", { name: /keep source/i }));

    expect(screen.getByRole("switch", { name: /embed cover art/i })).toBeDisabled();
    expect(screen.getByText(/final container unknown/i)).toBeInTheDocument();
  });

  // FR-234: gallery jobs run through gallery-dl, which reads none of these
  // fields.
  it("renders nothing at all for a gallery source (FR-234)", () => {
    const { container } = render(<Harness mediaType="gallery" />);

    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByRole("button", { name: /output options/i })).not.toBeInTheDocument();
  });

  it("shows audio choices only for audio jobs and container choices only for video jobs", async () => {
    const user = userEvent.setup();
    const { unmount } = render(<Harness mediaType="audio" />);
    await openPanel(user);

    expect(screen.getByText(/audio format/i)).toBeInTheDocument();
    expect(screen.queryByText(/codec priority/i)).not.toBeInTheDocument();
    unmount();

    render(<Harness mediaType="video" />);
    await openPanel(user);

    expect(screen.getByText(/video container/i)).toBeInTheDocument();
    expect(screen.getByText(/codec priority/i)).toBeInTheDocument();
    expect(screen.queryByText(/audio format/i)).not.toBeInTheDocument();
  });

  it("starts a new job with metadata and cover art embedded (FR-208/FR-209)", async () => {
    const user = userEvent.setup();
    render(<Harness />);
    await openPanel(user);

    expect(screen.getByRole("switch", { name: /embed title and artist/i })).toBeChecked();
    expect(screen.getByRole("switch", { name: /embed cover art/i })).toBeChecked();
  });

  it("emits the codec preference the user picked (FR-205)", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Harness mediaType="video" onChange={onChange} />);
    await openPanel(user);

    await user.click(screen.getByRole("radio", { name: /quality/i }));

    expect(onChange).toHaveBeenCalledWith(
      expect.objectContaining({ codec_preference: "quality" }),
    );
  });

  it("emits an embed flag the user turned off (FR-208)", async () => {
    const onChange = vi.fn();
    const user = userEvent.setup();
    render(<Harness onChange={onChange} />);
    await openPanel(user);

    await user.click(screen.getByRole("switch", { name: /embed title and artist/i }));

    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ embed_metadata: false }));
  });
});
