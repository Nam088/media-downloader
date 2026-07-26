import { useTranslation } from "react-i18next";
import { FileWarning, HardDrive, Layers } from "lucide-react";

import { formatFileSize } from "@/lib/format";
import { useLibraryStore } from "@/stores/library-store";
import type { LibraryBreakdownEntry } from "@/types/library";
import type { MediaType } from "@/types/download";
import { cn } from "@/lib/utils";

/**
 * FR-328 + FR-329 — tổng quan mức sử dụng, và mỗi dòng phân bố là một bộ lọc.
 *
 * Số liệu đến từ `library_stats` với ĐÚNG bộ lọc mà `list_library` vừa nhận,
 * nên chúng luôn mô tả tập đang hiển thị chứ không phải toàn thư viện (SC-307).
 * Hệ quả nhìn thấy được: bấm "youtube" xong thì tổng số mục tụt xuống còn số
 * của riêng YouTube — đó là hành vi đúng, không phải số liệu bị hỏng.
 */
export function LibraryStats() {
  const { t } = useTranslation();
  const stats = useLibraryStore((state) => state.stats);
  const filters = useLibraryStore((state) => state.filters);
  const togglePlatform = useLibraryStore((state) => state.togglePlatform);
  const toggleMediaType = useLibraryStore((state) => state.toggleMediaType);

  if (!stats) return null;

  return (
    <section
      className="grid gap-4 rounded-xl border border-border/70 bg-card/60 p-4 md:grid-cols-3"
      aria-label={t("library.stats_heading")}
      data-testid="library-stats"
    >
      <div className="flex flex-col gap-3">
        <Metric
          icon={<Layers className="h-4 w-4" />}
          label={t("library.stats_total_items")}
          value={String(stats.total_items)}
          testId="library-stats-total-items"
        />
        <Metric
          icon={<HardDrive className="h-4 w-4" />}
          label={t("library.stats_total_size")}
          value={formatFileSize(stats.total_size_bytes)}
          testId="library-stats-total-size"
        />
        {stats.missing_items > 0 && (
          <Metric
            icon={<FileWarning className="h-4 w-4 text-amber-600 dark:text-amber-400" />}
            label={t("library.stats_missing")}
            value={String(stats.missing_items)}
            testId="library-stats-missing"
          />
        )}
      </div>

      <Breakdown
        heading={t("library.stats_by_platform")}
        entries={stats.by_platform}
        labelOf={(entry) => entry.key}
        isActive={(entry) => filters.platforms.includes(entry.key)}
        onSelect={(entry) => togglePlatform(entry.key)}
        testId="library-breakdown-platform"
      />

      <Breakdown
        heading={t("library.stats_by_media_type")}
        entries={stats.by_media_type}
        labelOf={(entry) => t(`library.media_type_${entry.key}`)}
        isActive={(entry) => filters.media_types.includes(entry.key as MediaType)}
        onSelect={(entry) => toggleMediaType(entry.key as MediaType)}
        testId="library-breakdown-media-type"
      />
    </section>
  );
}

function Metric({
  icon,
  label,
  value,
  testId,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  testId: string;
}) {
  return (
    <div className="flex items-center gap-2.5" data-testid={testId}>
      <span className="text-muted-foreground">{icon}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
      <span className="ml-auto font-mono text-sm font-semibold tabular-nums">{value}</span>
    </div>
  );
}

function Breakdown({
  heading,
  entries,
  labelOf,
  isActive,
  onSelect,
  testId,
}: {
  heading: string;
  entries: LibraryBreakdownEntry[];
  labelOf: (entry: LibraryBreakdownEntry) => string;
  isActive: (entry: LibraryBreakdownEntry) => boolean;
  onSelect: (entry: LibraryBreakdownEntry) => void;
  testId: string;
}) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col gap-2" data-testid={testId}>
      <p className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
        {heading}
      </p>
      {entries.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("library.stats_breakdown_empty")}</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {entries.map((entry) => (
            <li key={entry.key}>
              <button
                type="button"
                onClick={() => onSelect(entry)}
                aria-pressed={isActive(entry)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1 text-xs transition-colors",
                  isActive(entry)
                    ? "bg-primary/10 font-semibold text-primary"
                    : "text-foreground/80 hover:bg-muted",
                )}
              >
                <span className="truncate">{labelOf(entry)}</span>
                <span className="ml-auto shrink-0 font-mono tabular-nums text-muted-foreground">
                  {entry.item_count}
                </span>
                <span className="shrink-0 font-mono tabular-nums text-muted-foreground">
                  {formatFileSize(entry.total_size_bytes)}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
