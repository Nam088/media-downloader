import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { CheckSquare, FolderInput, ListMusic, RefreshCw, Square, Trash2 } from "lucide-react";

import { ErrorBanner } from "@/components/ErrorBanner";
import { LibraryFilters } from "@/components/LibraryFilters";
import { LibraryGrid, type LibraryActionRequest } from "@/components/LibraryGrid";
import { LibraryStats } from "@/components/LibraryStats";
import { MediaPlayer, stopActiveMedia } from "@/components/MediaPlayer";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { useLibraryStore } from "@/stores/library-store";
import type { LibraryItem, LibraryReconciledEvent } from "@/types/library";

/** Tên file (không kèm thư mục) — thứ `rename_library_item` nhận. Truyền cả
 * đường dẫn xuống đó thì backend làm sạch nó thành một cái tên vô nghĩa. */
function fileNameOf(filePath: string): string {
  return filePath.split(/[\\/]/).pop() ?? filePath;
}

/**
 * Trang Thư viện (`specs/004-library`, FR-305 → FR-330 phía giao diện).
 *
 * `active` không phải một chi tiết trang trí: `App.tsx` dựng mọi trang cùng lúc
 * rồi ẩn/hiện bằng class, nên trang này tồn tại từ giây đầu tiên ứng dụng chạy.
 * Một `useEffect(..., [])` nạp dữ liệu ở đây sẽ bắt MỌI lần khởi động trả tiền
 * cho một truy vấn thư viện cộng một vòng đối soát đĩa mà chưa ai nhìn tới.
 */
