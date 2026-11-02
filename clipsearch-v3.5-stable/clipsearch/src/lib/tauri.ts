// ─────────────────────────────────────────────────────────────────────────────
// Tauri IPC wrappers — the only place in the frontend that knows about Tauri.
//
// Every backend command is wrapped in a typed function. Components import
// from here instead of calling `invoke` directly — this gives us a single
// place to add logging, error handling, or retry logic.
// ─────────────────────────────────────────────────────────────────────────────

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

// ── Types matching the Rust side ─────────────────────────────────────────────

export type SearchResult =
  | {
      source: "clipboard";
      id: number;
      title: string;
      preview: string;
      /** Raw content — for files: newline-separated paths; for text: the text itself. */
      content: string;
      /** "text" | "image" | "files" — drives icon & preview choice. */
      kind: string;
      source_app_name: string | null;
      created_at: number;
      last_used_at: number;
      pinned: boolean;
      image_path: string | null;
      custom_title: string | null;
      score: number;
    }
  | {
      source: "note";
      id: number;
      title: string;
      preview: string;
      rel_path: string;
      vault_path: string;
      tags: string[];
      modified_at: number;
      pinned: boolean;
      score: number;
    }
  | {
      source: "file";
      path: string;
      name: string;
      ext: string;
      is_dir: boolean;
      size: number;
      modified_at: number;
      score: number;
    }
  | {
      source: "app";
      name: string;
      exec_path: string;
      bundle_id: string | null;
      version: string | null;
      icon_path: string | null;
      score: number;
    }
  | {
      source: "web";
      query: string;
      provider: string;
      url: string;
      display_text: string;
      score: number;
    }
  | {
      source: "itsm";
      query: string;
      url: string;
      score: number;
    }
  | {
      source: "confl";
      query: string;
      url: string;
      score: number;
    }
  | {
      source: "jira";
      query: string;
      url: string;
      score: number;
    }
  | {
      source: "onenote";
      path: string;
      name: string;
      modified_at: number;
      score: number;
    };

export interface ClipboardItem {
  id: number;
  content: string;
  kind: string;
  byte_size: number;
  source_app: string | null;
  source_app_name: string | null;
  created_at: number;
  last_used_at: number;
  pinned: boolean;
  use_count: number;
  image_path: string | null;
  custom_title: string | null;
}

export interface Settings {
  obsidian_vault_path: string | null;
  hotkey: string;
  clipboard_history_limit: number;
  theme: string;
  clipboard_poll_ms: number;
  window_width: number;
  window_height: number;
  show_on_active_space: boolean;
  indexed_folders: string[];
  web_search_provider: string;
  enabled_sources: {
    clipboard: boolean;
    obsidian: boolean;
    files: boolean;
    web: boolean;
    itsm: boolean;
    onenote: boolean;
    confl: boolean;
    jira: boolean;
  };
  /** Compact mode — smaller row height + smaller icon size. */
  compact_mode: boolean;
}

export interface IndexerStatus {
  vault_path: string | null;
  indexed_count: number;
  last_scan_ms: number;
  scanning: boolean;
}

export interface FilesStatus {
  indexed_count: number;
  last_scan_ms: number;
  scanning: boolean;
}

export type IndexEvent =
  | { kind: "started"; total: number }
  | { kind: "progress"; done: number; total: number }
  | { kind: "finished"; indexed: number; took_ms: number }
  | { kind: "file_updated"; rel_path: string }
  | { kind: "file_removed"; rel_path: string }
  | { kind: "error"; message: string };

// Filter tab source identifier. Mirrors `activeFilter` in the Zustand store.
// Exported so FilterTabs.tsx can use it for the icon-map type.
export type FilterSource =
  | "all"
  | "favorites"
  | "clipboard"
  | "note"
  | "file"
  | "web"
  | "itsm"
  | "onenote"
  | "confl"
  | "jira";

// ── Search ───────────────────────────────────────────────────────────────────

export async function search(
  query: string,
  limit = 50
): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search_debounced", { query, limit });
}

// ── Clipboard ────────────────────────────────────────────────────────────────

export async function getClipboardHistory(
  limit = 100
): Promise<ClipboardItem[]> {
  return invoke<ClipboardItem[]>("get_clipboard_history", { limit });
}

export async function copyClipboardItem(id: number): Promise<void> {
  await invoke("copy_clipboard_item", { id });
}

export async function pinClipboardItem(
  id: number,
  pinned: boolean
): Promise<void> {
  await invoke("pin_clipboard_item", { id, pinned });
}

export async function deleteClipboardItem(id: number): Promise<void> {
  await invoke("delete_clipboard_item", { id });
}

/** Edit the text content of a clipboard item (for pinned/favorite items). */
export async function updateClipboardItem(id: number, content: string): Promise<void> {
  await invoke("update_clipboard_item", { id, content });
}

/** Update the custom title of a clipboard item (rename favorites). */
export async function updateClipboardTitle(
  id: number,
  customTitle: string | null
): Promise<void> {
  await invoke("update_clipboard_title", { id, customTitle });
}

export async function clearClipboardHistory(): Promise<number> {
  return invoke<number>("clear_clipboard_history");
}

// ── Obsidian ─────────────────────────────────────────────────────────────────

