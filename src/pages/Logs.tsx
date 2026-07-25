import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Trash2, Copy } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import type { LogEntry } from "@/types/logs";

/** Debug log level to Tailwind color — matches `logging::log_event`'s
 * "ERROR"/"WARN"/"INFO" strings on the backend. Anything else (there
 * shouldn't be any) falls back to the neutral/default color. */
function levelClassName(level: string): string {
  switch (level) {
    case "ERROR":
      return "text-destructive";
    case "WARN":
      return "text-amber-600 dark:text-amber-400";
    default:
      return "text-muted-foreground";
  }
}

function formatTimestamp(iso: string): string {
  try {
    return new Date(iso).toLocaleTimeString();
  } catch {
    return iso;
  }
}

/** Surfaces `logging::log_event` entries (job failures, retry attempts,
 * gallery-dl fallback issues, ...) inside the app itself — most of this
 * app's real failure modes only ever printed to a dev terminal
 * (`pnpm tauri dev`'s stdout), which doesn't exist at all in a packaged
 * production build, making them undiagnosable without this page. */
export function Logs() {
  const { t } = useTranslation();
  const [entries, setEntries] = useState<LogEntry[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<LogEntry[]>("get_logs").then(setEntries);

    const unlisten = listen<LogEntry>("log:new_entry", (event) => {
      setEntries((prev) => [...prev, event.payload]);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ block: "end" });
  }, [entries.length]);

  async function handleClear() {
    await invoke("clear_logs");
    setEntries([]);
  }

  async function handleCopyAll() {
    const text = entries.map((e) => `[${e.timestamp}] [${e.level}] ${e.message}`).join("\n");
    await navigator.clipboard.writeText(text);
    toast.success(t("logs.copied"));
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-4 px-6 py-6">
      <div className="flex items-center justify-between">
        <div className="flex flex-col gap-0.5">
          <h2 className="text-lg font-bold tracking-tight">{t("nav.logs")}</h2>
          <p className="text-xs text-muted-foreground">{t("logs.description")}</p>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={handleCopyAll} disabled={entries.length === 0} className="gap-1.5">
            <Copy className="h-3.5 w-3.5" />
            {t("logs.copy_all")}
          </Button>
          <Button variant="outline" size="sm" onClick={handleClear} disabled={entries.length === 0} className="gap-1.5">
            <Trash2 className="h-3.5 w-3.5" />
            {t("logs.clear")}
          </Button>
        </div>
      </div>

      <div className="h-[60vh] overflow-y-auto rounded-lg border border-border/80 bg-muted/20 p-3 font-mono text-xs">
        {entries.length === 0 ? (
          <p className="p-4 text-center text-muted-foreground">{t("logs.empty")}</p>
        ) : (
          <div className="flex flex-col gap-1">
            {entries.map((entry, i) => (
              <div key={i} className="flex gap-2 break-all">
                <span className="shrink-0 text-muted-foreground/70">{formatTimestamp(entry.timestamp)}</span>
                <span className={`shrink-0 font-semibold ${levelClassName(entry.level)}`}>[{entry.level}]</span>
                <span className="text-foreground/90">{entry.message}</span>
              </div>
            ))}
            <div ref={bottomRef} />
          </div>
        )}
      </div>
    </div>
  );
}
