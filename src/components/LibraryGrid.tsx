import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";
import {
  FileAudio,
  FileVideo,
  FolderOpen,
  FolderInput,
  Images,
  Link2,
  Pencil,
  Play,
  RotateCcw,
  Trash2,
  TriangleAlert,
  X,
} from "lucide-react";

import { Button } from "@/components/ui/button";
import { formatDuration, formatFileSize } from "@/lib/format";
import { cn } from "@/lib/utils";
import { hasActiveFilters, useLibraryStore } from "@/stores/library-store";
import type { MediaType } from "@/types/download";
import type { LibraryItem } from "@/types/library";

/** Thao tác cần một hộp thoại xác nhận trước khi chạm vào đĩa (FR-322). Chúng
 * đi ngược lên trang Thư viện thay vì tự chạy ở đây, vì hộp thoại và trình phát
 * sống ở đó. */
export type LibraryActionKind = "rename" | "move" | "delete" | "remove" | "relink";

export interface LibraryActionRequest {
  kind: LibraryActionKind;
  items: LibraryItem[];
}

interface LibraryGridProps {
  onPreview: (item: LibraryItem) => void;
  onRequest: (request: LibraryActionRequest) => void;
}

/**
 * FR-306 + FR-310 + FR-311 — lưới ảnh, danh sách gọn, và hai trạng thái rỗng.
 *
 * Về quy mô: mỗi lần chỉ có `LIBRARY_PAGE_SIZE` mục đi qua IPC, và trang kế
 * tiếp chỉ được xin khi người dùng thật sự cuộn tới đáy. Ảnh đại diện mang
 * `loading="lazy"`, nên trình duyệt chỉ tải ảnh của những ô đang ở gần khung
 * nhìn — điều kiện "không nạp toàn bộ ảnh cùng lúc" của FR-310.
 *
 * Về dữ liệu thật: trong CSDL hiện tại KHÔNG mục nào có ảnh đại diện và KHÔNG
 * mục nào có thời lượng — cả hai chỉ được ghi lại từ Phase 3 trở đi. Nên hai
 * trường hợp ấy không phải ngoại lệ hiếm gặp mà là mặc định: thiếu ảnh thì vẽ
 * biểu tượng theo loại nội dung, thiếu thời lượng thì KHÔNG vẽ gì cả — `0:00`
 * là một lời nói dối về một file dài mười phút.
 */
export function LibraryGrid({ onPreview, onRequest }: LibraryGridProps) {
  const { t } = useTranslation();
  const items = useLibraryStore((state) => state.items);
  const viewMode = useLibraryStore((state) => state.viewMode);
  const loading = useLibraryStore((state) => state.loading);
  const loadingMore = useLibraryStore((state) => state.loadingMore);
  const hasMore = useLibraryStore((state) => state.hasMore);
  const loadMore = useLibraryStore((state) => state.loadMore);
  const filters = useLibraryStore((state) => state.filters);
  const clearFilters = useLibraryStore((state) => state.clearFilters);
  const selectedIds = useLibraryStore((state) => state.selectedIds);
  const toggleSelected = useLibraryStore((state) => state.toggleSelected);
  const redownloadItem = useLibraryStore((state) => state.redownloadItem);
  const revealItem = useLibraryStore((state) => state.revealItem);

  const sentinelRef = useRef<HTMLDivElement | null>(null);

  // Cuộn tới đáy thì xin trang tiếp theo. Nút bấm bên dưới vẫn ở đó và làm
  // đúng việc ấy — `IntersectionObserver` chỉ là đường tắt cho chuột, không
  // phải điều kiện để tính năng hoạt động.
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || !hasMore) return;
    if (typeof IntersectionObserver === "undefined") return;

    const observer = new IntersectionObserver((entries) => {
      if (entries.some((entry) => entry.isIntersecting)) void loadMore();
    });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [hasMore, loadMore, items.length]);

  if (loading && items.length === 0) {
    return (
      <p className="py-16 text-center text-sm text-muted-foreground" data-testid="library-loading">
        {t("common.loading")}
      </p>
    );
  }

  if (items.length === 0) {
    // FR-311: hai câu chuyện khác nhau. "Chưa tải gì bao giờ" cần chỉ đường tới
    // chỗ tải; "không khớp bộ lọc" cần một nút tháo bộ lọc ra. Đổi chỗ hai câu
    // này là gửi người dùng đi sai hướng.
    return hasActiveFilters(filters) ? (
      <div
        className="flex flex-col items-center gap-3 py-16 text-center"
        data-testid="library-empty-no-results"
      >
        <p className="text-sm font-semibold text-foreground">{t("library.no_results_title")}</p>
        <p className="max-w-md text-xs text-muted-foreground">{t("library.no_results_body")}</p>
        <Button
          variant="outline"
          size="sm"
          onClick={clearFilters}
          data-testid="library-clear-filters-empty"
        >
          {t("library.clear_filters")}
        </Button>
      </div>
    ) : (
      <div
        className="flex flex-col items-center gap-3 py-16 text-center"
        data-testid="library-empty-nothing-downloaded"
      >
        <p className="text-sm font-semibold text-foreground">{t("library.empty_title")}</p>
        <p className="max-w-md text-xs text-muted-foreground">{t("library.empty_body")}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <ul
        className={cn(
          viewMode === "grid"
            ? "grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4"
            : "flex flex-col gap-2",
        )}
        data-testid="library-items"
        data-view-mode={viewMode}
      >
        {items.map((item) =>
          viewMode === "grid" ? (
            <GridCard
              key={item.id}
              item={item}
              selected={selectedIds.includes(item.id)}
              onToggleSelected={() => toggleSelected(item.id)}
              onPreview={() => onPreview(item)}
              onRequest={onRequest}
              onReveal={() => void revealItem(item.id)}
              onRedownload={() => void redownloadItem(item.id)}
            />
          ) : (
            <ListRow
              key={item.id}
              item={item}
              selected={selectedIds.includes(item.id)}
              onToggleSelected={() => toggleSelected(item.id)}
              onPreview={() => onPreview(item)}
              onRequest={onRequest}
              onReveal={() => void revealItem(item.id)}
              onRedownload={() => void redownloadItem(item.id)}
            />
          ),
        )}
      </ul>

      <div ref={sentinelRef} />

      {hasMore && (
        <Button
          variant="outline"
          size="sm"
          className="mx-auto"
          disabled={loadingMore}
          onClick={() => void loadMore()}
          data-testid="library-load-more"
        >
          {loadingMore ? t("common.loading") : t("library.load_more")}
        </Button>
      )}
    </div>
  );
}

