// PreviewPane — premium right panel (radius 20, padding 32).
// Fixed layout: header (icon + title + buttons), preview body, information block.

import { useState, useEffect, useRef } from "react";
import { useStore } from "@/store/useStore";
import { formatRelativeTime, formatBytes, truncate } from "@/lib/utils";
import { t } from "@/i18n";
import * as tauri from "@/lib/tauri";
import { convertFileSrc } from "@tauri-apps/api/core";
import { getVisibleResults } from "@/lib/visibleResults";

// ── PNG icons from the provided archive ──
import iconClipboard from "@/assets/icons/clipboard.png";
import iconObsidian from "@/assets/icons/obsidian.png";
import iconFiles from "@/assets/icons/files.png";
import iconApplications from "@/assets/icons/applications.png";
import iconInternet from "@/assets/icons/internet.png";
import iconItsm from "@/assets/icons/itsm.png";
import iconOnenote from "@/assets/icons/onenote.png";
import iconPdf from "@/assets/icons/pdf.png";
import iconWord from "@/assets/icons/word.png";
import iconExcel from "@/assets/icons/excel.png";
import iconPowerpoint from "@/assets/icons/powerpoint.png";
import iconImages from "@/assets/icons/images.png";
import iconAudio from "@/assets/icons/audio.png";
import iconVideo from "@/assets/icons/video.png";
import iconOther from "@/assets/icons/other.png";
import iconLinks from "@/assets/icons/links.png";
import iconMail from "@/assets/icons/mail.png";
import iconArchive from "@/assets/icons/archive.png";
import iconText from "@/assets/icons/text.png";
import iconDatabase from "@/assets/icons/database.png";
import iconCalendar from "@/assets/icons/calendar.png";
import iconConfl from "@/assets/icons/confl.png";
import iconJira from "@/assets/icons/jira.png";

// ── Helpers shared with ResultRow for clipboard icon detection ──
const URL_RE = /^(https?|ftp|file|mailto):\/\/[^\s]+$/i;
const WWW_RE = /^www\.[^\s]+\.[^\s]+$/i;

function isUrlContent(s: string): boolean {
  const trimmed = s.trim();
  if (trimmed.length === 0 || trimmed.length > 2048) return false;
  return URL_RE.test(trimmed) || WWW_RE.test(trimmed);
}

function extOfPath(p: string): string {
  const cleaned = p.trim();
  const dotIdx = cleaned.lastIndexOf(".");
  if (dotIdx < 0) return "";
  const slashIdx = Math.max(cleaned.lastIndexOf("/"), cleaned.lastIndexOf("\\"));
  if (slashIdx > dotIdx) return "";
  return cleaned.slice(dotIdx + 1).toLowerCase();
}

