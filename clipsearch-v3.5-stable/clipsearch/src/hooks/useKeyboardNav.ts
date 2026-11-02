// ─────────────────────────────────────────────────────────────────────────────
// Keyboard navigation hook.
//
// Binds the global keyboard handlers for the launcher:
//   ↑ / ↓       — move selection
//   Enter       — activate (copy / open / launch / browse)
//   Cmd/Ctrl+Enter — reveal in file manager (for file/app/note)
//   Cmd/Ctrl+O  — open the selected note in Obsidian
//   Cmd/Ctrl+R  — reveal the selected note/file/app in the file manager
//   Cmd/Ctrl+P  — pin/unpin the selected result (clipboard/note)
//   Esc         — close the launcher (or settings panel if open)
//   Cmd/Ctrl+,  — open settings
// ─────────────────────────────────────────────────────────────────────────────

import { useEffect } from "react";
import { useStore } from "@/store/useStore";
import { isMac } from "@/lib/utils";
import { getVisibleResults } from "@/lib/visibleResults";
import * as tauri from "@/lib/tauri";

export function useKeyboardNav() {
  const moveSelection = useStore((s) => s.moveSelection);
  const selectFirst = useStore((s) => s.selectFirst);
  const selectLast = useStore((s) => s.selectLast);
  const results = useStore((s) => s.results);
  const activeFilter = useStore((s) => s.activeFilter);
  const setFilter = useStore((s) => s.setFilter);
  const selectedIndex = useStore((s) => s.selectedIndex);
  const settingsOpen = useStore((s) => s.settingsOpen);
  const openSettings = useStore((s) => s.openSettings);
  const closeSettings = useStore((s) => s.closeSettings);
  // Subscribe to injectedKeys + favoriteKeys + query so the effect rebinds
  // when they change (the visible-results computation depends on them).
  const injectedKeys = useStore((s) => s.injectedKeys);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const query = useStore((s) => s.query);
  const duplicateFilePaths = useStore((s) => s.duplicateFilePaths);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = isMac() ? e.metaKey : e.ctrlKey;

      const target = e.target as HTMLElement;

      if (settingsOpen) {
        if (e.key === "Escape") {
          e.preventDefault();
          closeSettings();
        }
        return;
      }

      // ── If we're in a TEXTAREA (content editing) ──
      // Only intercept Esc (close launcher), let everything else through.
      if (target.tagName === "TEXTAREA") {
        if (e.key === "Escape" && !mod) {
          e.preventDefault();
          tauri.hideLauncher();
        }
        return;
      }

      // ── If we're in an INPUT that is NOT the search bar (e.g. title edit) ──
      // Only intercept Esc, let Enter/arrows go to the input's own handler.
      if (target.tagName === "INPUT" && !target.classList.contains("search-input")) {
        if (e.key === "Escape" && !mod) {
          e.preventDefault();
          tauri.hideLauncher();
        }
        return;
      }

      // ── If we're in the SEARCH INPUT ──
      // ArrowLeft/Right/Up/Down/Home/End → still switch tabs / navigate list
      // (the user explicitly asked to keep tab switching with arrows).
      // Ctrl+C / Ctrl+V / Ctrl+X / Ctrl+A → let the browser handle them
      // natively so the user can copy/paste/cut/select-all in the input.
      const inSearchInput =
        target.tagName === "INPUT" && target.classList.contains("search-input");

      if (inSearchInput && mod && ["KeyC", "KeyV", "KeyX", "KeyA"].includes(e.code)) {
        // Don't intercept — browser will handle copy/paste/cut/select-all.
        return;
      }

      // ── Enter handling — handle BEFORE the ArrowUp/Down/Esc/etc. checks
      // below, because those checks have early `return` statements that
      // would prevent Enter (with modifiers) from reaching the activation
      // logic at the bottom of the function.
      //
      // We compute the visible results here too (same as ResultList) so
      // that Enter/Ctrl+Enter/Ctrl+Shift+Enter act on the SAME row the
      // user sees highlighted.
      if (e.key === "Enter") {
        const visible = getVisibleResults({
          results,
          activeFilter,
          injectedKeys: useStore.getState().injectedKeys,
          favoriteKeys: useStore.getState().favoriteKeys,
          query: useStore.getState().query,
          duplicateFilePaths: useStore.getState().duplicateFilePaths,
        });
        const selected = visible[selectedIndex];
        if (selected) {
          e.preventDefault();
          if (mod && e.shiftKey) {
            // Ctrl+Shift+Enter → open for viewing
            openForViewing(selected);
          } else if (mod && !e.shiftKey) {
            // Ctrl+Enter → reveal in folder
            revealInFolder(selected);
          } else {
            // Plain Enter → primary action (copy / open / launch)
            activatePrimary(selected);
          }
        }
        return;
      }

      // ── Arrow Up/Down — always navigate the list, even when search input has focus ──
      // The search input is an <input> element, and by default arrow keys move
      // the text cursor. We intercept them to navigate results instead.
      if (e.key === "ArrowDown") {
        e.preventDefault();
        moveSelection(1);
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        moveSelection(-1);
        return;
      }

      if (mod && e.key === ",") {
        e.preventDefault();
        openSettings();
        return;
      }

      if (e.key === "Escape") {
        e.preventDefault();
        tauri.hideLauncher();
        return;
      }

      // ── Arrow Left/Right — switch filter tabs (always, even when search
      // input has focus — the user explicitly asked for this behavior) ────
      if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
        e.preventDefault();
        const tabs = ["all", "favorites", "clipboard", "file", "itsm", "confl", "jira", "note", "onenote", "web"] as const;
        const curIdx = tabs.indexOf(activeFilter as typeof tabs[number]);
        if (curIdx === -1) {
          setFilter("all");
          return;
        }
        const delta = e.key === "ArrowRight" ? 1 : -1;
        const nextIdx = (curIdx + delta + tabs.length) % tabs.length;
        setFilter(tabs[nextIdx]);
        return;
      }
      if (e.key === "Home") {
        e.preventDefault();
        selectFirst();
        return;
      }
      if (e.key === "End") {
        e.preventDefault();
        // Use the SAME selector as ResultList so the count matches the
        // visible list (otherwise End would skip past the visible area).
        const visible = getVisibleResults({
          results,
          activeFilter,
          injectedKeys: useStore.getState().injectedKeys,
          favoriteKeys: useStore.getState().favoriteKeys,
          query: useStore.getState().query,
          duplicateFilePaths: useStore.getState().duplicateFilePaths,
        });
        selectLast(visible.length);
        return;
      }

      // Compute the visible results — SAME filter logic as ResultList.
      // CRITICAL: if the keyboard handler used a different filter than
      // what's rendered, Ctrl+P / Ctrl+R / Ctrl+O would act on the wrong
      // row (e.g. an injected favorite that's invisible in "Все").
      //
      // NOTE: Enter (plain / Ctrl / Ctrl+Shift) is already handled ABOVE,
      // before the Arrow/Esc/Home/End checks — this avoids a bug where those
      // early-return checks swallowed Enter before it could reach the
      // activation logic.
      const visible = getVisibleResults({
        results,
        activeFilter,
        injectedKeys: useStore.getState().injectedKeys,
        favoriteKeys: useStore.getState().favoriteKeys,
        query: useStore.getState().query,
          duplicateFilePaths: useStore.getState().duplicateFilePaths,
      });
      const selected = visible[selectedIndex];
      if (!selected) return;

      // ── Ctrl+O: open in Obsidian (notes only) ──────────────────────────────
      // Use `e.code` (physical key) instead of `e.key` (which depends on
      // keyboard layout — Cyrillic layout would produce "щ" instead of "o").
      if (mod && e.code === "KeyO" && selected.source === "note") {
        e.preventDefault();
        tauri.openNote(selected.id).then(() => tauri.hideLauncher());
        return;
      }

      // ── Ctrl+R: reveal in file manager (alias for Ctrl+Enter) ──────────────
      if (mod && e.code === "KeyR") {
        e.preventDefault();
        revealInFolder(selected);
        return;
      }

      // ── Ctrl+P: pin/unpin ──────────────────────────────────────────────────
      // Works for all result types:
      //   clipboard/note → uses the per-table `pinned` column
      //   file/app/web   → uses the `favorites` table
      if (mod && e.code === "KeyP") {
        e.preventDefault();
        if (selected.source === "clipboard") {
          tauri.pinClipboardItem(selected.id, !selected.pinned).then(() => {
            useStore.getState().setQuery(useStore.getState().query);
          });
        } else if (selected.source === "note") {
          tauri.pinNote(selected.id, !selected.pinned).then(() => {
            useStore.getState().setQuery(useStore.getState().query);
          });
        } else {
          // file / app / web / itsm → use favorites table
          const key = favKeyOf(selected);
          const title = favTitleOf(selected);
          const subtitle = favSubtitleOf(selected);
          tauri.toggleFavorite(selected.source, key, title, subtitle, null).then(() => {
            useStore.getState().setQuery(useStore.getState().query);
          });
        }
        return;
      }
    }

    window.addEventListener("keydown", onKey, true);

    // ── Global typing redirect ─────────────────────────────────────────────
    // If the user presses a printable key and the focus is NOT in a textarea
    // or input, redirect the keystroke to the search bar. This makes the
    // launcher behave like Spotlight/Raycast — you can always just start typing.
    function onGlobalKey(e: KeyboardEvent) {
      // Don't interfere if settings are open or we're already in an input.
      if (useStore.getState().settingsOpen) return;
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;
      // Don't interfere with modifier combos (Ctrl+X, Cmd+X, etc.)
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      // Only redirect printable single-char keys.
      if (e.key.length !== 1) return;
      // Skip function keys, arrows, etc. (already handled by length === 1)
      // Focus the search input and let the event propagate naturally.
      const input = document.querySelector<HTMLInputElement>(".search-input");
      if (input && document.activeElement !== input) {
        input.focus();
        // The keystroke will now be received by the input since it has focus.
      }
    }
    window.addEventListener("keydown", onGlobalKey, true);

    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("keydown", onGlobalKey, true);
    };
  }, [
    moveSelection,
    selectFirst,
    selectLast,
    results,
    activeFilter,
    setFilter,
    selectedIndex,
    settingsOpen,
    openSettings,
    closeSettings,
    injectedKeys,
    favoriteKeys,
    query,
    duplicateFilePaths,
  ]);
}

