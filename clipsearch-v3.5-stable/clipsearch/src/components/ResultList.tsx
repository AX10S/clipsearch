// Scrollable results list. Filters results by the active tab and renders
// one ResultRow per visible item.
//
// The filter logic lives in `@/lib/visibleResults.ts` and is SHARED with
// useKeyboardNav — so the row the user SEES is always the same row that
// Enter / Ctrl+Enter / Ctrl+P will act on. This prevents the bug where
// keyboard activation hits an invisible row.

import { useEffect, useMemo, useRef } from "react";
import { useStore } from "@/store/useStore";
import { ResultRow } from "./ResultRow";
import { t } from "@/i18n";
import { getVisibleResults } from "@/lib/visibleResults";

export function ResultList() {
  const results = useStore((s) => s.results);
  const selectedIndex = useStore((s) => s.selectedIndex);
  const activeFilter = useStore((s) => s.activeFilter);
  const containerRef = useRef<HTMLDivElement>(null);

  // Subscribe to injectedKeys + favoriteKeys + query + duplicateFilePaths
  // so the memo recomputes when any of them change.
  const injectedKeys = useStore((s) => s.injectedKeys);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const query = useStore((s) => s.query);
  const duplicateFilePaths = useStore((s) => s.duplicateFilePaths);

  // Compute visible results via the SHARED selector (also used by keyboard nav).
  const filtered = useMemo(
    () => getVisibleResults({ results, activeFilter, injectedKeys, favoriteKeys, query, duplicateFilePaths }),
    [results, activeFilter, injectedKeys, favoriteKeys, query, duplicateFilePaths]
  );

  // Auto-scroll the selected row into view as the user arrows through.
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const child = container.children[selectedIndex] as HTMLElement | undefined;
    if (!child) return;
    const childRect = child.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();
    if (childRect.top < containerRect.top) {
      child.scrollIntoView({ block: "nearest" });
    } else if (childRect.bottom > containerRect.bottom) {
      child.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  if (filtered.length === 0) {
    return (
      <div className="flex-1 flex items-center justify-center px-8 py-12 text-center">
        <div className="text-sm text-black/40 dark:text-white/40">
          {t("search.no_results")}
        </div>
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className="flex-1 overflow-y-auto"
      style={{ padding: "8px" }}
      role="listbox"
    >
      {filtered.map((r, i) => (
        <ResultRow key={keyOf(r)} result={r} index={i} />
      ))}
    </div>
  );
}

function keyOf(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "clipboard":
      return `clip-${r.id}`;
    case "note":
      return `note-${r.id}`;
    case "file":
      return `file-${r.path}`;
    case "app":
      return `app-${r.exec_path}`;
    case "web":
      return `web-${r.url}`;
    case "itsm":
      return `itsm-${r.url}`;
    case "onenote":
      return `onenote-${r.path}`;
    case "confl":
      return `confl-${r.url}`;
    case "jira":
      return `jira-${r.url}`;
  }
}
