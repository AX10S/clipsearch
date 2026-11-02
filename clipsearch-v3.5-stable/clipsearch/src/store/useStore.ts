// ─────────────────────────────────────────────────────────────────────────────
// Zustand store — the single source of truth for the launcher UI.
//
// Holds:
//   • the current query + debounced search results
//   • the active selection (for keyboard navigation)
//   • the current settings + indexer status
//   • the "settings panel open" flag
//
// All Tauri IPC calls happen here; components only read from the store and
// dispatch actions. This keeps the components dumb and easy to reason about.
// ─────────────────────────────────────────────────────────────────────────────

import { create } from "zustand";
import {
  getSettings,
  getObsidianStatus,
  search as searchIpc,
  setObsidianVault as setObsidianVaultIpc,
  reindexVault as reindexVaultIpc,
  updateSettings as updateSettingsIpc,
  clearClipboardHistory as clearClipboardHistoryIpc,
  getFavorites,
  type SearchResult,
  type Settings,
  type IndexerStatus,
  type IndexEvent,
} from "@/lib/tauri";
import { debounce } from "@/lib/utils";
import { getVisibleResults } from "@/lib/visibleResults";

/// Merge favorites from DB into search results.
///
/// Behavior:
///   - EMPTY query: injects ALL favorites (so the "Избранные" tab works).
///     They are marked as `injected` so the "Все" tab can filter them out
///     (user requirement: favorites shouldn't show in "Все" by default).
///   - NON-EMPTY query: injects favorites whose title/subtitle/key matches
///     the query (so favorited files/URLs can be found by search). These are
///     NOT marked as injected — they appear in "Все" normally.
///   - Favorites from DB replace existing items (for updated custom titles).
///   - Final sort: favorites first, then by score.
async function mergeFavorites(
  results: SearchResult[],
  query: string
): Promise<{ results: SearchResult[]; favKeys: Set<string>; injectedKeys: Set<string>; duplicateFilePaths: Set<string> }> {
  const favKeys = new Set<string>();
  const injectedKeys = new Set<string>();

  // Load ALL favorites from DB (file/app/web/itsm + pinned clipboard items).
  let favs: SearchResult[] = [];
  try {
    favs = await getFavorites();
  } catch {}

  // Build favMap: key → favorite (for title replacement + boost detection)
  const favMap = new Map<string, SearchResult>();
  for (const f of favs) {
    const key = `${f.source}:${favKeyOf(f)}`;
    favMap.set(key, f);
    favKeys.add(key);
  }

  // Mark clipboard/note pinned items as favorites
  for (const r of results) {
    if ("pinned" in r && r.pinned) {
      favKeys.add(`${r.source}:${favKeyOf(r)}`);
    }
  }

  // Replace existing results with DB version (has updated custom title).
  // This ensures title changes appear immediately.
  //
  // IMPORTANT: we do NOT blindly spread favVersion over r — that would
  // overwrite r.title (computed by the backend from content) with
  // favVersion.title, which can be empty if the DB has stale data or
  // the backend has a bug. Instead, we copy only the fields that the
  // favorites table actually owns (custom_title, pinned), and keep
  // r.title / r.preview / r.content from the search result.
  const updatedResults = results.map((r) => {
    const key = `${r.source}:${favKeyOf(r)}`;
    const favVersion = favMap.get(key);
    if (favVersion) {
      const merged: SearchResult = { ...r };
      if ("custom_title" in favVersion && favVersion.custom_title) {
        (merged as any).custom_title = favVersion.custom_title;
        if (favVersion.title && favVersion.title.trim().length > 0) {
          (merged as any).title = favVersion.title;
        }
      }
      if ("pinned" in favVersion) {
        (merged as any).pinned = favVersion.pinned;
      }
      return merged;
    }
    return r;
  });

  // ── Compute the set of file paths that are BOTH in clipboard-files AND
  // in indexed files. This set is exposed via the store as `duplicateFilePaths`
  // and is used by `getVisibleResults` to hide the file-search DUPLICATE from
  // the "all" tab (keeping the clipboard entry which has the more recent
  // copy timestamp). The "file" tab is NOT affected — it shows ALL indexed
  // files regardless of whether they were also copied.
  // ────────────────────────────────────────────────────────────────────
  const duplicateFilePaths = new Set<string>();
  for (const r of updatedResults) {
    if (r.source === "clipboard" && (r as any).kind === "files") {
      const firstLine = ((r as any).content as string).split("\n")[0] ?? "";
      if (firstLine.trim().length > 0) {
        duplicateFilePaths.add(firstLine.trim().toLowerCase());
      }
    }
  }
  // Note: we no longer remove file-duplicates from `updatedResults` here.
  // They remain in the results array (so the "file" tab shows all indexed
  // files), but `getVisibleResults` filters them out ONLY for the "all" tab.
  const dedupedResults = updatedResults;

  let allResults: SearchResult[];

  if (query.trim().length === 0) {
    // Empty query — inject favorites NOT already in results at the top.
    //
    // CRITICAL: `injectedKeys` should ONLY contain favorites that are NOT
    // naturally in the search results (e.g. a file favorite for a file
    // that's NOT in an indexed folder, or a web URL favorite).
    //
    // Favorites that ARE in the search results (pinned clipboard items,
    // pinned notes, file favorites for files in indexed folders) must
    // remain VISIBLE in the "Все" tab — they show with a star icon.
    //
    // Why: when the user favorites a visible row, that row should NOT
    // disappear from "Все". If we marked ALL favorites as injected,
    // favoriting a row would hide it → the row "disappears" → the
    // selection jumps to row 0 (because findIndex returns -1).
    const existingKeys = new Set(dedupedResults.map((r) => `${r.source}:${favKeyOf(r)}`));
    const newFavs = favs.filter((f) => {
      const key = `${f.source}:${favKeyOf(f)}`;
      return !existingKeys.has(key);
    });
    const newFavsWithTitles = newFavs.map(ensureTitle);
    // Mark ONLY the injected favorites (those not in search results) as
    // injected. Favorites already in search results stay visible in "Все".
    for (const f of newFavsWithTitles) {
      injectedKeys.add(`${f.source}:${favKeyOf(f)}`);
    }
    allResults = [...newFavsWithTitles, ...dedupedResults];
  } else {
    // Non-empty query — also inject favorites whose title/subtitle/key
    // contains the query string. These are NOT marked as injected
    // (because they match the search and should appear in "Все").
    const q = query.trim().toLowerCase();
    const existingKeys = new Set(dedupedResults.map((r) => `${r.source}:${favKeyOf(r)}`));
    const matchingFavs = favs.filter((f) => {
      const key = `${f.source}:${favKeyOf(f)}`;
      if (existingKeys.has(key)) return false;
      const title = (favTitleOf(f) || "").toLowerCase();
      const subtitle = (favSubtitleOf(f) || "").toLowerCase();
      const keyLower = (favKeyOf(f) || "").toLowerCase();
      return (
        title.includes(q) ||
        subtitle.includes(q) ||
        keyLower.includes(q)
      );
    });
    allResults = [...matchingFavs.map(ensureTitle), ...dedupedResults];
  }

  // ── Preserve the previous ordering when the query is empty ─────────────
  // CRITICAL: when the user favorites a row, `mergeFavorites` runs again
  // (because `favKeys` changed). Even without a favorite-boost, the new sort
  // can re-order rows that have the SAME score (unstable secondary order).
  // That makes the favorited row visually "jump" to a new position.
  //
  // To prevent this, on empty query we keep the natural order produced by
  // the backend (which sorts by recency). The only additions we make are
  // injected favorites at the TOP — those are already hidden from the "Все"
  // tab, so they don't affect the visible order.
  //
  // For non-empty query, the score reflects search relevance, so re-sorting
  // is safe and expected.
  if (query.trim().length === 0) {
    // Don't re-sort — keep the order as-is (backend recency order + injected
    // favorites at the top).
  } else {
    allResults.sort((a, b) => (b.score || 0) - (a.score || 0));
  }

  return { results: allResults, favKeys, injectedKeys, duplicateFilePaths };
}