export function Library({ active }: { active: boolean }) {
  const { t } = useTranslation();

  const items = useLibraryStore((state) => state.items);
  const selectedIds = useLibraryStore((state) => state.selectedIds);
  const error = useLibraryStore((state) => state.error);
  const reconciling = useLibraryStore((state) => state.reconciling);
  const ensureLoaded = useLibraryStore((state) => state.ensureLoaded);
  const reconcile = useLibraryStore((state) => state.reconcile);
  const applyReconciled = useLibraryStore((state) => state.applyReconciled);
  const setError = useLibraryStore((state) => state.setError);
  const selectAllVisible = useLibraryStore((state) => state.selectAllVisible);
  const clearSelection = useLibraryStore((state) => state.clearSelection);
  const selectionInDisplayOrder = useLibraryStore((state) => state.selectionInDisplayOrder);
  const renameItem = useLibraryStore((state) => state.renameItem);
  const moveItems = useLibraryStore((state) => state.moveItems);
  const deleteItems = useLibraryStore((state) => state.deleteItems);
  const removeItems = useLibraryStore((state) => state.removeItems);
  const relinkItem = useLibraryStore((state) => state.relinkItem);
  const exportPlaylist = useLibraryStore((state) => state.exportPlaylist);

  const [preview, setPreview] = useState<LibraryItem | null>(null);
  const [request, setRequest] = useState<LibraryActionRequest | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [moveTarget, setMoveTarget] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const openedOnce = useRef(false);

  // Đối soát chạy MỘT lần khi trang được mở lần đầu, và ngoài ra chỉ chạy khi
  // người dùng bấm nút. Gắn nó vào mỗi lần render — hay mỗi lần chuyển tab —
  // là biến một vòng `stat` trên toàn thư viện thành phí thường trực.
  useEffect(() => {
    if (!active || openedOnce.current) return;
    openedOnce.current = true;
    void (async () => {
      await ensureLoaded();
      await reconcile();
    })();
  }, [active, ensureLoaded, reconcile]);

  // Mỗi lô đối soát phát một sự kiện mang đúng những id vừa đổi trạng thái, nên
  // lưới sáng dần lên thay vì đứng im tới khi quét xong (FR-327).
  useEffect(() => {
    const unlisten = listen<LibraryReconciledEvent>("library:reconciled", (event) => {
      applyReconciled(event.payload.changed_item_ids);
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [applyReconciled]);

  /** FR-316 + edge case "xoá mục đang phát": dừng trước khi file biến mất dưới
   * chân trình phát, chứ không để nó phát vào một đường dẫn không còn tồn tại. */
  const stopIfPlaying = useCallback(
    (affected: LibraryItem[]) => {
      if (preview === null) return;
      if (!affected.some((item) => item.id === preview.id)) return;
      stopActiveMedia();
      setPreview(null);
    },
    [preview],
  );

  const closeRequest = useCallback(() => {
    setRequest(null);
    setMoveTarget(null);
    setRenameValue("");
  }, []);

  function handleRequest(next: LibraryActionRequest) {
    setError(null);
    setMoveTarget(null);
    setRenameValue(next.kind === "rename" ? fileNameOf(next.items[0].file_path) : "");
    if (next.kind === "relink") {
      void handleRelink(next.items[0]);
      return;
    }
    setRequest(next);
  }

  async function handleRelink(item: LibraryItem) {
    const chosen = await openDialog({ directory: false, multiple: false });
    if (typeof chosen !== "string") return;
    await relinkItem(item.id, chosen);
  }

  async function handleChooseMoveTarget() {
    const chosen = await openDialog({ directory: true, multiple: false });
    if (typeof chosen === "string") setMoveTarget(chosen);
  }

  async function handleConfirm() {
    if (request === null) return;
    setBusy(true);
    let done = false;
    const ids = request.items.map((item) => item.id);

    if (request.kind === "rename") {
      done = await renameItem(request.items[0].id, renameValue);
    } else if (request.kind === "move" && moveTarget !== null) {
      stopIfPlaying(request.items);
      done = await moveItems(ids, moveTarget);
    } else if (request.kind === "delete") {
      stopIfPlaying(request.items);
      done = await deleteItems(ids);
    } else if (request.kind === "remove") {
      done = await removeItems(ids);
    }

    setBusy(false);
    // Thất bại giữ hộp thoại mở: `FILE_EXISTS` nghĩa là thao tác BỊ TỪ CHỐI và
    // người dùng phải chọn một cái tên khác — đóng hộp thoại ở đây sẽ trông
    // hệt như vừa thành công.
    if (done) closeRequest();
  }

  async function handleExportPlaylist() {
    const chosen = selectionInDisplayOrder();
    if (chosen.length === 0) return;
    const destination = await saveDialog({
      defaultPath: "playlist.m3u",
      filters: [{ name: t("library.export_filter_name"), extensions: ["m3u"] }],
    });
    if (typeof destination !== "string") return;
    await exportPlaylist(
      chosen.map((item) => item.id),
      destination,
    );
  }

  const allSelected = items.length > 0 && selectedIds.length === items.length;

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex flex-col gap-1">
          <h2 className="text-xl font-bold tracking-tight text-foreground">{t("nav.library")}</h2>
          <p className="text-xs text-muted-foreground">{t("library.subtitle")}</p>
        </div>
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          disabled={reconciling}
          onClick={() => void reconcile()}
          data-testid="library-reconcile"
        >
          <RefreshCw className={reconciling ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />
          {reconciling ? t("library.reconcile_running") : t("library.reconcile")}
        </Button>
      </div>

      <ErrorBanner error={error} onDismiss={() => setError(null)} />

      <LibraryStats />
      <LibraryFilters />

      <div className="flex flex-wrap items-center gap-2" data-testid="library-selection-bar">
        <Button
          variant="ghost"
          size="sm"
          className="h-8 gap-1.5 text-xs"
          onClick={() => (allSelected ? clearSelection() : selectAllVisible())}
          data-testid="library-select-all"
          disabled={items.length === 0}
        >
          {allSelected ? (
            <CheckSquare className="h-3.5 w-3.5" />
          ) : (
            <Square className="h-3.5 w-3.5" />
          )}
          {allSelected ? t("library.select_none") : t("library.select_all")}
        </Button>

        <span className="text-xs text-muted-foreground" data-testid="library-selected-count">
          {t("library.selected_count", { count: selectedIds.length })}
        </span>

        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            className="h-8 gap-1.5 text-xs"
            disabled={selectedIds.length === 0}
            onClick={() => handleRequest({ kind: "move", items: selectionInDisplayOrder() })}
            data-testid="library-bulk-move"
          >
            <FolderInput className="h-3.5 w-3.5" />
            {t("library.action_move")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-8 gap-1.5 text-xs"
            disabled={selectedIds.length === 0}
            onClick={() => handleRequest({ kind: "delete", items: selectionInDisplayOrder() })}
            data-testid="library-bulk-delete"
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t("library.action_delete")}
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-8 gap-1.5 text-xs"
            disabled={items.length === 0}
            onClick={() => void handleExportPlaylist()}
            data-testid="library-export-playlist"
          >
            <ListMusic className="h-3.5 w-3.5" />
            {t("library.export_playlist")}
          </Button>
        </div>
      </div>

      {preview !== null && (
        <div className="flex flex-col gap-2" data-testid="library-preview">
          <MediaPlayer filePath={preview.file_path} title={preview.title} autoPlay />
          <Button
            variant="ghost"
            size="sm"
            className="self-end text-xs"
            onClick={() => {
              stopActiveMedia();
              setPreview(null);
            }}
            data-testid="library-preview-close"
          >
            {t("common.close")}
          </Button>
        </div>
      )}

      <LibraryGrid onPreview={setPreview} onRequest={handleRequest} />

      <Dialog open={request !== null} onOpenChange={(open) => !open && closeRequest()}>
        <DialogContent data-testid="library-confirm-dialog">
          {request !== null && (
            <>
              <DialogHeader>
                <DialogTitle>{t(`library.${request.kind}_title`)}</DialogTitle>
                <DialogDescription data-testid="library-confirm-description">
                  {request.kind === "rename"
                    ? t("library.rename_description", {
                        name: fileNameOf(request.items[0].file_path),
                      })
                    : t(`library.${request.kind}_description`, { count: request.items.length })}
                </DialogDescription>
              </DialogHeader>

              {request.kind === "rename" && (
                <Input
                  value={renameValue}
                  onChange={(event) => setRenameValue(event.target.value)}
                  aria-label={t("library.rename_label")}
                  data-testid="library-rename-input"
                />
              )}

              {request.kind === "move" && (
                <div className="flex flex-col gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleChooseMoveTarget()}
                    data-testid="library-move-choose"
                  >
                    {t("library.move_choose_folder")}
                  </Button>
                  <p
                    className="truncate text-xs text-muted-foreground"
                    data-testid="library-move-target"
                  >
                    {moveTarget ?? t("library.move_no_folder")}
                  </p>
                </div>
              )}

              <DialogFooter>
                <Button variant="ghost" onClick={closeRequest} data-testid="library-confirm-cancel">
                  {t("common.cancel")}
                </Button>
                <Button
                  variant={
                    request.kind === "rename" || request.kind === "move" ? "default" : "destructive"
                  }
                  onClick={() => void handleConfirm()}
                  disabled={
                    busy ||
                    (request.kind === "rename" && renameValue.trim() === "") ||
                    (request.kind === "move" && moveTarget === null)
                  }
                  data-testid="library-confirm-submit"
                >
                  {t(`library.${request.kind}_confirm`)}
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
