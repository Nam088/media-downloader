import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { CloudflareGrantDialog } from "@/components/CloudflareGrantDialog";
import { useQueueStore } from "@/stores/queue-store";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

// Sonner needs a mounted <Toaster /> to put anything on screen, and what
// matters here is *what* was reported, not where it landed.
vi.mock("sonner", () => ({
  toast: { error: vi.fn(), success: vi.fn(), info: vi.fn() },
}));

const JOB_ID = "job-1";
const CHALLENGE_URL = "https://challenges.cloudflare.com/turnstile/abc123";

/** Puts a job's challenge in the store the way the event listener would. */
function seedChallenge(attempts = 0) {
  useQueueStore.setState({
    challenges: { [JOB_ID]: { challengeUrl: CHALLENGE_URL, attempts, dismissed: false } },
  });
}

function renderDialog(overrides: Partial<Parameters<typeof CloudflareGrantDialog>[0]> = {}) {
  const onOpenChange = vi.fn();
  render(
    <CloudflareGrantDialog
      jobId={JOB_ID}
      challengeUrl={CHALLENGE_URL}
      open
      onOpenChange={onOpenChange}
      {...overrides}
    />,
  );
  return { onOpenChange };
}

function grantField() {
  return screen.getByLabelText(/verification code/i);
}

function submitButton() {
  return screen.getByRole("button", { name: /continue the download/i });
}

describe("CloudflareGrantDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
    useQueueStore.setState({ challenges: {} });
  });

  it("shows the challenge URL and hands it to the system browser", async () => {
    const user = userEvent.setup();
    seedChallenge();
    renderDialog();

    expect(screen.getByText(/needs a quick verification/i)).toBeInTheDocument();
    expect(screen.getByText(CHALLENGE_URL)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /open the verification page/i }));
    expect(openUrl).toHaveBeenCalledWith(CHALLENGE_URL);
  });

  it("submits the pasted grant with the camelCase job id and closes", async () => {
    const user = userEvent.setup();
    seedChallenge();
    const { onOpenChange } = renderDialog();

    expect(submitButton()).toBeDisabled();

    await user.type(grantField(), "GRANT-XYZ");
    await user.click(submitButton());

    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith("submit_cloudflare_grant", {
        jobId: JOB_ID,
        grant: "GRANT-XYZ",
      });
    });
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });

  it("reports a grant the backend would not take, and clears the field", async () => {
    const user = userEvent.setup();
    vi.mocked(invoke).mockRejectedValue({ code: "SPOTIFLAC_CHALLENGE_TIMEOUT" });
    seedChallenge();
    const { onOpenChange } = renderDialog();

    await user.type(grantField(), "WRONG");
    await user.click(submitButton());

    await waitFor(() => expect(toast.error).toHaveBeenCalled());
    expect(toast.error).toHaveBeenCalledWith(expect.stringMatching(/code was not accepted/i));
    expect(grantField()).toHaveValue("");
    expect(onOpenChange).not.toHaveBeenCalled();
  });

  it("counts down as the worker re-asks after a rejected code", async () => {
    seedChallenge();
    renderDialog();

    expect(screen.getByText(/3 attempts left/i)).toBeInTheDocument();

    // A re-emitted challenge is the worker saying the last code was no good.
    act(() => {
      useQueueStore
        .getState()
        .applyChallenge({ job_id: JOB_ID, challenge_url: CHALLENGE_URL });
    });

    expect(screen.getByText(/2 attempts left/i)).toBeInTheDocument();
  });

  it("locks the form once the third code has been rejected", async () => {
    const user = userEvent.setup();
    seedChallenge(3);
    renderDialog();

    expect(screen.getByText(/0 attempts left/i)).toBeInTheDocument();
    expect(grantField()).toBeDisabled();
    expect(submitButton()).toBeDisabled();

    await user.click(submitButton());
    expect(invoke).not.toHaveBeenCalled();
  });

  describe("after a frontend reload", () => {
    it("recovers the URL the challenge event carried", async () => {
      vi.mocked(invoke).mockResolvedValue({ challenge_url: CHALLENGE_URL });
      renderDialog({ challengeUrl: null });

      await waitFor(() => {
        expect(invoke).toHaveBeenCalledWith("get_pending_challenge", { jobId: JOB_ID });
      });
      expect(await screen.findByText(CHALLENGE_URL)).toBeInTheDocument();
    });

    it("closes itself when the job is no longer waiting", async () => {
      vi.mocked(invoke).mockResolvedValue(null);
      const { onOpenChange } = renderDialog({ challengeUrl: null });

      await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
    });
  });
});
