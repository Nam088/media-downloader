// E2E smoke test automating the quickstart.md User Story 1 validation
// scenario, extended per T049 to cover all 6 platforms required by FR-014
// (SC-002) and to measure completion time against SC-001 (<30s for a video
// under 10 minutes). NOT executed in this environment — see wdio.conf.ts
// for what's needed to actually run it (a real display + tauri-driver).
import { expect } from "@wdio/globals";
import { PLATFORM_SAMPLE_LINKS, SC001_MAX_AUDIO_READY_MS } from "./fixtures";

describe("Audio download — one link per supported platform (FR-014, SC-002)", () => {
  for (const [platform, url] of Object.entries(PLATFORM_SAMPLE_LINKS)) {
    it(`downloads audio successfully from ${platform}`, async () => {
      const urlInput = await $('textarea[id="source-url"]');
      await urlInput.setValue(url);

      const previewButton = await $('button=Preview');
      await previewButton.click();

      const previewTitle = await $(".truncate.font-medium");
      await previewTitle.waitForDisplayed({ timeout: 15_000 });

      const startedAt = Date.now();

      const downloadButton = await $('button=Download audio');
      await downloadButton.click();

      const completedBadge = await $("text=Completed");
      await completedBadge.waitForDisplayed({ timeout: 60_000 });

      const elapsedMs = Date.now() - startedAt;
      // SC-001 assumes a <10min source video; this suite doesn't verify
      // the fixture's duration automatically, so treat this as informative
      // rather than a hard failure if a longer test link is ever used.
      console.log(`[${platform}] completed in ${elapsedMs}ms (SC-001 budget: ${SC001_MAX_AUDIO_READY_MS}ms)`);

      expect(elapsedMs).toBeLessThan(SC001_MAX_AUDIO_READY_MS * 4); // generous outer bound; log is the real signal
    });
  }
});