/// Extract a display title from a SearchResult (for favorites matching).
function favTitleOf(r: SearchResult): string {
  switch (r.source) {
    case "file": return r.name;
    case "app": return r.name;
    case "web": return r.display_text;
    case "itsm": return r.query;
    case "onenote": return r.name;
    case "confl": return r.query;
    case "jira": return r.query;
    case "clipboard": return r.custom_title || r.title;
    case "note": return r.title;
  }
}

/// Extract a subtitle/path for matching.
function favSubtitleOf(r: SearchResult): string {
  switch (r.source) {
    case "file": return r.path;
    case "app": return r.exec_path;
    case "web": return r.url;
    case "itsm": return r.url;
    case "onenote": return r.path;
    case "confl": return r.url;
    case "jira": return r.url;
    case "note": return r.rel_path;
    case "clipboard": return r.preview;
  }
}

/// Ensure a SearchResult has a non-empty title. If the backend returned an
/// empty title (e.g. due to a bug or stale DB row), fall back to:
///   1. custom_title (if set)
///   2. preview (first line of content / path / URL)
///   3. name / display_text / query (depending on source)
///   4. "<no title>" placeholder
///
/// This is a DEFENSIVE measure — the backend should always return a non-empty
/// title, but if it doesn't, we don't want the UI to show "(empty)".
function ensureTitle(r: SearchResult): SearchResult {
  // Check if title is already non-empty.
  const title = (r as any).title as string | undefined;
  if (title && title.trim().length > 0) {
    return r;
  }
  // Compute a fallback title.
  let fallback = "";
  switch (r.source) {
    case "clipboard":
      // For clipboard items, custom_title takes priority, then preview
      // (which is the first line of content / "[Image NxM]" / file path).
      if (r.custom_title && r.custom_title.trim().length > 0) {
        fallback = r.custom_title;
      } else if (r.preview && r.preview.trim().length > 0) {
        // For files, the preview is the full path. Show just the filename
        // for cleaner display.
        const firstLine = r.preview.split("\n")[0] || "";
        const basename = firstLine.split(/[\\/]/).pop() || firstLine;
        fallback = basename || firstLine || "[Без названия]";
      } else if (r.content) {
        const firstLine = r.content.split("\n").find((l) => l.trim().length > 0) || "";
        const basename = firstLine.split(/[\\/]/).pop() || firstLine;
        fallback = basename || firstLine || "[Без названия]";
      } else {
        fallback = "[Без названия]";
      }
      break;
    case "file":
      fallback = r.name || r.path.split(/[\\/]/).pop() || r.path || "[Без названия]";
      break;
    case "app":
      fallback = r.name || r.exec_path || "[Без названия]";
      break;
    case "web":
      fallback = r.display_text || r.url || "[Без названия]";
      break;
    case "itsm":
      fallback = r.query || r.url || "[Без названия]";
      break;
    case "onenote":
      fallback = r.name || r.path || "[Без названия]";
      break;
    case "note":
      fallback = r.title || r.rel_path || "[Без названия]";
      break;
  }
  return { ...r, title: fallback } as SearchResult;
}

