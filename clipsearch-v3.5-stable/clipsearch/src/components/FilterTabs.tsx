// FilterTabs — premium pill-shaped filter tabs (36px height, 18px radius).
// Uses CSS Variables. Active tab = accent purple, inactive = dark grey.
// Tabs are text-only (no icons) per user request — 2026-10-28.

import { useStore } from "@/store/useStore";
import { t } from "@/i18n";
import type { FilterSource } from "@/lib/tauri";
import type { SearchResult } from "@/lib/tauri";

export function FilterTabs() {
  const results = useStore((s) => s.results);
  const activeFilter = useStore((s) => s.activeFilter);
  const setFilter = useStore((s) => s.setFilter);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const injectedKeys = useStore((s) => s.injectedKeys);
  const query = useStore((s) => s.query);
  const settings = useStore((s) => s.settings);

  // On empty query, the "Все" tab count should exclude injected favorites
  // (they're hidden from the "Все" tab list — see visibleResults.ts).
  // On non-empty query, all results are visible (matching favorites are
  // NOT marked as injected).
  const visibleForAllTab = query.trim().length === 0
    ? results.filter((r) => !injectedKeys.has(`${r.source}:${favKeyOf(r)}`))
    : results;

  const counts = countBySource(visibleForAllTab);
  const favCount = results.filter((r) => {
    if ("pinned" in r && r.pinned) return true;
    if (r.source === "file" || r.source === "app" || r.source === "web" || r.source === "itsm" || r.source === "onenote" || r.source === "confl" || r.source === "jira") {
      const key = favKeyOf(r);
      return favoriteKeys.has(`${r.source}:${key}`);
    }
    return false;
  }).length;

  // Check which sources are enabled in settings.
  // The FilterSource type uses singular "file" and "note", but the settings
  // struct uses plural "files" and "obsidian" — map accordingly.
  const enabled = settings?.enabled_sources;
  const isSourceEnabled = (src: FilterSource): boolean => {
    if (src === "all" || src === "favorites") return true;
    const key =
      src === "note" ? "obsidian" :
      src === "file" ? "files" :
      src;
    return enabled?.[key as keyof typeof enabled] ?? true;
  };

  const allTabs: { source: FilterSource; label: string }[] = [
    { source: "all", label: "Все" },
    { source: "favorites", label: "Избранные" },
    { source: "clipboard", label: t("source.clipboard") },
    { source: "file", label: "Файлы" },
    { source: "itsm", label: "ITSM" },
    { source: "confl", label: "SW Conf" },
    { source: "jira", label: "SW Jira" },
    { source: "note", label: t("source.note") },
    { source: "onenote", label: "OneNote" },
    { source: "web", label: "Интернет" },
  ];

  // Filter out disabled sources.
  const tabs = allTabs.filter((tab) => isSourceEnabled(tab.source));

  return (
    <div
      className="no-drag flex items-center gap-2 overflow-x-auto"
      style={{ height: "40px", padding: "0 4px", flexShrink: 0 }}
    >
      {tabs.map((tab) => {
        const count = tab.source === "all"
          ? visibleForAllTab.length
          : tab.source === "favorites"
          ? favCount
          : counts[tab.source] ?? 0;
        const isActive = activeFilter === tab.source;
        return (
          <button
            key={tab.source}
            onClick={() => setFilter(tab.source)}
            className={`filter-tab ${isActive ? "active" : "inactive"}`}
          >
            <span>{tab.label}</span>
            <span className="filter-count">{count}</span>
          </button>
        );
      })}
    </div>
  );
}

function countBySource(results: SearchResult[]): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const r of results) {
    counts[r.source] = (counts[r.source] ?? 0) + 1;
  }
  return counts;
}

function favKeyOf(r: SearchResult): string {
  switch (r.source) {
    case "file": return r.path;
    case "app": return r.exec_path;
    case "web": return r.url;
    case "itsm": return r.url;
    case "onenote": return r.path;
    case "confl": return r.url;
    case "jira": return r.url;
    case "clipboard": return `clip:${r.id}`;
    case "note": return `note:${r.id}`;
  }
}
