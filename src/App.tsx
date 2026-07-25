import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "next-themes";
import { Download, Sparkles, Home as HomeIcon, History as HistoryIcon, Settings as SettingsIcon, ScrollText } from "lucide-react";
import { ThemeProvider } from "@/components/theme-provider";
import { ComplianceDisclaimer } from "@/components/ComplianceDisclaimer";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { Toaster } from "@/components/ui/sonner";
import { useAppSettings } from "@/hooks/use-app-settings";
import { Home } from "@/pages/Home";
import { History } from "@/pages/History";
import { Settings } from "@/pages/Settings";
import { Logs } from "@/pages/Logs";

type Route = "home" | "history" | "logs" | "settings";

function useSyncBackendSettings() {
  const { settings } = useAppSettings();
  const { setTheme } = useTheme();
  const { i18n } = useTranslation();

  useEffect(() => {
    if (!settings) return;
    if (settings.theme !== "system") setTheme(settings.theme);
    if (settings.language !== "system") i18n.changeLanguage(settings.language);
  }, [settings, setTheme, i18n]);
}

function AppShell() {
  const { t } = useTranslation();
  const [route, setRoute] = useState<Route>("home");
  const { settings } = useAppSettings();
  useSyncBackendSettings();

  // Derived at render time rather than corrected via a setState-in-effect
  // redirect: if the Logs tab gets hidden (Settings toggle) while it's the
  // active route, this just falls back to "home" for that render — no
  // effect, no extra render pass.
  const effectiveRoute: Route = route === "logs" && settings && !settings.show_logs_tab ? "home" : route;

  // Hidden by default (models::AppSettings.show_logs_tab) — the Logs page is
  // a debugging aid (job failures/retries/fallback decisions), not something
  // most users need in the main nav; toggled on from Settings.
  const navItems: { id: Route; label: string; icon: typeof HomeIcon }[] = [
    { id: "home", label: t("nav.home", "Home"), icon: HomeIcon },
    { id: "history", label: t("nav.history", "History"), icon: HistoryIcon },
    ...(settings?.show_logs_tab
      ? [{ id: "logs" as const, label: t("nav.logs", "Logs"), icon: ScrollText }]
      : []),
    { id: "settings", label: t("nav.settings", "Settings"), icon: SettingsIcon },
  ];

  return (
    <div className="min-h-screen bg-background text-foreground flex flex-col font-sans selection:bg-primary/20 selection:text-primary">
      <ComplianceDisclaimer />
      
      {/* Dashboard Top Header */}
      <header className="sticky top-0 z-50 border-b border-border/70 bg-background/85 backdrop-blur-xl transition-all shadow-2xs">
        <div className="mx-auto flex h-16 max-w-5xl items-center justify-between px-6">
          {/* Logo & Brand */}
          <div className="group flex items-center gap-3.5 cursor-pointer" onClick={() => setRoute("home")}>
            <div className="relative flex h-9 w-9 items-center justify-center rounded-md bg-primary text-primary-foreground shadow-sm transition-all duration-300 group-hover:scale-105 group-hover:bg-primary/90 group-active:scale-95">
              <Download className="h-4.5 w-4.5 stroke-[2.5] transition-transform duration-300 group-hover:translate-y-0.5" />
              <span className="absolute -top-0.5 -right-0.5 flex h-2.5 w-2.5">
                <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75"></span>
                <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500"></span>
              </span>
            </div>
            <div className="flex items-center gap-2.5">
              <span className="text-base font-bold tracking-tight text-foreground transition-colors group-hover:text-primary">{t("app.title")}</span>
              <span className="hidden sm:inline-flex items-center gap-1 rounded-md bg-primary/10 px-2 py-0.5 text-xs font-semibold text-primary">
                <Sparkles className="h-3 w-3 animate-pulse text-primary" /> PRO
              </span>
            </div>
          </div>

          {/* Flat Navbar Link Navigation */}
          <nav className="flex items-center gap-8">
            {navItems.map((item) => {
              const isActive = effectiveRoute === item.id;
              const Icon = item.icon;
              return (
                <button
                  key={item.id}
                  onClick={() => setRoute(item.id)}
                  className={`group relative flex items-center gap-2.5 py-5 text-sm font-semibold transition-colors duration-200 ${
                    isActive 
                      ? "text-primary" 
                      : "text-muted-foreground hover:text-foreground"
                  }`}
                >
                  <Icon className={`h-4.5 w-4.5 transition-transform duration-200 ${isActive ? "text-primary scale-110" : "group-hover:scale-110"}`} />
                  <span>{item.label}</span>
                  {isActive && (
                    <span className="absolute bottom-0 left-0 right-0 h-[2.5px] rounded-full bg-primary animate-in fade-in slide-in-from-bottom-1 zoom-in-95 duration-200 ease-out shadow-2xs" />
                  )}
                </button>
              );
            })}
          </nav>

          {/* Right Action Controls */}
          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>
      </header>

      {/* Main Content with Smooth Spring-like CSS Animation */}
      <main className="flex-1 py-4">
        <div className={effectiveRoute === "home" ? "block animate-in fade-in-50 slide-in-from-bottom-2 zoom-in-98 duration-300 ease-out" : "hidden"}>
          <Home />
        </div>
        <div className={effectiveRoute === "history" ? "block animate-in fade-in-50 slide-in-from-bottom-2 zoom-in-98 duration-300 ease-out" : "hidden"}>
          <History />
        </div>
        <div className={effectiveRoute === "logs" ? "block animate-in fade-in-50 slide-in-from-bottom-2 zoom-in-98 duration-300 ease-out" : "hidden"}>
          <Logs />
        </div>
        <div className={effectiveRoute === "settings" ? "block animate-in fade-in-50 slide-in-from-bottom-2 zoom-in-98 duration-300 ease-out" : "hidden"}>
          <Settings />
        </div>
      </main>

      <Toaster />
    </div>
  );
}

function App() {
  return (
    <ThemeProvider>
      <AppShell />
    </ThemeProvider>
  );
}

export default App;
