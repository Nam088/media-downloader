import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Sparkles, RefreshCw, X, RotateCcw, HelpCircle, MessageCircle } from "lucide-react";
import { Button } from "@/components/ui/button";

// ─── Types ────────────────────────────────────────────────────────────────────

interface QuestionNode {
  type: "question";
  text: string;
  choices: { label: string; next: string }[];
}

interface MessageNode {
  type: "message" | "end";
  text: string;
  author?: string;
  next?: string;
}

type ScenarioNode = QuestionNode | MessageNode;

interface Scenario {
  format: "scenario";
  title?: string;
  start: string;
  nodes: Record<string, ScenarioNode>;
}

interface StepMessage {
  id?: number | string;
  title?: string;
  message: string;
  author?: string;
}

// ─── Component ────────────────────────────────────────────────────────────────

interface SecretMessageModalProps {
  open: boolean;
  onClose: () => void;
}

export function SecretMessageModal({ open, onClose }: SecretMessageModalProps) {
  const { t } = useTranslation();

  // Default geography scenario — built with t() so no hardcoded strings
  function buildDefaultScenario(): Scenario {
    const bot = t("secret.geo_bot");
    return {
      format: "scenario",
      title: t("secret.geo_title"),
      start: "q1",
      nodes: {
        q1: {
          type: "question",
          text: t("secret.geo_q1"),
          choices: [
            { label: t("secret.geo_q1_paris"), next: "q1_correct" },
            { label: t("secret.geo_q1_berlin"), next: "q1_wrong" },
            { label: t("secret.geo_q1_rome"), next: "q1_wrong" },
            { label: t("secret.geo_q1_amsterdam"), next: "q1_wrong" },
          ],
        },
        q1_correct: {
          type: "message",
          text: t("secret.geo_q1_correct"),
          author: bot,
          next: "q2",
        },
        q1_wrong: {
          type: "message",
          text: t("secret.geo_q1_wrong"),
          author: bot,
          next: "q2",
        },
        q2: {
          type: "question",
          text: t("secret.geo_q2"),
          choices: [
            { label: t("secret.geo_q2_amazon"), next: "q2_amazon" },
            { label: t("secret.geo_q2_nile"), next: "q2_correct" },
            { label: t("secret.geo_q2_yangtze"), next: "q2_wrong" },
            { label: t("secret.geo_q2_mississippi"), next: "q2_wrong" },
          ],
        },
        q2_correct: {
          type: "message",
          text: t("secret.geo_q2_correct"),
          author: bot,
          next: "q3",
        },
        q2_amazon: {
          type: "message",
          text: t("secret.geo_q2_amazon_ans"),
          author: bot,
          next: "q3",
        },
        q2_wrong: {
          type: "message",
          text: t("secret.geo_q2_wrong"),
          author: bot,
          next: "q3",
        },
        q3: {
          type: "question",
          text: t("secret.geo_q3"),
          choices: [
            { label: t("secret.geo_q3_china"), next: "q3_wrong" },
            { label: t("secret.geo_q3_usa"), next: "q3_wrong" },
            { label: t("secret.geo_q3_russia"), next: "q3_correct" },
            { label: t("secret.geo_q3_canada"), next: "q3_wrong" },
          ],
        },
        q3_correct: {
          type: "end",
          text: t("secret.geo_q3_correct"),
          author: bot,
        },
        q3_wrong: {
          type: "end",
          text: t("secret.geo_q3_wrong"),
          author: bot,
        },
      },
    };
  }

  // ── Flat-step mode (legacy / remote JSON array) ──────────────────────────
  const [customSteps, setCustomSteps] = useState<StepMessage[] | null>(null);
  const [currentIndex, setCurrentIndex] = useState(0);

  // ── Scenario mode (branching Q&A) ────────────────────────────────────────
  const [remoteScenario, setRemoteScenario] = useState<Scenario | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<string>("q1");

  const DEFAULT_GIST_URL =
    "https://gist.githubusercontent.com/Nam088/52e7bf02b289b95af2e3132a7da2c0a0/raw/scenario.json";

  // ── Shared UI state ──────────────────────────────────────────────────────
  const [loading, setLoading] = useState(false);
  const [animKey, setAnimKey] = useState(0);

  // Active scenario (remote overrides default)
  const activeScenario: Scenario = remoteScenario ?? buildDefaultScenario();
  const isScenarioMode = customSteps === null;

  // Reset + initial load on open
  useEffect(() => {
    if (open) {
      setCurrentIndex(0);
      setCurrentNodeId("q1");
      setAnimKey((k) => k + 1);
      void loadRemote(DEFAULT_GIST_URL);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);


  // ── Remote JSON loader ───────────────────────────────────────────────────
  async function loadRemote(url: string) {
    setLoading(true);
    const target = url;
    if (!target.trim()) { setLoading(false); return; }

    try {
      const res = await fetch(target, { cache: "no-store" });
      if (res.ok) {
        const data = (await res.json()) as unknown;

        // Scenario format
        if (
          data &&
          typeof data === "object" &&
          !Array.isArray(data) &&
          (data as Record<string, unknown>).format === "scenario"
        ) {
          const sc = data as Scenario;
          setRemoteScenario(sc);
          setCustomSteps(null);
          setCurrentNodeId(sc.start);
          setAnimKey((k) => k + 1);
          setLoading(false);
          return;
        }

        // Flat step array
        if (Array.isArray(data) && data.length > 0) {
          const parsedSteps: StepMessage[] = (data as Record<string, unknown>[])
            .map((item, idx) => ({
              id: (item.id as string | number) || idx + 1,
              title: (item.title as string) || t("secret.step_title", { number: idx + 1 }),
              message: (item.message as string) || (item.text as string) || (item.advice as string) || "",
              author: (item.author as string) || "Nam088",
            }))
            .filter((s) => s.message.length > 0);

          if (parsedSteps.length > 0) {
            setCustomSteps(parsedSteps);
            setRemoteScenario(null);
            setCurrentIndex(0);
            setAnimKey((k) => k + 1);
            setLoading(false);
            return;
          }
        }
      }
    } catch { /* fall through */ }

    setCustomSteps(null);
    setRemoteScenario(null);
    setCurrentNodeId("q1");
    setLoading(false);
  }

  function resetToDefault() {
    setCustomSteps(null);
    setRemoteScenario(null);
    setCurrentNodeId("q1");
    setAnimKey((k) => k + 1);
  }

  function goTo(nodeId: string) {
    setCurrentNodeId(nodeId);
    setAnimKey((k) => k + 1);
  }

  function replayScenario() {
    setCurrentNodeId(activeScenario.start);
    setAnimKey((k) => k + 1);
  }

  // ── Render: flat steps ───────────────────────────────────────────────────
  function renderFlatStep() {
    const steps = customSteps!;
    const step = steps[currentIndex] ?? steps[0];
    const total = steps.length;
    return (
      <>
        <div
          key={animKey}
          className="my-6 flex flex-col items-center text-center px-2 min-h-[130px] justify-center animate-in fade-in-50 duration-150"
        >
          <MessageCircle className="h-7 w-7 text-primary/30 mb-2" />
          {step.title && (
            <p className="text-xs font-semibold text-primary/70 mb-1">{step.title}</p>
          )}
          <p className="text-sm font-medium text-foreground leading-relaxed">{step.message}</p>
          {step.author && (
            <span className="mt-3 rounded-full bg-primary/10 px-3 py-0.5 font-mono text-[11px] font-semibold text-primary border border-primary/20">
              {step.author}
            </span>
          )}
        </div>
        <div className="flex items-center justify-between border-t border-border/40 pt-3">
          <div className="flex items-center gap-1.5">
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => { setCurrentIndex((p) => (p > 0 ? p - 1 : total - 1)); setAnimKey((k) => k + 1); }}
              disabled={total <= 1}
              className="h-8 w-8 p-0 cursor-pointer"
            >
              ‹
            </Button>
            <span className="font-mono text-xs text-muted-foreground px-2">
              {t("secret.step", { current: currentIndex + 1, total })}
            </span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => { setCurrentIndex((p) => (p < total - 1 ? p + 1 : 0)); setAnimKey((k) => k + 1); }}
              disabled={total <= 1}
              className="h-8 w-8 p-0 cursor-pointer"
            >
              ›
            </Button>
          </div>
          <Button type="button" size="sm" onClick={onClose} className="text-xs font-semibold cursor-pointer">
            {t("secret.close")}
          </Button>
        </div>
      </>
    );
  }

  // ── Render: scenario node ────────────────────────────────────────────────
  function renderScenarioNode() {
    const node = activeScenario.nodes[currentNodeId];
    if (!node) return null;

    if (node.type === "question") {
      return (
        <>
          <div
            key={animKey}
            className="my-5 flex flex-col items-center text-center px-2 min-h-[130px] justify-center animate-in fade-in-50 duration-150 gap-3"
          >
            <HelpCircle className="h-8 w-8 text-primary/40" />
            <p className="text-sm font-semibold text-foreground leading-relaxed">{node.text}</p>
          </div>
          <div className="flex flex-col gap-2 border-t border-border/40 pt-3">
            {node.choices.map((choice) => (
              <button
                key={choice.label}
                type="button"
                onClick={() => goTo(choice.next)}
                className="w-full rounded-xl border border-border/60 bg-muted/40 px-4 py-2.5 text-sm font-medium text-foreground text-left hover:bg-primary/10 hover:border-primary/40 transition-all duration-150 cursor-pointer"
              >
                {choice.label}
              </button>
            ))}
            <div className="flex justify-end mt-1">
              <Button type="button" variant="ghost" size="sm" onClick={onClose} className="text-xs text-muted-foreground cursor-pointer">
                {t("secret.close")}
              </Button>
            </div>
          </div>
        </>
      );
    }

    const isEnd = node.type === "end";
    return (
      <>
        <div
          key={animKey}
          className="my-6 flex flex-col items-center text-center px-2 min-h-[130px] justify-center animate-in fade-in-50 duration-150 gap-3"
        >
          <MessageCircle className={`h-7 w-7 ${isEnd ? "text-green-500/50" : "text-primary/30"}`} />
          <p className="text-sm font-medium text-foreground leading-relaxed">{node.text}</p>
          {node.author && (
            <span className="rounded-full bg-primary/10 px-3 py-0.5 font-mono text-[11px] font-semibold text-primary border border-primary/20">
              {node.author}
            </span>
          )}
        </div>
        <div className="flex items-center justify-between border-t border-border/40 pt-3">
          {isEnd ? (
            <Button type="button" variant="outline" size="sm" onClick={replayScenario} className="text-xs cursor-pointer gap-1.5">
              <RotateCcw className="h-3.5 w-3.5" />
              {t("secret.replay")}
            </Button>
          ) : (
            <div />
          )}
          <div className="flex items-center gap-2">
            {!isEnd && node.next && (
              <Button type="button" size="sm" onClick={() => goTo(node.next!)} className="text-xs font-semibold cursor-pointer">
                {t("secret.continue")} ›
              </Button>
            )}
            <Button type="button" variant={isEnd ? "default" : "ghost"} size="sm" onClick={onClose} className="text-xs cursor-pointer">
              {t("secret.close")}
            </Button>
          </div>
        </div>
      </>
    );
  }

  if (!open) return null;

  const scenarioTitle = isScenarioMode ? (activeScenario.title ?? t("secret.title")) : t("secret.title");

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-20 px-4 bg-background/60 backdrop-blur-xs animate-in fade-in-50 duration-200"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg overflow-hidden rounded-2xl border border-primary/40 bg-card p-6 shadow-2xl animate-in slide-in-from-top-6 duration-300 transition-all"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border/40 pb-3">
          <div className="flex items-center gap-2">
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <Sparkles className="h-4 w-4" />
            </div>
            <div>
              <h3 className="text-sm font-bold text-foreground">{scenarioTitle}</h3>
              <p className="text-xs text-muted-foreground">{t("secret.subtitle")}</p>
            </div>
          </div>
          <div className="flex items-center gap-1">
            {(remoteScenario !== null || customSteps !== null) && (
              <button
                type="button"
                onClick={resetToDefault}
                className="rounded-lg p-1 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors cursor-pointer"
                title={t("secret.reset_tooltip")}
              >
                <RotateCcw className="h-4 w-4" />
              </button>
            )}
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg p-1 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors cursor-pointer"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        </div>

        {/* Loading */}
        {loading ? (
          <div className="my-10 flex items-center justify-center gap-2 text-xs text-muted-foreground animate-pulse">
            <RefreshCw className="h-4 w-4 animate-spin text-primary" />
            <span>{t("secret.loading")}</span>
          </div>
        ) : isScenarioMode ? (
          renderScenarioNode()
        ) : (
          renderFlatStep()
        )}
      </div>
    </div>
  );
}
