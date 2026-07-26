import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import { ExternalLink } from "lucide-react";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { MAX_GRANT_ATTEMPTS, useQueueStore } from "@/stores/queue-store";

/** Ties the label to the field. A constant rather than a `useId()` value so
 * tests can talk about the input by its accessible name alone. */
const GRANT_INPUT_ID = "cloudflare-grant";

export interface CloudflareGrantDialogProps {
  /** The job waiting on a grant. `null` when no challenge is open. */
  jobId: string | null;
  /** URL carried by the `job:cloudflare_challenge` event, when the caller
   * already has it. `null` falls back to the store's copy, then to the
   * backend's — see the recovery effect below. */
  challengeUrl: string | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * The Cloudflare grant prompt (US3).
 *
 * A music download can stall on a Cloudflare check that only a real browser
 * can clear. The worker parks the job in `waiting_input`, this dialog sends the
 * user out to the page and takes the code back — `submitGrant` writes it to the
 * worker's stdin. Nothing here decides whether the code was any good: the
 * worker does, and its verdict arrives as queue events. So the dialog owns only
 * the field and the round-trip; the attempt count it displays comes from the
 * store, which counts the worker's re-emitted challenges (data-model.md §6).
 */
export function CloudflareGrantDialog({
  jobId,
  challengeUrl,
  open,
  onOpenChange,
}: CloudflareGrantDialogProps) {
  const { t } = useTranslation();
  const [grant, setGrant] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const challenge = useQueueStore((state) => (jobId ? state.challenges[jobId] : undefined));
  const submitGrant = useQueueStore((state) => state.submitGrant);
  const restorePendingChallenge = useQueueStore((state) => state.restorePendingChallenge);

  // Kept in a ref so the recovery effect can close the dialog without listing
  // the callback as a dependency — callers pass an inline arrow, and that would
  // re-run the recovery lookup on every render.
  const onOpenChangeRef = useRef(onOpenChange);
  useEffect(() => {
    onOpenChangeRef.current = onOpenChange;
  }, [onOpenChange]);

  const url = challengeUrl ?? challenge?.challengeUrl ?? null;
  const attemptsLeft = Math.max(0, MAX_GRANT_ATTEMPTS - (challenge?.attempts ?? 0));
  const exhausted = attemptsLeft <= 0;

  // A fresh challenge — a new job, or the same one re-challenged after being
  // dismissed — starts from an empty field. Adjusted during render rather
  // than in an effect: an effect would paint the previous job's half-typed
  // code for one frame first, and React re-runs this pass before committing
  // anything to the DOM.
  const [openedFor, setOpenedFor] = useState<string | null>(null);
  const currentlyOpenFor = open ? jobId : null;
  if (currentlyOpenFor !== openedFor) {
    setOpenedFor(currentlyOpenFor);
    setGrant("");
    setSubmitting(false);
  }

  // Reload case: the challenge event fired before this window existed, so the
  // URL survives only in the Rust side's in-memory map. Coming back empty means
  // the job moved on and the dialog has nothing left to ask for.
  useEffect(() => {
    if (!open || !jobId || challengeUrl || challenge) return;
    let cancelled = false;
    void (async () => {
      await restorePendingChallenge(jobId);
      if (cancelled) return;
      if (!useQueueStore.getState().challenges[jobId]) onOpenChangeRef.current(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [open, jobId, challengeUrl, challenge, restorePendingChallenge]);

  const handleOpenBrowser = useCallback(() => {
    if (!url) return;
    void openUrl(url);
  }, [url]);

  const trimmedGrant = grant.trim();
  const canSubmit = Boolean(jobId) && trimmedGrant.length > 0 && !submitting && !exhausted;

  const handleSubmit = useCallback(
    async (event: FormEvent) => {
      event.preventDefault();
      if (!canSubmit || !jobId) return;
      setSubmitting(true);
      try {
        await submitGrant(jobId, trimmedGrant);
        onOpenChange(false);
      } catch {
        // Only the command itself failing lands here — a code the worker
        // rejects comes back as a re-emitted challenge, not as a throw. Either
        // way the code on screen is spent, so the field is cleared.
        setGrant("");
        toast.error(t("music.challenge.submit_failed"));
      } finally {
        setSubmitting(false);
      }
    },
    [canSubmit, jobId, trimmedGrant, submitGrant, onOpenChange, t],
  );

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("music.challenge.title")}</DialogTitle>
          <DialogDescription>{t("music.challenge.instructions")}</DialogDescription>
        </DialogHeader>

        <form className="flex flex-col gap-4" onSubmit={handleSubmit}>
          {url && (
            <div className="flex flex-col gap-2">
              <p className="rounded-lg bg-muted px-2.5 py-1.5 text-xs break-all text-muted-foreground">
                {url}
              </p>
              <Button type="button" variant="outline" onClick={handleOpenBrowser}>
                <ExternalLink />
                {t("music.challenge.openBrowser")}
              </Button>
            </div>
          )}

          <div className="flex flex-col gap-2">
            <Label htmlFor={GRANT_INPUT_ID}>{t("music.challenge.grantLabel")}</Label>
            <Input
              id={GRANT_INPUT_ID}
              value={grant}
              autoComplete="off"
              spellCheck={false}
              placeholder={t("music.challenge.grantPlaceholder")}
              disabled={exhausted || submitting}
              onChange={(event) => setGrant(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              {t("music.challenge.attemptsLeft", { count: attemptsLeft })}
            </p>
          </div>

          <DialogFooter>
            <Button type="submit" disabled={!canSubmit}>
              {t("music.challenge.submit")}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
