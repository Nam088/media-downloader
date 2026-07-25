import { useState } from "react";
import { useTranslation } from "react-i18next";
import { HistoryList } from "@/components/HistoryList";
import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";

export function History() {
  const { t } = useTranslation();
  const [searchTerm, setSearchTerm] = useState("");
  const [filterStatus, setFilterStatus] = useState<string>("all");

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      {/* Top Header & Search Controls */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div className="flex flex-col gap-1">
          <h2 className="text-xl font-bold tracking-tight text-foreground">{t("nav.history")}</h2>
          <p className="text-xs text-muted-foreground">Manage and access all your downloaded media files</p>
        </div>

        {/* Search Input Box */}
        <div className="relative w-full sm:w-72">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={searchTerm}
            onChange={(e) => setSearchTerm(e.target.value)}
            placeholder="Search downloads..."
            className="pl-9 h-9 text-xs rounded-lg border-border/80 bg-card shadow-2xs focus-visible:ring-1 focus-visible:ring-primary/40"
          />
        </div>
      </div>

      {/* Filter Tabs */}
      <div className="flex items-center gap-2 border-b border-border/60 pb-3">
        {[
          { id: "all", label: "All Downloads" },
          { id: "completed", label: "Completed" },
          { id: "failed", label: "Failed" },
          { id: "canceled", label: "Canceled" },
        ].map((tab) => (
          <button
            key={tab.id}
            onClick={() => setFilterStatus(tab.id)}
            className={`rounded-md px-3 py-1.5 text-xs font-semibold transition-all ${
              filterStatus === tab.id
                ? "bg-primary text-primary-foreground shadow-2xs"
                : "bg-muted/50 text-muted-foreground hover:bg-muted hover:text-foreground"
            }`}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* History List Component */}
      <HistoryList searchTerm={searchTerm} filterStatus={filterStatus} />
    </div>
  );
}
