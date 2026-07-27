import { useState } from "react";
import { useTranslation } from "react-i18next";
import { convertFileSrc } from "@tauri-apps/api/core";
import { openExternalUrl } from "@/lib/open-url";
import {
  Check,
  ChevronLeft,
  ChevronRight,
  ExternalLink,
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
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/tooltip";
import { formatDuration, formatFileSize, formatPlatformLabel } from "@/lib/format";
import { pageNumbers, totalPagesOf } from "@/lib/pagination";
import { cn } from "@/lib/utils";
import { LIBRARY_PAGE_SIZES, hasActiveFilters, useLibraryStore } from "@/stores/library-store";
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
 * Về quy mô: mỗi lần chỉ có một trang mục đi qua IPC, và trang kế tiếp chỉ được
 * xin khi người dùng bấm sang nó. Ảnh đại diện mang `loading="lazy"`, nên trình
 * duyệt chỉ tải ảnh của những ô đang ở gần khung nhìn — điều kiện "không nạp
 * toàn bộ ảnh cùng lúc" của FR-310.
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
  const page = useLibraryStore((state) => state.page);
  const viewMode = useLibraryStore((state) => state.viewMode);
  const loading = useLibraryStore((state) => state.loading);
  const filters = useLibraryStore((state) => state.filters);
  const clearFilters = useLibraryStore((state) => state.clearFilters);
  const selectedIds = useLibraryStore((state) => state.selectedIds);
  const toggleSelected = useLibraryStore((state) => state.toggleSelected);
  const redownloadItem = useLibraryStore((state) => state.redownloadItem);
  const revealItem = useLibraryStore((state) => state.revealItem);

  if (loading && items.length === 0) {
    return (
      <div
        className={cn(
          "animate-in fade-in-50 duration-200",
          viewMode === "grid"
            ? "grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4"
            : "flex flex-col gap-2.5",
        )}
        data-testid="library-loading"
      >
        {Array.from({ length: 8 }).map((_, i) => (
          <div
            key={i}
            className="flex flex-col gap-3 rounded-xl border border-border/70 bg-card p-3 shadow-2xs"
          >
            <Skeleton className="h-32 w-full rounded-lg" />
            <Skeleton className="h-4 w-3/4" />
            <div className="flex items-center justify-between pt-1">
              <Skeleton className="h-3 w-1/3" />
              <Skeleton className="h-3 w-1/4" />
            </div>
          </div>
        ))}
      </div>
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
        key={page}
        className={cn(
          "animate-in fade-in-50 duration-150 ease-out",
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

      <LibraryPagination />
    </div>
  );
}

/**
 * Thanh phân trang — cùng ngôn ngữ hình ảnh với trang Lịch sử: chọn cỡ trang ở
 * trái, `‹ 1 … 4 5 6 … 20 ›` ở phải, trang đang xem là nút đặc.
 *
 * Phần thuật toán (`pageNumbers`, `totalPagesOf`) nằm ở `@/lib/pagination` để
 * hai danh sách không thể gấp khác nhau; phần đánh dấu ở lại đây vì hai trang
 * có `data-testid` riêng và nhịp bố cục riêng.
 */
function LibraryPagination() {
  const { t } = useTranslation();
  const page = useLibraryStore((state) => state.page);
  const pageSize = useLibraryStore((state) => state.pageSize);
  const totalItems = useLibraryStore((state) => state.totalItems);
  const setPage = useLibraryStore((state) => state.setPage);
  const setPageSize = useLibraryStore((state) => state.setPageSize);

  const totalPages = totalPagesOf(totalItems, pageSize);

  return (
    <div
      className="flex flex-wrap items-center justify-between gap-3 pt-1"
      data-testid="library-pagination"
    >
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <label className="sr-only" htmlFor="library-page-size">
          {t("library.page_size_label")}
        </label>
        <select
          id="library-page-size"
          value={pageSize}
          onChange={(event) => void setPageSize(Number(event.target.value))}
          data-testid="library-page-size"
          className="h-8 rounded-lg border border-border/80 bg-card px-2 text-xs shadow-2xs"
        >
          {LIBRARY_PAGE_SIZES.map((size) => (
            <option key={size} value={size}>
              {size}
            </option>
          ))}
        </select>
        <span data-testid="library-page-label">
          {t("library.page_label", { current: page, total: totalPages })}
        </span>
      </div>

      <div className="flex items-center gap-1">
        <Button
          variant="outline"
          size="icon"
          className="h-8 w-8"
          disabled={page <= 1}
          onClick={() => void setPage(page - 1)}
          aria-label={t("library.prev_page")}
          data-testid="library-prev-page"
        >
          <ChevronLeft className="h-4 w-4" />
        </Button>
        {pageNumbers(page, totalPages).map((token, index) =>
          token === "ellipsis" ? (
            <span
              key={`ellipsis-${index}`}
              aria-hidden
              className="px-1 text-xs text-muted-foreground"
            >
              …
            </span>
          ) : (
            <Button
              key={token}
              variant={token === page ? "default" : "outline"}
              size="icon"
              className="h-8 w-8 text-xs"
              aria-current={token === page ? "page" : undefined}
              onClick={() => void setPage(token)}
              data-testid={`library-page-${token}`}
            >
              {token}
            </Button>
          ),
        )}
        <Button
          variant="outline"
          size="icon"
          className="h-8 w-8"
          disabled={page >= totalPages}
          onClick={() => void setPage(page + 1)}
          aria-label={t("library.next_page")}
          data-testid="library-next-page"
        >
          <ChevronRight className="h-4 w-4" />
        </Button>
      </div>
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
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 rounded-md text-muted-foreground hover:bg-primary/10 hover:text-primary transition-all duration-150"
          aria-label={label}
          data-testid={testId}
          onClick={onClick}
        >
          {children}
        </Button>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
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
    <label className="relative inline-flex cursor-pointer items-center justify-center">
      <input
        type="checkbox"
        checked={selected}
        onChange={onToggle}
        aria-label={label}
        data-testid="library-select"
        className="peer sr-only"
      />
      <div className="flex h-5.5 w-5.5 items-center justify-center rounded-md border border-border/80 bg-background/85 backdrop-blur-md shadow-xs transition-all duration-200 peer-checked:border-primary peer-checked:bg-primary peer-checked:text-primary-foreground peer-focus-visible:ring-2 peer-focus-visible:ring-primary/40 hover:border-primary/60 hover:scale-105">
        <Check className={cn("h-3.5 w-3.5 stroke-[3] transition-transform duration-150", selected ? "scale-100 opacity-100" : "scale-0 opacity-0")} />
      </div>
    </label>
  );
}

/** Dòng phụ dưới tiêu đề. Thời lượng chỉ xuất hiện khi thật sự biết; badge
 * nguồn phát chỉ xuất hiện trên mục nhạc có `source_provider` (T043). */
function ItemMeta({ item }: { item: LibraryItem }) {
  return (
    <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
      <span className="rounded bg-muted/60 px-1.5 py-0.5 font-medium text-foreground/80">{formatPlatformLabel(item.platform)}</span>
      <span className="rounded bg-muted/60 px-1.5 py-0.5 font-mono text-foreground/80">{item.file_format}</span>
      <span className="font-mono text-xs text-muted-foreground">{formatFileSize(item.file_size_bytes)}</span>
      {item.duration_seconds !== null && (
        <span className="font-mono text-xs text-muted-foreground" data-testid="library-item-duration">{formatDuration(item.duration_seconds)}</span>
      )}
      <span className="ml-auto text-[10px] text-muted-foreground/70">{new Date(item.downloaded_at).toLocaleDateString()}</span>
    </div>
  );
}

function GridCard(props: ItemProps) {
  const { t } = useTranslation();
  const { item, selected, onToggleSelected } = props;

  return (
    <li
      className={cn(
        "group flex flex-col overflow-hidden rounded-xl border bg-card shadow-2xs transition-all duration-200 hover:shadow-md hover:border-primary/40",
        selected ? "border-primary ring-1 ring-primary/30 bg-primary/[0.02]" : "border-border/70",
      )}
      data-testid="library-item"
      data-item-id={item.id}
      data-missing={item.is_missing ? "true" : "false"}
    >
      <div className="relative overflow-hidden">
        <button
          type="button"
          onClick={props.onPreview}
          aria-label={t("library.action_play")}
          className="group/thumb block w-full relative overflow-hidden cursor-pointer"
        >
          <Thumbnail item={item} className="h-36 w-full transition-transform duration-300 group-hover/thumb:scale-105" />
          <div className="absolute inset-0 bg-black/30 opacity-0 group-hover/thumb:opacity-100 transition-opacity duration-200 flex items-center justify-center">
            <div className="flex h-10 w-10 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-lg transition-transform duration-200 group-hover/thumb:scale-110">
              <Play className="h-5 w-5 fill-current ml-0.5" />
            </div>
          </div>
        </button>
        <div className="absolute left-2.5 top-2.5 z-10">
          <SelectBox
            selected={selected}
            onToggle={onToggleSelected}
            label={t("library.select_item", { title: item.title })}
          />
        </div>
        {item.is_missing && (
          <div className="absolute right-2.5 top-2.5 z-10">
            <MissingBadge />
          </div>
        )}
      </div>

      <div className="flex flex-1 flex-col gap-2 p-3.5">
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              onClick={() => void openExternalUrl(item.source_url)}
              className="line-clamp-2 text-xs font-semibold leading-snug text-foreground transition-colors hover:text-primary text-left hover:underline cursor-pointer flex items-start gap-1"
            >
              <span className="flex-1">{item.title}</span>
              <ExternalLink className="h-3 w-3 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity text-primary mt-0.5" />
            </button>
          </TooltipTrigger>
          <TooltipContent>{t("common.open_in_browser")}</TooltipContent>
        </Tooltip>
        <ItemMeta item={item} />
        <div className="mt-auto flex items-center justify-between pt-2.5 border-t border-border/40">
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
        "group flex items-center gap-3 rounded-xl border bg-card px-3.5 py-2.5 shadow-2xs transition-all duration-200 hover:border-primary/40 hover:shadow-xs",
        selected ? "border-primary ring-1 ring-primary/30 bg-primary/[0.02]" : "border-border/70",
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
      <Thumbnail item={item} className="h-10 w-10 shrink-0 rounded-lg" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                onClick={() => void openExternalUrl(item.source_url)}
                className="truncate text-xs font-semibold text-foreground transition-colors hover:text-primary hover:underline cursor-pointer inline-flex items-center gap-1 text-left"
              >
                <span className="truncate">{item.title}</span>
                <ExternalLink className="h-3 w-3 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity text-primary" />
              </button>
            </TooltipTrigger>
            <TooltipContent>{t("common.open_in_browser")}</TooltipContent>
          </Tooltip>
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
