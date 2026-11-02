// ─────────────────────────────────────────────────────────────────────────────
// ClipSearch — Launcher root (Graphite design system).
//
// Layout:
//   ┌──────────────────────────────────────────────────────┐
//   │  [Search bar card — 56px]                            │
//   │  [Filter tabs card — 52px]                           │
//   │  ┌────────────────────┐  ┌────────────────────────┐  │
//   │  │ Results (57%)      │  │ Preview (43%)          │  │
//   │  └────────────────────┘  └────────────────────────┘  │
//   │  [Action bar card — 52px]                            │
//   └──────────────────────────────────────────────────────┘
// ─────────────────────────────────────────────────────────────────────────────

import { useEffect } from "react";
import { useStore } from "@/store/useStore";
import { useKeyboardNav } from "@/hooks/useKeyboardNav";
import { SearchBar } from "@/components/SearchBar";
import { FilterTabs } from "@/components/FilterTabs";
import { ResultList } from "@/components/ResultList";
import { PreviewPane } from "@/components/PreviewPane";
import { ActionBar } from "@/components/ActionBar";
import { SettingsPanel } from "@/components/SettingsPanel";
import logoDark from "@/assets/clipsearch-logo.png";
import logoLight from "@/assets/clipsearch-logo-light.png";

export default function App() {
  const theme = useStore((s) => s.theme);
  const compactMode = useStore((s) => s.settings?.compact_mode ?? false);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const refreshSettings = useStore((s) => s.refreshSettings);
  const refreshIndexerStatus = useStore((s) => s.refreshIndexerStatus);
  const applyIndexEvent = useStore((s) => s.applyIndexEvent);
  const setQuery = useStore((s) => s.setQuery);

  useKeyboardNav();

  // Sync the <html> class with the current theme so the body background
  // matches (prevents transparent gaps at window corners).
  useEffect(() => {
    document.documentElement.classList.remove("dark", "light");
    document.documentElement.classList.add(theme);
  }, [theme]);

  useEffect(() => {
    refreshSettings();
    refreshIndexerStatus();
    setQuery("");

    let unlistenIndex: (() => void) | undefined;
    import("@/lib/tauri").then((tauri) => {
      tauri.onIndexEvent(applyIndexEvent).then((un) => (unlistenIndex = un));
    });

    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onThemeChange = () => useStore.getState().applyTheme();
    mq.addEventListener("change", onThemeChange);

    return () => {
      unlistenIndex?.();
      mq.removeEventListener("change", onThemeChange);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className={`launcher-surface ${theme} ${compactMode ? "compact" : ""} animate-fade-in`}>
      <div className="drag-region" />

      {/* Search bar + ClipSearch logo + settings gear (side by side).
          Tight gap (6px) so the logo sits close to the search bar and the
          settings gear — previously gap-2.5 (10px) felt too wide. */}
      <div className="flex items-center" style={{ flexShrink: 0, gap: "6px" }}>
        <SearchBar />
        {/* ClipSearch logo — uses the dark-themed PNG in dark mode and the
            light-themed PNG in light mode. Height matches the search bar
            (56px) so the logo sits visually aligned with the input. */}
        <img
          src={theme === "dark" ? logoDark : logoLight}
          alt="ClipSearch"
          draggable={false}
          style={{
            height: "56px",
            width: "auto",
            flexShrink: 0,
            userSelect: "none",
            pointerEvents: "none",
            objectFit: "contain",
            padding: "2px 4px",
          }}
        />
        <button
          className="action-btn shrink-0"
          onClick={() => useStore.getState().openSettings()}
          title="Настройки"
          style={{ width: "56px", height: "56px" }}
        >
          <svg
            style={{ width: "22px", height: "22px" }}
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
      </div>

      {/* Filter tabs — flat, no card */}
      <FilterTabs />

      {/* Two-column body: results (57%) + preview (43%), 16px gap */}
      <div className="flex-1 flex min-h-0" style={{ gap: "16px", minHeight: "0" }}>
        <div className="card flex flex-col overflow-hidden" style={{ flex: "57", minWidth: "0" }}>
          <ResultList />
        </div>
        <div style={{ flex: "43", minWidth: "0", display: "flex", flexDirection: "column", minHeight: "0" }}>
          <PreviewPane />
        </div>
      </div>

      {/* Action bar — flat, no card */}
      <ActionBar />

      {settingsOpen && <SettingsPanel />}
    </div>
  );
}
