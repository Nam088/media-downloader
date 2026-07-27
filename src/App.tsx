import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useTheme } from "next-themes";
import { Download, Sparkles, Home as HomeIcon, History as HistoryIcon, Settings as SettingsIcon, ScrollText, Library as LibraryIcon } from "lucide-react";
import { ThemeProvider } from "@/components/theme-provider";
import { ComplianceDisclaimer } from "@/components/ComplianceDisclaimer";
import { ThemeToggle } from "@/components/ThemeToggle";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { Toaster } from "@/components/ui/sonner";
import { useAppSettings } from "@/hooks/use-app-settings";
import { Home } from "@/pages/Home";
import { History } from "@/pages/History";
import { Library } from "@/pages/Library";
import { Settings } from "@/pages/Settings";
import { Logs } from "@/pages/Logs";

import { SecretMessageModal } from "@/components/SecretMessageModal";
import { useKeyboardShortcuts } from "@/hooks/use-keyboard-shortcuts";

type Route = "home" | "library" | "history" | "logs" | "settings";

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
  const [showSecretModal, setShowSecretModal] = useState(false);
  const { settings } = useAppSettings();
  useSyncBackendSettings();

  useKeyboardShortcuts({
    onSecretTrigger: () => {
      setShowSecretModal((prev) => !prev);
    },
    onSearchFocus: () => {
      if (route !== "library" && route !== "history") {
        setRoute("library");
      }
      setTimeout(() => {
        const searchInput =
          document.querySelector<HTMLInputElement>("input[type='search']") ||
          document.querySelector<HTMLInputElement>("input[placeholder*='search']") ||
          document.querySelector<HTMLInputElement>("input[placeholder*='tìm']") ||
          document.querySelector<HTMLInputElement>("input[data-testid*='search']");
        if (searchInput) {
          searchInput.focus();
          searchInput.select();
        }
      }, 50);
    },
  });

  // Derived at render time rather than corrected via a setState-in-effect
  // redirect: if the Logs tab gets hidden (Settings toggle) while it's the
  // active route, this just falls back to "home" for that render — no
  const effectiveRoute: Route = route === "logs" && settings && !settings.show_logs_tab ? "home" : route;

  const navItems: { id: Route; label: string; icon: typeof HomeIcon }[] = [
    { id: "home", label: t("nav.home", "Home"), icon: HomeIcon },
    { id: "library", label: t("nav.library"), icon: LibraryIcon },
    { id: "history", label: t("nav.history", "History"), icon: HistoryIcon },
    ...(settings?.show_logs_tab
      ? [{ id: "logs" as const, label: t("nav.logs", "Logs"), icon: ScrollText }]
      : []),
    { id: "settings", label: t("nav.settings", "Settings"), icon: SettingsIcon },
  ];

  return (
    <div className="min-h-screen w-full max-w-full overflow-x-hidden overscroll-none bg-background text-foreground flex flex-col font-sans selection:bg-primary/20 selection:text-primary">
      <ComplianceDisclaimer />
      
      <header className="fixed top-0 left-0 right-0 z-50 w-full select-none border-b border-border/70 bg-background/85 backdrop-blur-xl transition-all shadow-2xs">
        <div className="mx-auto flex h-16 max-w-5xl items-center justify-between px-6">
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

          <nav className="flex items-center gap-3 sm:gap-6 md:gap-8">
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
                    <span className="absolute bottom-0 left-0 right-0 h-[2.5px] rounded-full bg-primary shadow-2xs transition-all duration-200 ease-out animate-in fade-in-50 zoom-in-95" />
                  )}
                </button>
              );
            })}
          </nav>

          <div className="flex items-center gap-2">
            <ThemeToggle />
            <LanguageSwitcher />
          </div>
        </div>
      </header>

      <main className="flex-1 pt-16 py-4">
        <div className={effectiveRoute === "home" ? "block animate-in fade-in-50 duration-150" : "hidden"}>
          <Home />
        </div>
        <div className={effectiveRoute === "library" ? "block animate-in fade-in-50 duration-150" : "hidden"}>
          <Library active={effectiveRoute === "library"} />
        </div>
        <div className={effectiveRoute === "history" ? "block animate-in fade-in-50 duration-150" : "hidden"}>
          <History />
        </div>
        <div className={effectiveRoute === "logs" ? "block animate-in fade-in-50 duration-150" : "hidden"}>
          <Logs />
        </div>
        <div className={effectiveRoute === "settings" ? "block animate-in fade-in-50 duration-150" : "hidden"}>
          <Settings />
        </div>
      </main>

      <SecretMessageModal open={showSecretModal} onClose={() => setShowSecretModal(false)} />
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