async function activatePrimary(result: tauri.SearchResult) {
  switch (result.source) {
    case "clipboard":
      await tauri.copyClipboardItem(result.id);
      tauri.hideLauncher();
      break;
    case "note":
      await tauri.openNote(result.id);
      tauri.hideLauncher();
      break;
    case "file":
      // Enter on a file → copy the file itself to clipboard (like Ctrl+C in Explorer).
      await tauri.copyFileToClipboard(result.path);
      tauri.hideLauncher();
      break;
    case "app":
      await tauri.openApp(result.exec_path);
      tauri.hideLauncher();
      break;
    case "web":
    case "itsm":
    case "confl":
    case "jira":
      await tauri.openWebUrl(result.url);
      tauri.hideLauncher();
      break;
    case "onenote":
      await tauri.openOnenote(result.path);
      tauri.hideLauncher();
      break;
  }
}

async function revealInFolder(result: tauri.SearchResult) {
  switch (result.source) {
    case "note":
      await tauri.revealNote(result.id);
      break;
    case "file":
      await tauri.revealFile(result.path);
      break;
    case "app":
      await tauri.revealApp(result.exec_path);
      break;
    case "clipboard":
      // For clipboard-FILES, reveal the first copied file in the file
      // manager. For clipboard text/image, there's no folder to reveal.
      if ((result as any).kind === "files") {
        const firstPath = ((result as any).content as string).split("\n")[0];
        if (firstPath && firstPath.trim().length > 0) {
          await tauri.revealFile(firstPath.trim());
        }
      }
      break;
    case "web":
    case "itsm":
    case "confl":
    case "jira":
      // No folder to reveal for these sources.
      break;
  }
}

