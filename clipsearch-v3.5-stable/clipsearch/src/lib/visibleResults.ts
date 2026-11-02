// Shared selector — returns the SAME filtered results array used by both
// ResultList (for rendering) and useKeyboardNav (for keyboard actions).
//
// CRITICAL: ResultList and useKeyboardNav MUST use this same function.
// Previously each consumer had its own filter code, and they drifted out of
// sync — Enter activated the wrong row because the visible row N was no
// longer the same as the row N used by keyboard logic.

import type { SearchResult } from "@/lib/tauri";

/**
 * Compute the visible results for a given active filter.
 *
 * Behavior:
 *   - "all" tab:
 *     • EMPTY query → hide INJECTED favorites (user requirement: favorites
 *       shouldn't show in "Все" by default) AND hide file-duplicates that
 *       are also in clipboard-files (to avoid showing the same file twice —
 *       once as "БУФЕР" + once as "ФАЙЛ").
 *     • NON-EMPTY query → show everything (matching favorites are NOT
 *       marked as injected — they appear in search results normally).
 *       Deduplication still applies so the same file isn't shown twice.
 *   - "favorites" tab:
 *     • Show all favorites (pinned clipboard/note items + everything in
 *       favoriteKeys).
 *   - "file" tab:
 *     • Show ALL indexed files (NO deduplication — the user needs to see
 *       every indexed file regardless of whether it was also copied).
 *   - other tabs (clipboard / note / web / itsm / onenote):
 *     • Filter by source.
 */
export function getVisibleResults(
  state: {
    results: SearchResult[];
    activeFilter: string;
    injectedKeys: Set<string>;
    favoriteKeys: Set<string>;
    query: string;
    /** Set of lowercase file paths that also exist as clipboard-files.
     *  Used to dedupe in the "all" tab — when a file is both indexed and
     *  copied, we hide the indexed "file" entry from "all" (keeping the
     *  clipboard entry which has the more recent copy timestamp). The
     *  "file" tab is NOT affected and still shows all indexed files. */
    duplicateFilePaths: Set<string>;
  }
): SearchResult[] {
  const { results, activeFilter, injectedKeys, favoriteKeys, query, duplicateFilePaths } = state;

  if (activeFilter === "all") {
    // Hide injected favorites (empty query only) AND hide file-duplicates
    // (always — to avoid showing the same file twice as both clipboard + file).
    return results.filter((r) => {
      // Hide injected favorites on empty query.
      if (query.trim().length === 0 && injectedKeys.has(`${r.source}:${favKeyOf(r)}`)) {
        return false;
      }
      // Hide file-duplicates — when a file is both in clipboard-files AND
      // in indexed files, we keep the clipboard entry (it has the more
      // recent copy timestamp) and drop the file-search duplicate.
      if (r.source === "file" && duplicateFilePaths.has(r.path.toLowerCase())) {
        return false;
      }
      return true;
    });
  }

  if (activeFilter === "favorites") {
    return results.filter((r) => {
      // Pinned clipboard/note items are always favorites.
      if ("pinned" in r && r.pinned) return true;
      // file/app/web/itsm/onenote → check the favorites table via favKeys.
      if (
        r.source === "file" ||
        r.source === "app" ||
        r.source === "web" ||
        r.source === "itsm" ||
        r.source === "onenote"
      ) {
        const key = favKeyOf(r);
        return favoriteKeys.has(`${r.source}:${key}`);
      }
      // Clipboard items not pinned are NOT favorites.
      return false;
    });
  }

  // For "file" tab and other source-specific tabs, do NOT apply deduplication
  // — the user expects to see all indexed files in the "file" tab even if
  // they were also copied to the clipboard.
  return results.filter((r) => r.source === activeFilter);
}

function favKeyOf(r: SearchResult): string {
  switch (r.source) {
    case "clipboard": return `clip:${r.id}`;
    case "note":      return `note:${r.id}`;
    case "file":      return r.path;
    case "app":       return r.exec_path;
    case "web":       return r.url;
    case "itsm":      return r.url;
    case "onenote":   return r.path;
    case "confl":     return r.url;
    case "jira":      return r.url;
  }
}
