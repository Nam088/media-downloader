import { useState } from "react";
import { useTranslation } from "react-i18next";
import { CheckCheck, Pause, Play, Trash2, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useQueueStore } from "@/stores/queue-store";

interface QueueToolbarProps {
  /** Jobs that are running or waiting to run — what "Pause all" would act on. */
  activeCount: number;
  pausedCount: number;
  /** Completed, failed or canceled jobs still sitting in the queue view. */
  finishedCount: number;
}

/**
 * Whole-queue actions (FR-118). Every button is disabled when it would do
 * nothing, so a disabled "Resume all" is a readable statement that nothing is
 * paused rather than a dead control.
 *
 * "Cancel all" is the only destructive one and asks first — it is a single
 * click away from throwing away every partially downloaded file in the queue.
 */
export function QueueToolbar({ activeCount, pausedCount, finishedCount }: QueueToolbarProps) {
  const { t } = useTranslation();
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const pauseAll = useQueueStore((state) => state.pauseAll);
  const resumeAll = useQueueStore((state) => state.resumeAll);
  const cancelAll = useQueueStore((state) => state.cancelAll);
  const clearFinished = useQueueStore((state) => state.clearFinished);

  const hasStoppable = activeCount > 0 || pausedCount > 0;

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Button
        variant="outline"
        size="sm"
        disabled={activeCount === 0}
        onClick={() => void pauseAll()}
      >
        <Pause className="mr-1 size-4" />
        {t("queue.pause_all")}
      </Button>

      <Button
        variant="outline"
        size="sm"
        disabled={pausedCount === 0}
        onClick={() => void resumeAll()}
      >
        <Play className="mr-1 size-4" />
        {t("queue.resume_all")}
      </Button>

      {confirmingCancel ? (
        <>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => {
              void cancelAll();
              setConfirmingCancel(false);
            }}
          >
            <CheckCheck className="mr-1 size-4" />
            {t("queue.confirm_cancel_all")}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => setConfirmingCancel(false)}>
            {t("common.cancel")}
          </Button>
        </>
      ) : (
        <Button
          variant="outline"
          size="sm"
          disabled={!hasStoppable}
          onClick={() => setConfirmingCancel(true)}
        >
          <X className="mr-1 size-4" />
          {t("queue.cancel_all")}
        </Button>
      )}

      <Button
        variant="ghost"
        size="sm"
        disabled={finishedCount === 0}
        onClick={() => clearFinished()}
      >
        <Trash2 className="mr-1 size-4" />
        {t("queue.clear_finished")}
      </Button>
    </div>
  );
}