const MEDIA_TYPE_ICONS: Record<MediaType, typeof FileAudio> = {
  audio: FileAudio,
  video: FileVideo,
  gallery: Images,
};

/** FR-301 acceptance #3: không ô nào để trống. Mọi mục trong CSDL hiện tại rơi
 * vào nhánh này, nên nó là hình dạng bình thường của lưới, không phải nhánh dự
 * phòng. */
function Thumbnail({ item, className }: { item: LibraryItem; className?: string }) {
  const Icon = MEDIA_TYPE_ICONS[item.media_type];
  // The asset protocol can refuse a path outside its allowed scope, or the
  // file on disk can be gone — either way this falls back to the same
  // placeholder as "no thumbnail" rather than the browser's bare
  // broken-image glyph (FR-301 acceptance #3: no empty cell).
  const [failed, setFailed] = useState(false);

  if (item.thumbnail_path === null || failed) {
    return (
      <div
        className={cn(
          "flex items-center justify-center bg-muted/60 text-muted-foreground",
          className,
        )}
        data-testid="library-thumbnail-placeholder"
        data-media-type={item.media_type}
      >
        <Icon className="h-8 w-8" />
      </div>
    );
  }

  return (
    <img
      src={convertFileSrc(item.thumbnail_path)}
      alt={item.title}
      loading="lazy"
      decoding="async"
      className={cn("object-cover", className)}
      data-testid="library-thumbnail-image"
      onError={() => setFailed(true)}
    />
  );
}

function MissingBadge() {
  const { t } = useTranslation();
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full bg-amber-500/15 px-2 py-0.5 text-[10px] font-semibold text-amber-700 dark:text-amber-400"
      data-testid="library-missing-badge"
    >
      <TriangleAlert className="h-3 w-3" />
      {t("library.missing_badge")}
    </span>
  );
}

interface ItemProps {
  item: LibraryItem;
  selected: boolean;
  onToggleSelected: () => void;
  onPreview: () => void;
  onRequest: (request: LibraryActionRequest) => void;
  onReveal: () => void;
  onRedownload: () => void;
}

/** Các nút chung của cả hai kiểu hiển thị. Mục đang thiếu đổi bộ nút: phát và
 * đổi tên đều vô nghĩa khi file không còn ở đó, còn "tìm lại file" (FR-325) và
 * "tải lại" (FR-326) chỉ có nghĩa đúng ở trường hợp này. */