/// Ctrl+Shift+Enter → open the file/app/note for viewing.
async function openForViewing(result: tauri.SearchResult) {
  switch (result.source) {
    case "file":
      await tauri.openFile(result.path);
      tauri.hideLauncher();
      break;
    case "app":
      await tauri.openApp(result.exec_path);
      tauri.hideLauncher();
      break;
    case "note":
      await tauri.openNote(result.id);
      tauri.hideLauncher();
      break;
    case "clipboard":
      // For clipboard-FILES, open the first file for viewing.
      // For clipboard text/image, fall back to copy+close (same as Enter).
      if ((result as any).kind === "files") {
        const firstPath = ((result as any).content as string).split("\n")[0];
        if (firstPath && firstPath.trim().length > 0) {
          await tauri.openFile(firstPath.trim());
          tauri.hideLauncher();
        }
      } else {
        await tauri.copyClipboardItem(result.id);
        tauri.hideLauncher();
      }
      break;
    case "web":
    case "itsm":
    case "confl":
    case "jira":
      await tauri.openWebUrl(result.url);
      tauri.hideLauncher();
      break;
    case "onenote":
      await tauri.openOnenote(result.path);
      tauri.hideLauncher();
      break;
  }
}

/// Get the stable key for a file/app/web result (used for favorites).
function favKeyOf(r: tauri.SearchResult): string {
  switch (r.source) {
    case "file":
      return r.path;
    case "app":
      return r.exec_path;
    case "web":
      return r.url;
    case "itsm":
      return r.url;
    case "onenote":
      return r.path;
    case "confl":
      return r.url;
    case "jira":
      return r.url;
    default:
      return "";
  }
}

function favTitleOf(r: tauri.SearchResult): string {
  switch (r.source) {
    case "file":
    case "app":
    case "onenote":
      return r.name;
    case "web":
      return r.display_text;
    case "itsm":
    case "confl":
    case "jira":
      return r.query;
    default:
      return "";
  }
}

function favSubtitleOf(r: tauri.SearchResult): string | null {
  switch (r.source) {
    case "file":
      return r.path;
    case "app":
      return r.exec_path;
    case "web":
      return r.provider;
    case "onenote":
      return r.path;
    case "itsm":
    case "confl":
    case "jira":
      return r.url;
    default:
      return null;
  }
}
