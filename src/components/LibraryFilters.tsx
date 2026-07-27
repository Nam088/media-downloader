import { useTranslation } from "react-i18next";
import { ArrowDownWideNarrow, ArrowUpNarrowWide, LayoutGrid, List, Search, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { formatPlatformLabel } from "@/lib/format";
import { cn } from "@/lib/utils";
import { hasActiveFilters, useLibraryStore } from "@/stores/library-store";
import type { MediaType } from "@/types/download";
import type { LibrarySort } from "@/types/library";

const MEDIA_TYPES: MediaType[] = ["audio", "video", "gallery"];
const SORTS: LibrarySort[] = ["downloaded_at", "title", "file_size", "duration"];

/**
 * FR-306 → FR-309 — ô tìm kiếm, bộ lọc, thứ tự sắp xếp và kiểu hiển thị.
 *
 * Ô tìm kiếm được điều khiển bởi `searchInput` (đổi ngay mỗi phím) chứ không
 * phải `filters.search` (đổi sau một nhịp debounce). Nếu buộc vào cái sau, con
 * trỏ sẽ giật lùi mỗi lần gõ nhanh hơn 300ms.
 *
 * Danh sách nền tảng và định dạng lấy từ `facets` — ảnh chụp thống kê không áp
 * bộ lọc — chứ không phải từ `stats`, để chọn một nền tảng không làm biến mất
 * các nền tảng còn lại khỏi chính bộ lọc đó.
 */
export function LibraryFilters() {
  const { t } = useTranslation();
  const searchInput = useLibraryStore((state) => state.searchInput);
  const filters = useLibraryStore((state) => state.filters);
  const facets = useLibraryStore((state) => state.facets);
  const sort = useLibraryStore((state) => state.sort);
  const direction = useLibraryStore((state) => state.direction);
  const viewMode = useLibraryStore((state) => state.viewMode);
  const setSearch = useLibraryStore((state) => state.setSearch);
  const setFilters = useLibraryStore((state) => state.setFilters);
  const toggleMediaType = useLibraryStore((state) => state.toggleMediaType);
  const togglePlatform = useLibraryStore((state) => state.togglePlatform);
  const toggleFormat = useLibraryStore((state) => state.toggleFormat);
  const clearFilters = useLibraryStore((state) => state.clearFilters);
  const setSort = useLibraryStore((state) => state.setSort);
  const setViewMode = useLibraryStore((state) => state.setViewMode);

  const filtering = hasActiveFilters(filters);
  const platforms = facets?.by_platform.map((entry) => entry.key) ?? [];
  const formats = facets?.formats ?? [];

  return (
    <section className="flex flex-col gap-4" data-testid="library-filters">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
        <div className="relative w-full sm:max-w-sm">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={searchInput}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t("library.search_placeholder")}
            aria-label={t("library.search_label")}
            data-testid="library-search"
            className="h-9 rounded-lg border-border/80 bg-card pl-9 text-xs shadow-2xs focus-visible:ring-1 focus-visible:ring-primary/40"
          />
        </div>

        <div className="flex items-center gap-2 sm:ml-auto">
          <label className="sr-only" htmlFor="library-sort">
            {t("library.sort_label")}
          </label>
          <select
            id="library-sort"
            value={sort}
            onChange={(event) => setSort(event.target.value as LibrarySort, direction)}
            data-testid="library-sort"
            className="h-9 cursor-pointer rounded-lg border border-border/80 bg-card px-3 py-1 text-xs font-medium text-foreground shadow-2xs outline-none transition-colors hover:bg-accent/40 focus-visible:ring-1 focus-visible:ring-primary/40 dark:bg-card"
          >
            {SORTS.map((option) => (
              <option key={option} value={option} className="bg-popover text-popover-foreground">
                {t(`library.sort_${option}`)}
              </option>
            ))}
          </select>

          <Button
            variant="outline"
            size="icon"
            className="h-9 w-9"
            data-testid="library-sort-direction"
            aria-label={
              direction === "asc" ? t("library.sort_ascending") : t("library.sort_descending")
            }
            onClick={() => setSort(sort, direction === "asc" ? "desc" : "asc")}
          >
            {direction === "asc" ? (
              <ArrowUpNarrowWide className="h-4 w-4" />
            ) : (
              <ArrowDownWideNarrow className="h-4 w-4" />
            )}
          </Button>

          <div className="flex items-center rounded-lg border border-border/80 bg-card p-0.5 shadow-2xs">
            <Button
              variant={viewMode === "grid" ? "secondary" : "ghost"}
              size="icon"
              className="h-8 w-8"
              aria-pressed={viewMode === "grid"}
              aria-label={t("library.view_grid")}
              data-testid="library-view-grid"
              onClick={() => setViewMode("grid")}
            >
              <LayoutGrid className="h-4 w-4" />
            </Button>
            <Button
              variant={viewMode === "list" ? "secondary" : "ghost"}
              size="icon"
              className="h-8 w-8"
              aria-pressed={viewMode === "list"}
              aria-label={t("library.view_list")}
              data-testid="library-view-list"
              onClick={() => setViewMode("list")}
            >
              <List className="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 rounded-xl border border-border/70 bg-card/40 p-3">
        <FilterRow label={t("library.filter_media_type")}>
          {MEDIA_TYPES.map((mediaType) => (
            <Chip
              key={mediaType}
              active={filters.media_types.includes(mediaType)}
              onClick={() => toggleMediaType(mediaType)}
              testId={`library-filter-media-type-${mediaType}`}
            >
              {t(`library.media_type_${mediaType}`)}
            </Chip>
          ))}
        </FilterRow>

        {platforms.length > 0 && (
          <FilterRow label={t("library.filter_platform")}>
            {platforms.map((platform) => (
              <Chip
                key={platform}
                active={filters.platforms.includes(platform)}
                onClick={() => togglePlatform(platform)}
                testId={`library-filter-platform-${platform}`}
              >
                {formatPlatformLabel(platform)}
              </Chip>
            ))}
          </FilterRow>
        )}

        {formats.length > 0 && (
          <FilterRow label={t("library.filter_format")}>
            {formats.map((format) => (
              <Chip
                key={format}
                active={filters.formats.includes(format)}
                onClick={() => toggleFormat(format)}
                testId={`library-filter-format-${format}`}
              >
                {format}
              </Chip>
            ))}
          </FilterRow>
        )}

        <FilterRow label={t("library.filter_date")}>
          <input
            type="date"
            value={filters.downloaded_from ?? ""}
            onChange={(event) => setFilters({ downloaded_from: event.target.value || null })}
            aria-label={t("library.filter_date_from")}
            data-testid="library-filter-date-from"
            className="h-8 rounded-md border border-border/80 bg-card px-2 text-xs"
          />
          <span className="text-xs text-muted-foreground">
            {t("library.filter_date_to_joiner")}
          </span>
          <input
            type="date"
            value={filters.downloaded_to ?? ""}
            onChange={(event) => setFilters({ downloaded_to: event.target.value || null })}
            aria-label={t("library.filter_date_to")}
            data-testid="library-filter-date-to"
            className="h-8 rounded-md border border-border/80 bg-card px-2 text-xs"
          />
          <Chip
            active={filters.missing_only}
            onClick={() => setFilters({ missing_only: !filters.missing_only })}
            testId="library-filter-missing-only"
          >
            {t("library.filter_missing_only")}
          </Chip>
        </FilterRow>

        {filtering && (
          <div
            className="flex flex-wrap items-center gap-2 border-t border-border/60 pt-3"
            data-testid="library-active-filters"
          >
            <span className="text-xs font-semibold text-muted-foreground">
              {t("library.active_filters_label")}
            </span>
            {filters.search.trim() !== "" && (
              <ActiveChip>{t("library.active_search", { term: filters.search.trim() })}</ActiveChip>
            )}
            {filters.media_types.map((mediaType) => (
              <ActiveChip key={`media-${mediaType}`}>
                {t(`library.media_type_${mediaType}`)}
              </ActiveChip>
            ))}
            {filters.platforms.map((platform) => (
              <ActiveChip key={`platform-${platform}`}>{formatPlatformLabel(platform)}</ActiveChip>
            ))}
            {filters.formats.map((format) => (
              <ActiveChip key={`format-${format}`}>{format}</ActiveChip>
            ))}
            {filters.downloaded_from !== null && (
              <ActiveChip>
                {t("library.active_date_from", { date: filters.downloaded_from })}
              </ActiveChip>
            )}
            {filters.downloaded_to !== null && (
              <ActiveChip>
                {t("library.active_date_to", { date: filters.downloaded_to })}
              </ActiveChip>
            )}
            {filters.missing_only && <ActiveChip>{t("library.filter_missing_only")}</ActiveChip>}
            <Button
              variant="ghost"
              size="sm"
              className="ml-auto h-7 gap-1 text-xs"
              onClick={clearFilters}
              data-testid="library-clear-filters"
            >
              <X className="h-3.5 w-3.5" />
              {t("library.clear_filters")}
            </Button>
          </div>
        )}
      </div>
    </section>
  );
}

function FilterRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <span className="w-24 shrink-0 text-xs font-semibold text-muted-foreground">{label}</span>
      {children}
    </div>
  );
}

function Chip({
  active,
  onClick,
  testId,
  children,
}: {
  active: boolean;
  onClick: () => void;
  testId: string;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      data-testid={testId}
      className={cn(
        "rounded-full border px-2.5 py-1 text-xs font-medium transition-colors",
        active
          ? "border-primary bg-primary/10 text-primary"
          : "border-border/80 bg-card text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function ActiveChip({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-full bg-primary/10 px-2.5 py-1 text-xs font-medium text-primary">
      {children}
    </span>
  );
}
