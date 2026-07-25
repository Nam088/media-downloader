import { useTranslation } from "react-i18next";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import type { MediaSource } from "@/types/download";

interface PlaylistScopeDialogProps {
  preview: MediaSource | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onChoose: (scope: "single_item" | "entire_playlist") => void;
}

/** FR-013: a playlist link must never be bulk-downloaded without asking —
 * this is the confirmation gate between preview and `create_download_job`. */
export function PlaylistScopeDialog({
  preview,
  open,
  onOpenChange,
  onChoose,
}: PlaylistScopeDialogProps) {
  const { t } = useTranslation();
  if (!preview) return null;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{t("playlistScope.title")}</DialogTitle>
          <DialogDescription>
            {preview.playlist_item_count != null
              ? t("playlistScope.description_with_count", { count: preview.playlist_item_count })
              : t("playlistScope.description_unknown_count")}
          </DialogDescription>
        </DialogHeader>
        <DialogFooter className="gap-2 sm:gap-2">
          <Button variant="outline" onClick={() => onChoose("single_item")}>
            {t("playlistScope.single_item_button")}
          </Button>
          <Button onClick={() => onChoose("entire_playlist")}>
            {t("playlistScope.entire_playlist_button")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