interface State {
  // ── Search ──────────────────────────────────────────────────────────────
  query: string;
  results: SearchResult[];
  selectedIndex: number;
  isSearching: boolean;
  /** Set of favorite keys (source:key) for the current results — used to show
   * the ★ icon and the correct "Unpin"/"Pin" label. */
  favoriteKeys: Set<string>;
  /** Set of keys for favorites that were INJECTED into results on EMPTY query
   * (i.e. they're not naturally in search results). The "Все" tab hides
   * these so favorites don't appear there by default. The "Избранные" tab
   * still shows them because it filters by favoriteKeys, not by injectedKeys.
   * On NON-EMPTY query, this set is empty (matching favorites are NOT marked
   * as injected and appear in "Все" normally). */
  injectedKeys: Set<string>;
  /** Set of lowercase file paths that also exist as clipboard-files.
   *  Used by `getVisibleResults` to hide the file-search duplicate from
   *  the "all" tab (keeping the clipboard entry). The "file" tab still
   *  shows all indexed files. */
  duplicateFilePaths: Set<string>;
  /** Active filter tab — which source to show. "all" = no filter. */
  activeFilter: "all" | "favorites" | "clipboard" | "note" | "file" | "web" | "itsm" | "onenote" | "confl" | "jira";

  // ── Settings ───────────────────────────────────────────────────────────
  settings: Settings | null;
  settingsOpen: boolean;
  indexerStatus: IndexerStatus | null;