export async function setObsidianVault(path: string): Promise<IndexerStatus> {
  return invoke<IndexerStatus>("set_obsidian_vault", { path });
}

export async function getObsidianStatus(): Promise<IndexerStatus> {
  return invoke<IndexerStatus>("get_obsidian_status");
}

export async function reindexVault(): Promise<void> {
  await invoke("reindex_vault");
}

export async function openNote(id: number): Promise<void> {
  await invoke("open_note", { id });
}

export async function revealNote(id: number): Promise<void> {
  await invoke("reveal_note", { id });
}

/** Pin/unpin a note (favorites). Pinned notes always appear at the top. */
export async function pinNote(id: number, pinned: boolean): Promise<void> {
  await invoke("pin_note", { id, pinned });
}

// ── Favorites (for file/app/web results) ─────────────────────────────────────

export async function toggleFavorite(
  source: string,
  key: string,
  title: string,
  subtitle: string | null,
  customTitle: string | null = null
): Promise<boolean> {
  return invoke<boolean>("toggle_favorite", {
    source,
    key,
    title,
    subtitle,
    customTitle,
  });
}

export async function isFavorited(source: string, key: string): Promise<boolean> {
  return invoke<boolean>("is_favorited", { source, key });
}

/** Update the custom title of a favorite. */
export async function updateFavoriteTitle(
  source: string,
  key: string,
  newTitle: string
): Promise<void> {
  await invoke("update_favorite_title", { source, key, newTitle });
}

/** Get all favorites as SearchResult-like objects. */
export async function getFavorites(): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("get_favorites");
}

// ── Files ────────────────────────────────────────────────────────────────────

export async function openFile(path: string): Promise<void> {
  await invoke("open_file", { path });
}

/** Copy a single file to the clipboard (as CF_HDROP, like Ctrl+C in Explorer). */
export async function copyFileToClipboard(path: string): Promise<void> {
  await invoke("copy_file_to_clipboard", { path });
}

export async function revealFile(path: string): Promise<void> {
  await invoke("reveal_file", { path });
}

export async function reindexFiles(): Promise<void> {
  await invoke("reindex_files");
}

export async function getFilesStatus(): Promise<FilesStatus> {
  return invoke<FilesStatus>("get_files_status");
}

export async function setIndexedFolders(folders: string[]): Promise<void> {
  await invoke("set_indexed_folders", { folders });
}

// ── Apps ─────────────────────────────────────────────────────────────────────

export async function openApp(execPath: string): Promise<void> {
  await invoke("open_app", { execPath });
}

export async function revealApp(execPath: string): Promise<void> {
  await invoke("reveal_app", { execPath });
}

export async function getAppsStatus(): Promise<{ count: number }> {
  return invoke<{ count: number }>("get_apps_status");
}

// ── Web ──────────────────────────────────────────────────────────────────────

export async function openWebUrl(url: string): Promise<void> {
  await invoke("open_web_url", { url });
}

/** Open a OneNote .one file in OneNote via onenote:// URI. */
export async function openOnenote(path: string): Promise<void> {
  await invoke("open_onenote", { path });
}

/** Copy a web URL to the clipboard as text. */
export async function copyWebUrl(url: string): Promise<void> {
  // We reuse the clipboard-manager plugin's write_text to copy the URL.
  const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
  await writeText(url);
}

/** Open a directory picker and return the selected path (or null). */
export async function pickVaultDirectory(): Promise<string | null> {
  const selected = await openDialog({
    directory: true,
    multiple: false,
    title: "Select Obsidian Vault Directory",
  });
  if (Array.isArray(selected)) {
    return selected[0] ?? null;
  }
  return selected;
}

// ── Settings / app ───────────────────────────────────────────────────────────

export async function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export async function updateSettings(patch: Partial<Settings>): Promise<Settings> {
  return invoke<Settings>("update_settings", { patch });
}

export function toggleLauncher(): void {
  invoke("toggle_launcher").catch(console.error);
}

export function hideLauncher(): void {
  invoke("hide_launcher").catch(console.error);
}

export function getAppVersion(): Promise<string> {
  return invoke<string>("get_app_version");
}

// ── Autostart ───────────────────────────────────────────────────────────────

/** Check whether the app is registered to start at login (Windows registry /
 *  macOS LaunchAgent). State is owned by the OS, not by settings.json. */
export async function isAutostartEnabled(): Promise<boolean> {
  return invoke<boolean>("is_autostart_enabled");
}

/** Register the app to start at login. */
export async function enableAutostart(): Promise<void> {
  await invoke("enable_autostart");
}

/** Unregister the app from starting at login. */
export async function disableAutostart(): Promise<void> {
  await invoke("disable_autostart");
}

// ── Events ───────────────────────────────────────────────────────────────────

/** Subscribe to Obsidian indexer events. Returns an unsubscribe function. */
export function onIndexEvent(
  cb: (event: IndexEvent) => void
): Promise<UnlistenFn> {
  return listen<IndexEvent>("obsidian://event", (e) => cb(e.payload));
}

/** Subscribe to the launcher-shown event (fired every time the window opens). */
export function onLauncherShown(cb: () => void): Promise<UnlistenFn> {
  return listen("launcher://shown", () => cb());
}