function ItemActions({
  item,
  onPreview,
  onRequest,
  onReveal,
  onRedownload,
}: Omit<ItemProps, "selected" | "onToggleSelected">) {
  const { t } = useTranslation();

  if (item.is_missing) {
    return (
      <>
        <IconAction
          label={t("library.action_relink")}
          testId="library-action-relink"
          onClick={() => onRequest({ kind: "relink", items: [item] })}
        >
          <Link2 className="h-3.5 w-3.5" />
        </IconAction>
        <IconAction
          label={t("library.action_redownload")}
          testId="library-action-redownload"
          onClick={onRedownload}
        >
          <RotateCcw className="h-3.5 w-3.5" />
        </IconAction>
        <IconAction
          label={t("library.action_remove")}
          testId="library-action-remove"
          onClick={() => onRequest({ kind: "remove", items: [item] })}
        >
          <X className="h-3.5 w-3.5" />
        </IconAction>
      </>
    );
  }

  return (
    <>
      <IconAction label={t("library.action_play")} testId="library-action-play" onClick={onPreview}>
        <Play className="h-3.5 w-3.5" />
      </IconAction>
      <IconAction
        label={t("library.action_reveal")}
        testId="library-action-reveal"
        onClick={onReveal}
      >
        <FolderOpen className="h-3.5 w-3.5" />
      </IconAction>
      <IconAction
        label={t("library.action_rename")}
        testId="library-action-rename"
        onClick={() => onRequest({ kind: "rename", items: [item] })}
      >
        <Pencil className="h-3.5 w-3.5" />
      </IconAction>
      <IconAction
        label={t("library.action_move")}
        testId="library-action-move"
        onClick={() => onRequest({ kind: "move", items: [item] })}
      >
        <FolderInput className="h-3.5 w-3.5" />
      </IconAction>
      <IconAction
        label={t("library.action_delete")}
        testId="library-action-delete"
        onClick={() => onRequest({ kind: "delete", items: [item] })}
      >
        <Trash2 className="h-3.5 w-3.5" />
      </IconAction>
    </>
  );
}

function IconAction({
  label,
  testId,
  onClick,
  children,
}: {
  label: string;
  testId: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="h-7 w-7"
      aria-label={label}
      data-testid={testId}
      onClick={onClick}
    >
      {children}
    </Button>
  );
}

function SelectBox({
  selected,
  onToggle,
  label,
}: {
  selected: boolean;
  onToggle: () => void;
  label: string;
}) {
  return (
    <input
      type="checkbox"
      checked={selected}
      onChange={onToggle}
      aria-label={label}
      data-testid="library-select"
      className="h-4 w-4 shrink-0 cursor-pointer accent-primary"
    />
  );
}

/** Dòng phụ dưới tiêu đề. Thời lượng chỉ xuất hiện khi thật sự biết. */
function ItemMeta({ item }: { item: LibraryItem }) {
  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-muted-foreground">
      <span>{item.platform}</span>
      <span>{item.file_format}</span>
      <span>{formatFileSize(item.file_size_bytes)}</span>
      {item.duration_seconds !== null && (
        <span data-testid="library-item-duration">{formatDuration(item.duration_seconds)}</span>
      )}
      <span>{new Date(item.downloaded_at).toLocaleDateString()}</span>
    </div>
  );
}

function GridCard(props: ItemProps) {
  const { t } = useTranslation();
  const { item, selected, onToggleSelected } = props;

  return (
    <li
      className={cn(
        "flex flex-col overflow-hidden rounded-xl border bg-card shadow-2xs transition-colors",
        selected ? "border-primary" : "border-border/70",
      )}
      data-testid="library-item"
      data-item-id={item.id}
      data-missing={item.is_missing ? "true" : "false"}
    >
      <div className="relative">
        <button
          type="button"
          onClick={props.onPreview}
          aria-label={t("library.action_play")}
          className="block w-full"
        >
          <Thumbnail item={item} className="h-32 w-full" />
        </button>
        <div className="absolute left-2 top-2 rounded bg-background/80 p-1">
          <SelectBox
            selected={selected}
            onToggle={onToggleSelected}
            label={t("library.select_item", { title: item.title })}
          />
        </div>
        {item.is_missing && (
          <div className="absolute right-2 top-2">
            <MissingBadge />
          </div>
        )}
      </div>

      <div className="flex flex-1 flex-col gap-1 p-3">
        <p className="line-clamp-2 text-xs font-semibold text-foreground" title={item.title}>
          {item.title}
        </p>
        <ItemMeta item={item} />
        <div className="mt-auto flex flex-wrap items-center pt-2">
          <ItemActions {...props} />
        </div>
      </div>
    </li>
  );
}

function ListRow(props: ItemProps) {
  const { t } = useTranslation();
  const { item, selected, onToggleSelected } = props;

  return (
    <li
      className={cn(
        "flex items-center gap-3 rounded-lg border bg-card px-3 py-2 shadow-2xs transition-colors",
        selected ? "border-primary" : "border-border/70",
      )}
      data-testid="library-item"
      data-item-id={item.id}
      data-missing={item.is_missing ? "true" : "false"}
    >
      <SelectBox
        selected={selected}
        onToggle={onToggleSelected}
        label={t("library.select_item", { title: item.title })}
      />
      <Thumbnail item={item} className="h-10 w-10 shrink-0 rounded" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <p className="truncate text-xs font-semibold text-foreground" title={item.title}>
            {item.title}
          </p>
          {item.is_missing && <MissingBadge />}
        </div>
        <ItemMeta item={item} />
      </div>
      <div className="flex shrink-0 items-center">
        <ItemActions {...props} />
      </div>
    </li>
  );
}