  // ── Theme ──────────────────────────────────────────────────────────────
  theme: "light" | "dark";

  // ── Actions ────────────────────────────────────────────────────────────
  setQuery: (q: string) => void;
  setFilter: (f: "all" | "favorites" | "clipboard" | "note" | "file" | "web" | "itsm" | "onenote" | "confl" | "jira") => void;
  moveSelection: (delta: -1 | 1) => void;
  setSelection: (index: number) => void;
  selectFirst: () => void;
  selectLast: (total: number) => void;
  openSettings: () => void;
  closeSettings: () => void;
  refreshSettings: () => Promise<void>;
  refreshIndexerStatus: () => Promise<void>;
  applyIndexEvent: (event: IndexEvent) => void;
  pickVault: (path: string) => Promise<void>;
  reindex: () => Promise<void>;
  updateSettings: (patch: Partial<Settings>) => Promise<void>;
  clearClipboard: () => Promise<void>;
  applyTheme: () => void;
}

// Debounced search: 80ms is short enough to feel instant, long enough to
// coalesce bursts of keystrokes into one query.
// We track the latest query token so an in-flight debounced search for an
// OLD query can detect that the query has changed (e.g. cleared) and refuse
// to overwrite the results — this prevents the "favorites disappear after
// clearing the search" race condition.
let currentQueryToken = "";
let getStoreState: () => State = () => ({ activeFilter: "all" } as State); // overwritten in create()
const debouncedSearch = debounce(async (
  query: string,
  set: (s: Partial<State>) => void,
  prevSelectedKey: string | null,
) => {
  const myToken = query;
  set({ isSearching: true });
  try {
    const searchResults = await searchIpc(query, 50);
    if (currentQueryToken !== myToken) return;
    const { results, favKeys, injectedKeys, duplicateFilePaths } = await mergeFavorites(searchResults, query);
    if (currentQueryToken !== myToken) return;
    // Restore selectedIndex to the previously-selected row, if it's still
    // in the visible results.
    const visible = getVisibleResults({
      results,
      activeFilter: getStoreState().activeFilter,
      injectedKeys,
      favoriteKeys: favKeys,
      query,
      duplicateFilePaths,
    });
    let restoredIndex = 0;
    if (prevSelectedKey) {
      const idx = visible.findIndex((r) => `${r.source}:${favKeyOf(r)}` === prevSelectedKey);
      if (idx >= 0) restoredIndex = idx;
    }
    set({ results, isSearching: false, selectedIndex: restoredIndex, favoriteKeys: favKeys, injectedKeys, duplicateFilePaths });
  } catch (e) {
    console.error("search failed", e);
    if (currentQueryToken !== myToken) return;
    set({ results: [], isSearching: false, favoriteKeys: new Set(), injectedKeys: new Set(), duplicateFilePaths: new Set() });
  }
}, 80);