export function PreviewPane() {
  const results = useStore((s) => s.results);
  const selectedIndex = useStore((s) => s.selectedIndex);
  const activeFilter = useStore((s) => s.activeFilter);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const setQuery = useStore((s) => s.setQuery);
  const query = useStore((s) => s.query);
  const injectedKeys = useStore((s) => s.injectedKeys);
  const duplicateFilePaths = useStore((s) => s.duplicateFilePaths);

  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [editingTitle, setEditingTitle] = useState(false);
  const [titleDraft, setTitleDraft] = useState("");
  const [fullContent, setFullContent] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const titleInputRef = useRef<HTMLInputElement>(null);

  // CRITICAL: use the SAME `getVisibleResults` selector as ResultList and
  // useKeyboardNav. Previously PreviewPane had its own filter logic that
  // didn't account for `injectedKeys` — so the row visible at index N in
  // the list didn't match the row at index N used by the preview pane,
  // causing the preview to show the WRONG image (a different row's
  // image_path). Now all three consumers share the same filter, so the
  // visible row N is always the same as the previewed row N.
  const selected = (() => {
    const filtered = getVisibleResults({
      results,
      activeFilter,
      injectedKeys,
      favoriteKeys,
      query,
      duplicateFilePaths,
    });
    return filtered[selectedIndex];
  })();

  useEffect(() => {
    setEditing(false);
    setDraft("");
    setEditingTitle(false);
    setTitleDraft("");
    setFullContent(null);
  }, [selectedIndex, selected?.source]);

  // Load full content from DB for clipboard TEXT items (the preview field
  // only contains a truncated snippet). For files/image kinds we don't need
  // to load the full content — the icon + image_path already give the user
  // what they need.
  useEffect(() => {
    if (selected?.source === "clipboard" && selected.kind === "text" && !editing) {
      tauri.getClipboardHistory(500).then((items) => {
        const item = items.find((it) => it.id === (selected as any).id);
        if (item) {
          setFullContent(item.content);
        }
      }).catch(() => setFullContent(null));
    } else {
      setFullContent(null);
    }
  }, [selected, editing]);

  useEffect(() => {
    if (editing && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.select();
    }
  }, [editing]);

  useEffect(() => {
    if (editingTitle && titleInputRef.current) {
      titleInputRef.current.focus();
      titleInputRef.current.select();
    }
  }, [editingTitle]);

  useEffect(() => {
    if (editing && selected?.source === "clipboard" && selected.kind === "text" && draft === "") {
      tauri.getClipboardHistory(500).then((items) => {
        const item = items.find((it) => it.id === (selected as any).id);
        if (item) setDraft(item.content);
      });
    }
  }, [editing, selected, draft]);

  if (!selected) {
    return (
      <div className="card-panel flex-1 flex items-center justify-center" style={{ padding: "32px" }}>
        <span style={{ fontSize: "var(--fs-15)", color: "var(--text-muted)" }}>
          {t("search.placeholder")}
        </span>
      </div>
    );
  }

  const startEdit = () => setEditing(true);
  const cancelEdit = () => { setEditing(false); setDraft(""); };
  const saveEdit = async () => {
    if (selected.source === "clipboard") {
      await tauri.updateClipboardItem(selected.id, draft);
      setEditing(false);
      setDraft("");
      setQuery(query);
    }
  };

  const startTitleEdit = () => {
    setTitleDraft(titleOf(selected));
    setEditingTitle(true);
  };

  const saveTitleEdit = async () => {
    if (!editingTitle) return;
    // Capture values before state updates (prevents double-fire issues)
    const newTitle = titleDraft.trim();
    setEditingTitle(false);
    setTitleDraft("");

    if (!newTitle) return;

    try {
      if (selected.source === "clipboard") {
        // Clipboard items: update custom_title in clipboard table.
        await tauri.updateClipboardTitle(selected.id, newTitle);
      } else if (selected.source !== "note") {
        // File/app/web/itsm: update title in favorites table.
        const key = favKeyOf(selected);
        await tauri.updateFavoriteTitle(selected.source, key, newTitle);
      }
      // Force a fresh search to reload with the new title.
      setTimeout(() => {
        useStore.getState().setQuery(useStore.getState().query);
      }, 50);
    } catch (e) {
      console.error("Failed to update title:", e);
    }
  };

  const isClipboard = selected.source === "clipboard";
  const isFav = isFavCheck(selected, favoriteKeys);

  return (
    <div
      className="preview-selectable card-panel"
      style={{
        padding: "28px",
        gap: "20px",
        overflowY: "auto",
        overflowX: "hidden",
        flex: "1 1 auto",
        minHeight: "0",
        display: "flex",
        flexDirection: "column",
      }}
    >
      {/* ══ Header: icon + title ══ */}
      <div style={{ display: "flex", alignItems: "center", gap: "16px", flexShrink: 0 }}>
        {/* Icon */}
        <div className="icon-container" style={{ ...iconBgStyle(selected), flexShrink: 0, width: "56px", height: "56px" }}>
          {isClipboard && selected.image_path ? (
            <img
              src={convertFileSrc(selected.image_path)}
              alt="clip"
              draggable={false}
              className="icon-preview-img"
              onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = "none"; }}
            />
          ) : selected.source === "file" && !selected.is_dir && isImageExt(selected.ext) ? (
            <img
              src={convertFileSrc(selected.path)}
              alt={selected.name}
              draggable={false}
              className="icon-preview-img"
              onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = "none"; }}
            />
          ) : (
            <PreviewTypeIcon result={selected} />
          )}
        </div>

        {/* Title + subtitle */}
        <div style={{ flex: "1 1 auto", minWidth: "0" }}>
          {editingTitle ? (
            <input
              ref={titleInputRef}
              type="text"
              value={titleDraft}
              onChange={(e) => setTitleDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  e.stopPropagation();
                  saveTitleEdit();
                } else if (e.key === "Escape") {
                  e.preventDefault();
                  e.stopPropagation();
                  setEditingTitle(false);
                  setTitleDraft("");
                }
              }}
              onBlur={() => {
                // Save on blur — but only if still editing (Enter already saved
                // and set editingTitle=false, so this won't double-fire).
                if (editingTitle) saveTitleEdit();
              }}
              style={{
                width: "100%",
                fontSize: "var(--fs-22)",
                fontWeight: 700,
                color: "var(--text-primary)",
                background: "var(--bg-icon)",
                border: "1px solid var(--accent)",
                borderRadius: "var(--r-button)",
                padding: "4px 8px",
                outline: "none",
              }}
            />
          ) : (
            <h2
              style={{
                fontSize: "var(--fs-22)",
                fontWeight: 700,
                color: "var(--text-primary)",
                lineHeight: 1.25,
                margin: 0,
                wordBreak: "break-word",
                overflow: "hidden",
                textOverflow: "ellipsis",
                display: "-webkit-box",
                WebkitLineClamp: 2,
                WebkitBoxOrient: "vertical",
              }}
            >
              {truncate(titleOf(selected), 100)}
            </h2>
          )}
          <div style={{ fontSize: "var(--fs-13)", color: "var(--text-muted)", marginTop: "4px" }}>
            {sourceLabel(selected)}
          </div>
        </div>
      </div>

      {/* ══ Action buttons row — under the title, left-aligned ══
          Order: Rename → Edit text → Copy → Open → Delete → Favorite ★ */}
      <div style={{ display: "flex", alignItems: "center", gap: "6px", flexShrink: 0 }}>
        {/* Rename title — for ALL favorited items (clipboard pinned + file/app/web/itsm).
            FIRST in the row. */}
        {isFav && !editingTitle && (
          <button className="action-btn" onClick={startTitleEdit} title="Переименовать">
            <svg style={{ width: "18px", height: "18px" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M20.59 13.41l-7.17 7.17a2 2 0 0 1-2.83 0L2 12V2h10l8.59 8.59a2 2 0 0 1 0 2.82z" />
              <line x1="7" y1="7" x2="7.01" y2="7" />
            </svg>
          </button>
        )}
        {/* Edit text — clipboard TEXT items only. SECOND in the row. */}
        {isClipboard && !editing && selected.kind === "text" && (
          <button className="action-btn" onClick={startEdit} title="Редактировать текст">
            <svg style={{ width: "18px", height: "18px" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
              <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
            </svg>
          </button>
        )}
        <button className="action-btn" onClick={() => copyResult(selected)} title="Копировать">
          <svg style={{ width: "18px", height: "18px" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </svg>
        </button>
        <button className="action-btn" onClick={() => openResult(selected)} title="Открыть">
          <svg style={{ width: "18px", height: "18px" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </button>
        {/* Delete from clipboard — only for clipboard items. Same color as
            the other action buttons (no red highlighting). Placed right
            before the favorite star. */}
        {isClipboard && (
          <button
            className="action-btn"
            onClick={async () => {
              try {
                await tauri.deleteClipboardItem(selected.id);
                useStore.getState().setQuery(useStore.getState().query);
              } catch (e) {
                console.error("Failed to delete clipboard item:", e);
              }
            }}
            title="Удалить"
          >
            <svg style={{ width: "18px", height: "18px" }} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <polyline points="3 6 5 6 21 6" />
              <path d="M19 6l-1.5 14a2 2 0 0 1-2 2H8.5a2 2 0 0 1-2-2L5 6" />
              <path d="M10 11v6M14 11v6" />
              <path d="M9 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
            </svg>
          </button>
        )}
        <button
          className={`action-btn ${isFav ? "fav" : ""}`}
          onClick={() => toggleFavoriteResult(selected)}
          title={isFav ? "Открепить" : "В избранное"}
        >
          <svg
            style={{ width: "18px", height: "18px" }}
            viewBox="0 0 24 24"
            fill={isFav ? "currentColor" : "none"}
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
          </svg>
        </button>
      </div>

      {/* ══ Body: editor / preview / image ══ */}
      {isClipboard && editing ? (
        <div style={{ display: "flex", flexDirection: "column", gap: "8px", flex: "1 1 auto", minHeight: "200px" }}>
          <textarea
            ref={textareaRef}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            style={{
              flex: "1 1 auto",
              minHeight: "200px",
              padding: "16px",
              borderRadius: "var(--r-button)",
              fontSize: "var(--fs-14)",
              fontFamily: "var(--font-sans)",
              background: "var(--bg-icon)",
              color: "var(--text-primary)",
              border: "1px solid var(--accent)",
              outline: "none",
              resize: "none",
            }}
            spellCheck={false}
          />
          <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end" }}>
            <button
              style={{
                padding: "8px 16px",
                height: "36px",
                borderRadius: "var(--r-button)",
                background: "var(--bg-icon)",
                color: "var(--text-secondary)",
                fontSize: "var(--fs-13)",
                border: "none",
                cursor: "default",
              }}
              onClick={cancelEdit}
            >
              Отмена
            </button>
            <button
              onClick={saveEdit}
              style={{
                padding: "8px 16px",
                height: "36px",
                borderRadius: "var(--r-button)",
                background: "var(--accent)",
                color: "#FFF",
                fontSize: "var(--fs-13)",
                fontWeight: 600,
                border: "none",
                cursor: "default",
              }}
            >
              Сохранить
            </button>
          </div>
        </div>
      ) : isClipboard && selected.image_path ? (
        // Image preview — covers both `kind = "image"` (screenshots) and
        // `kind = "files"` when the copied file is an image.
        <div style={{ display: "flex", justifyContent: "center", alignItems: "center", flex: "1 1 auto", minHeight: "120px", maxHeight: "350px", overflow: "hidden" }}>
          <img
            src={convertFileSrc(selected.image_path)}
            alt="clip"
            style={{ maxWidth: "100%", maxHeight: "320px", objectFit: "contain", borderRadius: "var(--r-button)" }}
            draggable={false}
          />
        </div>
      ) : selected.source === "file" && !selected.is_dir && isImageExt(selected.ext) ? (
        <div style={{ display: "flex", justifyContent: "center", alignItems: "center", flex: "1 1 auto", minHeight: "120px", maxHeight: "350px", overflow: "hidden" }}>
          <img
            src={convertFileSrc(selected.path)}
            alt={selected.name}
            style={{ maxWidth: "100%", maxHeight: "320px", objectFit: "contain", borderRadius: "var(--r-button)" }}
            draggable={false}
          />
        </div>
      ) : (
        <div
          style={{
            flex: "1 1 auto",
            minHeight: "120px",
            fontSize: "var(--fs-15)",
            color: "var(--text-secondary)",
            lineHeight: 1.7,
            whiteSpace: "pre-wrap",
            wordBreak: "break-word",
            overflow: "hidden",
          }}
        >
          {fullContent || bodyOf(selected) || <span style={{ color: "var(--text-muted)", fontStyle: "italic" }}>Нет превью</span>}
        </div>
      )}

      {/* ══ Tags (notes only) ══ */}
      {selected.source === "note" && selected.tags.length > 0 && (
        <div style={{ display: "flex", flexWrap: "wrap", gap: "6px", flexShrink: 0 }}>
          {selected.tags.map((tag) => (
            <span
              key={tag}
              style={{
                fontSize: "var(--fs-13)",
                padding: "3px 10px",
                borderRadius: "var(--r-pill)",
                background: "var(--bg-icon)",
                color: "var(--text-secondary)",
              }}
            >
              #{tag}
            </span>
          ))}
        </div>
      )}

      {/* ══ Information block ══ */}
      <div style={{ flexShrink: 0, paddingTop: "20px", borderTop: "1px solid var(--border)" }}>
        <div
          style={{
            fontSize: "var(--fs-13)",
            fontWeight: 700,
            textTransform: "uppercase",
            letterSpacing: "0.08em",
            color: "var(--text-muted)",
            marginBottom: "16px",
          }}
        >
          Information
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: "14px" }}>
          {infoRows(selected).map(([key, value]) => (
            <div key={key} style={{ display: "flex", gap: "16px", alignItems: "baseline" }}>
              <span
                style={{
                  width: "120px",
                  flexShrink: 0,
                  fontSize: "var(--fs-15)",
                  fontWeight: 500,
                  color: "var(--text-muted)",
                }}
              >
                {key}
              </span>
              <span
                style={{
                  flex: "1 1 auto",
                  minWidth: "0",
                  fontSize: "var(--fs-15)",
                  fontWeight: 400,
                  color: "var(--text-secondary)",
                  wordBreak: "break-all",
                }}
              >
                {value}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

// ── Helpers ──────────────────────────────────────────────────────────────────

function titleOf(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "clipboard":
      // Use custom_title if set, otherwise the first line of content.
      return truncate(r.custom_title || r.title, 120);
    case "note":
      return truncate(r.title, 120);
    case "file":
    case "app":
      return r.name;
    case "web":
      return r.display_text;
    case "itsm":
      return r.query;
    case "onenote":
      return r.name;
    case "confl":
      return r.query;
    case "jira":
      return r.query;
  }
}

function bodyOf(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "clipboard":
      return r.preview || r.title;
    case "note":
      return r.preview || "";
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
  }
}

function sourceLabel(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "clipboard": return "Буфер обмена";
    case "note": return "Obsidian";
    case "file": return r.is_dir ? "Папка" : "Файл";
    case "app": return "Приложение";
    case "web": return "Интернет";
    case "itsm": return "ITSM (Naumen)";
    case "onenote": return "OneNote";
    case "confl": return "Sberworks Confluence";
    case "jira": return "Sberworks Jira";
  }
}

function infoRows(r: import("@/lib/tauri").SearchResult): [string, string][] {
  switch (r.source) {
    case "clipboard": {
      const isImage = !!r.image_path;
      const rows: [string, string][] = [
        ["Источник", r.source_app_name ?? "—"],
        ["Тип", isImage ? "Изображение" : "Текст"],
      ];
      if (!isImage) rows.push(["Символов", String(r.title.length + (r.preview?.length ?? 0))]);
      rows.push(["Скопировано", formatRelativeTime(r.last_used_at)]);
      return rows;
    }
    case "note": {
      const rows: [string, string][] = [
        ["Источник", "Obsidian"],
        ["Тип", r.rel_path.endsWith(".canvas") ? "Canvas" : "Markdown"],
        ["Путь", r.rel_path],
        ["Хранилище", r.vault_path.split(/[\\/]/).pop() ?? r.vault_path],
        ["Изменено", formatRelativeTime(r.modified_at)],
      ];
      if (r.tags.length > 0) rows.push(["Теги", r.tags.map((t) => `#${t}`).join(" ")]);
      return rows;
    }
    case "file": {
      return [
        ["Источник", "Файловая система"],
        ["Тип", r.is_dir ? "Папка" : (r.ext.toUpperCase() || "Файл")],
        ["Размер", r.is_dir ? "—" : formatBytes(r.size)],
        ["Путь", r.path],
        ["Изменено", formatRelativeTime(r.modified_at)],
      ];
    }
    case "app": {
      const rows: [string, string][] = [
        ["Источник", "Приложение"],
        ["Тип", "Исполняемый файл"],
        ["Путь", r.exec_path],
      ];
      if (r.version) rows.push(["Версия", r.version]);
      return rows;
    }
    case "web": {
      return [
        ["Источник", "Интернет"],
        ["Поисковик", r.provider.charAt(0).toUpperCase() + r.provider.slice(1)],
        ["URL", r.url],
      ];
    }
    case "itsm": {
      return [
        ["Источник", "ITSM"],
        ["Система", "Naumen"],
        ["URL", r.url],
      ];
    }
    case "confl": {
      return [
        ["Источник", "Sberworks"],
        ["Система", "Confluence"],
        ["URL", r.url],
      ];
    }
    case "jira": {
      return [
        ["Источник", "Sberworks"],
        ["Система", "Jira"],
        ["URL", r.url],
      ];
    }
    case "onenote": {
      return [
        ["Источник", "OneNote"],
        ["Тип", "OneNote файл"],
        ["Путь", r.path],
        ["Изменено", formatRelativeTime(r.modified_at)],
      ];
    }
  }
}

function isImageExt(ext: string): boolean {
  return ["jpg","jpeg","png","gif","webp","svg","bmp","ico","tiff"].includes(ext.toLowerCase());
}

function favKeyOf(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "file": return r.path;
    case "app": return r.exec_path;
    case "web": return r.url;
    case "itsm": return r.url;
    case "onenote": return r.path;
    case "confl": return r.url;
    case "jira": return r.url;
    default: return "";
  }
}

function isFavCheck(r: import("@/lib/tauri").SearchResult | undefined, favKeys: Set<string>): boolean {
  if (!r) return false;
  if ("pinned" in r && r.pinned) return true;
  if (r.source === "file" || r.source === "app" || r.source === "web" || r.source === "itsm" || r.source === "confl" || r.source === "jira") {
    return favKeys.has(`${r.source}:${favKeyOf(r)}`);
  }
  return false;
}

function iconBgStyle(_r: import("@/lib/tauri").SearchResult): React.CSSProperties {
  // Transparent background — no "box" around icons (user requirement).
  return { background: "transparent" };
}

function PreviewTypeIcon({ result }: { result: import("@/lib/tauri").SearchResult }) {
  // Use PNG icons — CSS .icon-container img handles sizing.
  if (result.source === "file") {
    if (result.is_dir) {
      return <img src={iconFiles} alt="" draggable={false} />;
    }
    const e = result.ext.toLowerCase();
    if (e === "pdf") return <img src={iconPdf} alt="" draggable={false} />;
    if (["doc", "docx"].includes(e)) return <img src={iconWord} alt="" draggable={false} />;
    if (["xls", "xlsx", "csv"].includes(e)) return <img src={iconExcel} alt="" draggable={false} />;
    if (["ppt", "pptx"].includes(e)) return <img src={iconPowerpoint} alt="" draggable={false} />;
    if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"].includes(e)) return <img src={iconImages} alt="" draggable={false} />;
    if (["mp3", "wav", "flac", "ogg", "aac", "m4a"].includes(e)) return <img src={iconAudio} alt="" draggable={false} />;
    if (["mp4", "avi", "mov", "mkv", "webm"].includes(e)) return <img src={iconVideo} alt="" draggable={false} />;
    if (["eml", "emlx"].includes(e)) return <img src={iconMail} alt="" draggable={false} />;
    if (["zip", "rar", "7z", "tar", "gz"].includes(e)) return <img src={iconArchive} alt="" draggable={false} />;
    if (["txt", "md", "log"].includes(e)) return <img src={iconText} alt="" draggable={false} />;
    if (["db", "sqlite", "sql"].includes(e)) return <img src={iconDatabase} alt="" draggable={false} />;
    if (["ics", "ical"].includes(e)) return <img src={iconCalendar} alt="" draggable={false} />;
    if (e === "url") return <img src={iconLinks} alt="" draggable={false} />;
    return <img src={iconOther} alt="" draggable={false} />;
  }

  // Clipboard items: pick icon by kind + content.
  //   • kind = "files"  → file-type icon by extension (Word/Excel/PDF/...)
  //   • kind = "text"   → if content is URL → Link icon, else Clipboard icon
  //   • kind = "image"  → handled by <img> preview before reaching here
  if (result.source === "clipboard") {
    if (result.kind === "files") {
      const firstPath = result.content.split("\n")[0] ?? "";
      const ext = extOfPath(firstPath);
      if (!ext) {
        return <img src={iconFiles} alt="" draggable={false} />;
      }
      if (ext === "pdf") return <img src={iconPdf} alt="" draggable={false} />;
      if (["doc", "docx"].includes(ext)) return <img src={iconWord} alt="" draggable={false} />;
      if (["xls", "xlsx", "csv"].includes(ext)) return <img src={iconExcel} alt="" draggable={false} />;
      if (["ppt", "pptx"].includes(ext)) return <img src={iconPowerpoint} alt="" draggable={false} />;
      if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"].includes(ext)) return <img src={iconImages} alt="" draggable={false} />;
      if (["mp3", "wav", "flac", "ogg", "aac", "m4a"].includes(ext)) return <img src={iconAudio} alt="" draggable={false} />;
      if (["mp4", "avi", "mov", "mkv", "webm"].includes(ext)) return <img src={iconVideo} alt="" draggable={false} />;
      if (["eml", "emlx"].includes(ext)) return <img src={iconMail} alt="" draggable={false} />;
      if (["zip", "rar", "7z", "tar", "gz"].includes(ext)) return <img src={iconArchive} alt="" draggable={false} />;
      if (["txt", "md", "log"].includes(ext)) return <img src={iconText} alt="" draggable={false} />;
      if (["db", "sqlite", "sql"].includes(ext)) return <img src={iconDatabase} alt="" draggable={false} />;
      if (["ics", "ical"].includes(ext)) return <img src={iconCalendar} alt="" draggable={false} />;
      if (ext === "url") return <img src={iconLinks} alt="" draggable={false} />;
      return <img src={iconOther} alt="" draggable={false} />;
    }
    if (result.kind === "text" && isUrlContent(result.content)) {
      return <img src={iconLinks} alt="" draggable={false} />;
    }
    return <img src={iconClipboard} alt="" draggable={false} />;
  }

  const iconMap: Record<string, string> = {
    clipboard: iconClipboard,
    note: iconObsidian,
    app: iconApplications,
    web: iconInternet,
    itsm: iconItsm,
    onenote: iconOnenote,
    confl: iconConfl,
    jira: iconJira,
  };

  const src = iconMap[result.source] ?? iconOther;
  return <img src={src} alt="" draggable={false} />;
}

// ── Action helpers ───────────────────────────────────────────────────────────

async function copyResult(r: import("@/lib/tauri").SearchResult) {
  switch (r.source) {
    case "clipboard":
      await tauri.copyClipboardItem(r.id);
      break;
    case "note":
      await tauri.openNote(r.id);
      break;
    case "file":
      // Copy the file itself to clipboard (CF_HDROP), not the path text.
      await tauri.copyFileToClipboard(r.path);
      break;
    case "app":
      await tauri.revealApp(r.exec_path);
      break;
    case "web":
      await tauri.copyWebUrl(r.url);
      break;
    case "itsm":
    case "confl":
    case "jira":
      await tauri.copyWebUrl(r.url);
      break;
    case "onenote":
      await tauri.copyWebUrl(r.path);
      break;
  }
}

async function openResult(r: import("@/lib/tauri").SearchResult) {
  switch (r.source) {
    case "clipboard":
      await tauri.copyClipboardItem(r.id);
      tauri.hideLauncher();
      break;
    case "note":
      await tauri.openNote(r.id);
      tauri.hideLauncher();
      break;
    case "file":
      await tauri.openFile(r.path);
      tauri.hideLauncher();
      break;
    case "app":
      await tauri.openApp(r.exec_path);
      tauri.hideLauncher();
      break;
    case "web":
      await tauri.openWebUrl(r.url);
      tauri.hideLauncher();
      break;
    case "itsm":
    case "confl":
    case "jira":
      await tauri.openWebUrl(r.url);
      tauri.hideLauncher();
      break;
    case "onenote":
      await tauri.openOnenote(r.path);
      tauri.hideLauncher();
      break;
  }
}

async function toggleFavoriteResult(r: import("@/lib/tauri").SearchResult) {
  if (r.source === "clipboard") {
    await tauri.pinClipboardItem(r.id, !r.pinned);
  } else if (r.source === "note") {
    await tauri.pinNote(r.id, !r.pinned);
  } else {
    const key = favKeyOf(r);
    let defaultTitle = "";
    let subtitle: string | null = null;
    switch (r.source) {
      case "file":
      case "app":
      case "onenote":
        defaultTitle = r.name;
        break;
      case "web":
        defaultTitle = r.display_text;
        subtitle = r.provider;
        break;
      case "itsm":
        defaultTitle = r.query;
        subtitle = "Naumen ITSM";
        break;
      case "confl":
        defaultTitle = r.query;
        subtitle = "Sberworks Confluence";
        break;
      case "jira":
        defaultTitle = r.query;
        subtitle = "Sberworks Jira";
        break;
    }
    if (r.source === "file") subtitle = r.path;
    else if (r.source === "app") subtitle = r.exec_path;
    else if (r.source === "onenote") subtitle = r.path;
    await tauri.toggleFavorite(r.source, key, defaultTitle, subtitle, null);
  }
  useStore.getState().setQuery(useStore.getState().query);
}