/// Build the stable key for a result (used for favorites lookup + dedup).
function favKeyOf(r: SearchResult): string {
  switch (r.source) {
    case "file":
      return r.path;
    case "app":
      return r.exec_path;
    case "web":
    case "itsm":
    case "confl":
    case "jira":
      return r.url;
    case "onenote":
      return r.path;
    case "clipboard":
      return `clip:${r.id}`;
    case "note":
      return `note:${r.id}`;
    default:
      return "";
  }
}

function detectTheme(settings: Settings | null): "light" | "dark" {
  const theme = settings?.theme ?? "dark";
  if (theme === "light") return "light";
  return "dark";
}

export const useStore = create<State>((set, get) => {
  // Wire up the store-state accessor used by `debouncedSearch` (which lives
  // outside `create` and doesn't have its own `get`).
  getStoreState = get;
  return {
  query: "",
  results: [],
  selectedIndex: 0,
  isSearching: false,
  favoriteKeys: new Set(),
  injectedKeys: new Set(),
  duplicateFilePaths: new Set(),
  activeFilter: "all",
  settings: null,
  settingsOpen: false,
  indexerStatus: null,
  theme: "dark",

  setQuery: (q) => {
    set({ query: q });
    // Bump the token on every keystroke. Any in-flight debounced search
    // whose token no longer matches `currentQueryToken` will discard its
    // results — this prevents stale results from overwriting fresh ones
    // (the "favorites disappear after clearing" race condition).
    currentQueryToken = q;

    // ── Preserve the selected row across re-queries ─────────────────────
    // When the user toggles favorite on the selected row, `setQuery(same)`
    // is called to refresh results. We capture the selected row's key HERE,
    // then restore `selectedIndex` to the same row after results arrive.
    // Without this, `selectedIndex: 0` reset causes the selection to jump
    // to the top of the list (visible as "the row moved" — but really the
    // selection moved, not the row).
    const prevSelectedKey = (() => {
      const st = get();
      const visible = getVisibleResults({
        results: st.results,
        activeFilter: st.activeFilter,
        injectedKeys: st.injectedKeys,
        favoriteKeys: st.favoriteKeys,
        query: st.query,
        duplicateFilePaths: st.duplicateFilePaths,
      });
      const sel = visible[st.selectedIndex];
      return sel ? `${sel.source}:${favKeyOf(sel)}` : null;
    })();

    if (q.trim().length === 0) {
      // Empty query — show recent items + favorites (injected, hidden from "Все").
      // Use a snapshot of the token at call time so the empty-query result
      // isn't overwritten by a stale debounced search that started before
      // the input was cleared.
      const myToken = q;
      searchIpc("", 50)
        .then(async (searchResults) => {
          if (currentQueryToken !== myToken) return;
          const { results, favKeys, injectedKeys, duplicateFilePaths } = await mergeFavorites(searchResults, q);
          if (currentQueryToken !== myToken) return;
          // Restore selectedIndex to the previously-selected row, if it's
          // still in the visible results. Otherwise fall back to 0.
          const visible = getVisibleResults({ results, activeFilter: get().activeFilter, injectedKeys, favoriteKeys: favKeys, query: q, duplicateFilePaths });
          let restoredIndex = 0;
          if (prevSelectedKey) {
            const idx = visible.findIndex((r) => `${r.source}:${favKeyOf(r)}` === prevSelectedKey);
            if (idx >= 0) restoredIndex = idx;
          }
          set({ results, isSearching: false, selectedIndex: restoredIndex, favoriteKeys: favKeys, injectedKeys, duplicateFilePaths });
        })
        .catch(() => {
          if (currentQueryToken !== myToken) return;
          set({ results: [], favoriteKeys: new Set(), injectedKeys: new Set(), duplicateFilePaths: new Set() });
        });
    } else {
      debouncedSearch(q, set, prevSelectedKey);
    }
  },

  setFilter: (f) => {
    set({ activeFilter: f, selectedIndex: 0 });
  },

  moveSelection: (delta) => {
    // CRITICAL: compute the visible (filtered) results the SAME way as
    // ResultList — otherwise ↑/↓ navigation walks through INVISIBLE rows
    // (e.g. injected favorites hidden from the "Все" tab) and the user
    // sees the selection "disappear" until they press enough times to skip
    // past the invisible block.
    const state = get();
    const visible = getVisibleResults({
      results: state.results,
      activeFilter: state.activeFilter,
      injectedKeys: state.injectedKeys,
      favoriteKeys: state.favoriteKeys,
      query: state.query,
      duplicateFilePaths: state.duplicateFilePaths,
    });
    if (visible.length === 0) return;
    const next = (state.selectedIndex + delta + visible.length) % visible.length;
    set({ selectedIndex: next });
  },

  setSelection: (index) => set({ selectedIndex: Math.max(0, index) }),

  selectFirst: () => set({ selectedIndex: 0 }),

  selectLast: (total) => set({ selectedIndex: Math.max(0, total - 1) }),

  openSettings: () => set({ settingsOpen: true }),
  closeSettings: () => set({ settingsOpen: false }),

  refreshSettings: async () => {
    try {
      const settings = await getSettings();
      set({ settings, theme: detectTheme(settings) });
    } catch (e) {
      console.error("refreshSettings failed", e);
    }
  },

  refreshIndexerStatus: async () => {
    try {
      const status = await getObsidianStatus();
      set({ indexerStatus: status });
    } catch (e) {
      console.error("refreshIndexerStatus failed", e);
    }
  },

  applyIndexEvent: (event) => {
    const status = get().indexerStatus;
    switch (event.kind) {
      case "started":
        // Scan started — mark as scanning.
        set({
          indexerStatus: {
            ...(status ?? {
              vault_path: null,
              indexed_count: 0,
              last_scan_ms: 0,
              scanning: true,
            }),
            scanning: true,
          },
        });
        break;
      case "progress":
        // We could surface done/total in the UI, but for now just refresh
        // the count periodically.
        break;
      case "finished":
        set({
          indexerStatus: {
            ...(status ?? {
              vault_path: null,
              indexed_count: event.indexed,
              last_scan_ms: event.took_ms,
              scanning: false,
            }),
            indexed_count: event.indexed,
            last_scan_ms: event.took_ms,
            scanning: false,
          },
        });
        // Re-run the current search so fresh results show up.
        get().setQuery(get().query);
        break;
      case "file_updated":
      case "file_removed":
        // Live update — debounce a search refresh. Pass null for
        // prevSelectedKey so the selection isn't forcibly preserved
        // (a file indexer event is a global change, not a focused action
        // like toggling a favorite).
        debouncedSearch(get().query, set, null);
        break;
      case "error":
        console.error("indexer error", event.message);
        break;
    }
  },

  pickVault: async (path) => {
    const status = await setObsidianVaultIpc(path);
    set({ indexerStatus: status });
    await get().refreshSettings();
  },

  reindex: async () => {
    await reindexVaultIpc();
  },

  updateSettings: async (patch) => {
    const updated = await updateSettingsIpc(patch);
    set({ settings: updated, theme: detectTheme(updated) });

    // If the currently-active filter was just disabled, fall back to "all"
    // so the user isn't stuck looking at an empty tab whose source is off.
    const active = get().activeFilter;
    if (active !== "all" && active !== "favorites" && updated.enabled_sources) {
      const key =
        active === "note" ? "obsidian" :
        active === "file" ? "files" :
        active;
      const isEnabled = (updated.enabled_sources as Record<string, boolean>)[key] ?? true;
      if (!isEnabled) {
        set({ activeFilter: "all", selectedIndex: 0 });
      }
    }
  },

  clearClipboard: async () => {
    await clearClipboardHistoryIpc();
    // Refresh the current search so deleted items disappear.
    get().setQuery(get().query);
  },

  applyTheme: () => {
    const theme = detectTheme(get().settings);
    set({ theme });
  },
  };
});
