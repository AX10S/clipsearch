// ResultRow — premium result card (96px height, 16px radius).
// Left: 60×60 colored icon container. Middle: title + path/description.
// Right: badge + time. Selected: 4px accent bar on the left.
//
// Colored icons by type — no text badges needed in most cases.

import { memo } from "react";
import { useStore } from "@/store/useStore";
import { formatRelativeTime, truncate } from "@/lib/utils";
import type { SearchResult } from "@/lib/tauri";
import { convertFileSrc } from "@tauri-apps/api/core";

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

// URL detection — used to pick the Link icon when the clipboard holds a URL.
// Conservative: only matches strings that ARE a URL (no extra text around it).
const URL_RE = /^(https?|ftp|file|mailto):\/\/[^\s]+$/i;
const WWW_RE = /^www\.[^\s]+\.[^\s]+$/i;

function isUrlContent(s: string): boolean {
  const trimmed = s.trim();
  if (trimmed.length === 0 || trimmed.length > 2048) return false;
  return URL_RE.test(trimmed) || WWW_RE.test(trimmed);
}

/// Extract the file extension (lowercase, without dot) from a path string.
function extOfPath(p: string): string {
  const cleaned = p.trim();
  const dotIdx = cleaned.lastIndexOf(".");
  if (dotIdx < 0) return "";
  // Stop at directory separator if any (paths may be Windows-style).
  const slashIdx = Math.max(cleaned.lastIndexOf("/"), cleaned.lastIndexOf("\\"));
  if (slashIdx > dotIdx) return "";
  return cleaned.slice(dotIdx + 1).toLowerCase();
}

interface Props {
  result: SearchResult;
  index: number;
}

function RowInner({ result, index }: Props) {
  const selectedIndex = useStore((s) => s.selectedIndex);
  const setSelection = useStore((s) => s.setSelection);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const selected = index === selectedIndex;

  const isFav = (() => {
    if ("pinned" in result && result.pinned) return true;
    if (result.source === "file" || result.source === "app" || result.source === "web") {
      const key = favKeyOf(result);
      return favoriteKeys.has(`${result.source}:${key}`);
    }
    return false;
  })();

  const onClick = () => setSelection(index);
  const onDoubleClick = async () => { await activate(result); };

  return (
    <div
      role="option"
      aria-selected={selected}
      data-selected={selected}
      data-favorite={isFav}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onMouseDown={(e) => e.preventDefault()}
      className={`result-row ${selected ? "selected" : ""} ${isFav ? "favorite-row" : ""}`}
    >
      {/* Icon container — 60×60, colored by type */}
      <div className="icon-container" style={iconBgStyle(result)}>
        {result.source === "clipboard" && result.image_path ? (
          <img
            src={convertFileSrc(result.image_path)}
            alt="clip"
            draggable={false}
            className="icon-preview-img"
            onError={(e) => {
              // If the image fails to load (e.g. file deleted, path invalid),
              // hide the broken img so the container's background shows.
              (e.currentTarget as HTMLImageElement).style.display = "none";
            }}
          />
        ) : result.source === "file" && !result.is_dir && isImageExt(result.ext) ? (
          <img
            src={convertFileSrc(result.path)}
            alt={result.name}
            draggable={false}
            className="icon-preview-img"
            onError={(e) => {
              (e.currentTarget as HTMLImageElement).style.display = "none";
            }}
          />
        ) : (
          <TypeIcon result={result} />
        )}
      </div>

      {/* Title + path/description */}
      <div className="flex-1 min-w-0">
        <div
          className="truncate"
          style={{ fontSize: "var(--fs-22)", fontWeight: 600, color: "var(--text-primary)", lineHeight: 1.3 }}
        >
          {truncate(titleOf(result), 70) || <span style={{ opacity: 0.4, fontStyle: "italic" }}>(empty)</span>}
        </div>
        {subtitleOf(result) && (
          <div
            className="truncate mt-0.5"
            style={{ fontSize: "var(--fs-15)", color: "var(--text-muted)", lineHeight: 1.4 }}
          >
            {truncate(subtitleOf(result), 90)}
          </div>
        )}
      </div>

      {/* Right: badge + time + favorite star */}
      <div className="shrink-0 flex flex-col items-end gap-1.5 ml-2">
        <SourceBadgePill result={result} />
        <div className="flex items-center gap-1.5">
          {isFav && (
            <svg style={{ width: "14px", height: "14px", color: "#F5A623" }} viewBox="0 0 24 24" fill="currentColor">
              <polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2" />
            </svg>
          )}
          {metaOf(result) && (
            <span className="tnum" style={{ fontSize: "var(--fs-13)", color: "var(--text-muted)" }}>
              {metaOf(result)}
            </span>
          )}
        </div>
      </div>
    </div>
  );
}

/// Background for the icon container — transparent (no "box" around icons).
/// The user asked to remove the frame/box and just show icons directly.
function iconBgStyle(_r: SearchResult): React.CSSProperties {
  return { background: "transparent" };
}

function titleOf(r: SearchResult): string {
  switch (r.source) {
    case "clipboard":
      // Use custom_title if set (for renamed favorites).
      return r.custom_title || r.title;
    case "note":
      return r.title;
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

function subtitleOf(r: SearchResult): string {
  switch (r.source) {
    case "clipboard":
      return r.preview || "";
    case "note":
      return r.rel_path;
    case "file":
      return r.path;
    case "app":
      return r.exec_path;
    case "web":
      return "";
    case "itsm":
      return "";
    case "onenote":
      return r.path;
    case "confl":
      return "";
    case "jira":
      return "";
  }
}

function metaOf(r: SearchResult): string | null {
  switch (r.source) {
    case "clipboard":
      return formatRelativeTime(r.last_used_at);
    case "note":
    case "file":
      return formatRelativeTime(r.modified_at);
    case "app":
      return null;
    case "web":
      return r.provider.toUpperCase();
    case "itsm":
      return "NAUMEN";
    case "onenote":
      return formatRelativeTime(r.modified_at);
    case "confl":
      return "CONFLUENCE";
    case "jira":
      return "JIRA";
  }
}

function isImageExt(ext: string): boolean {
  return ["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"].includes(ext.toLowerCase());
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
    default: return "";
  }
}

async function activate(result: SearchResult) {
  const tauri = await import("@/lib/tauri");
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
      await tauri.openFile(result.path);
      tauri.hideLauncher();
      break;
    case "app":
      await tauri.openApp(result.exec_path);
      tauri.hideLauncher();
      break;
    case "web":
      await tauri.openWebUrl(result.url);
      tauri.hideLauncher();
      break;
    case "itsm":
      await tauri.openWebUrl(result.url);
      tauri.hideLauncher();
      break;
    case "confl":
      await tauri.openWebUrl(result.url);
      tauri.hideLauncher();
      break;
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

/// Type icon — uses PNG icons from the provided archive.
/// Returns an <img> element with the matching icon.
function TypeIcon({ result }: { result: SearchResult }) {
  // For file types, select icon based on extension.
  if (result.source === "file") {
    if (result.is_dir) {
      return <PngIcon src={iconFiles} />;
    }
    return <FilePngIcon ext={result.ext} />;
  }

  // Clipboard items: pick an icon based on the kind + content.
  //   • kind = "files"  → file-type icon by extension (Word/Excel/PDF/...)
  //   • kind = "text"   → if content is a URL → Link icon
  //   • kind = "image"  → handled by <img> preview before this point
  //   • fallback        → clipboard icon
  if (result.source === "clipboard") {
    if (result.kind === "files") {
      const firstPath = result.content.split("\n")[0] ?? "";
      const ext = extOfPath(firstPath);
      // Directories — group under the generic Files icon.
      // (We can't easily tell a dir from the path string alone; if the
      // extension is empty, fall through to the Files icon.)
      if (!ext) {
        return <PngIcon src={iconFiles} />;
      }
      return <FilePngIcon ext={ext} />;
    }
    if (result.kind === "text" && isUrlContent(result.content)) {
      return <PngIcon src={iconLinks} />;
    }
    return <PngIcon src={iconClipboard} />;
  }

  // For other sources, use the source-specific icon.
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
  return <PngIcon src={src} />;
}

/// File icon based on extension — uses PNG icons.
function FilePngIcon({ ext }: { ext: string }) {
  const e = ext.toLowerCase();
  if (e === "pdf") return <PngIcon src={iconPdf} />;
  if (["doc", "docx"].includes(e)) return <PngIcon src={iconWord} />;
  if (["xls", "xlsx", "csv"].includes(e)) return <PngIcon src={iconExcel} />;
  if (["ppt", "pptx"].includes(e)) return <PngIcon src={iconPowerpoint} />;
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"].includes(e)) return <PngIcon src={iconImages} />;
  if (["mp3", "wav", "flac", "ogg", "aac", "m4a"].includes(e)) return <PngIcon src={iconAudio} />;
  if (["mp4", "avi", "mov", "mkv", "webm"].includes(e)) return <PngIcon src={iconVideo} />;
  if (["eml", "emlx"].includes(e)) return <PngIcon src={iconMail} />;
  if (["zip", "rar", "7z", "tar", "gz"].includes(e)) return <PngIcon src={iconArchive} />;
  if (["txt", "md", "log"].includes(e)) return <PngIcon src={iconText} />;
  if (["db", "sqlite", "sql"].includes(e)) return <PngIcon src={iconDatabase} />;
  if (["ics", "ical"].includes(e)) return <PngIcon src={iconCalendar} />;
  if (e === "url") return <PngIcon src={iconLinks} />;
  return <PngIcon src={iconOther} />;
}

/// PNG icon wrapper — renders an <img>. CSS `.icon-container img` handles sizing.
function PngIcon({ src }: { src: string }) {
  return (
    <img
      src={src}
      alt=""
      draggable={false}
    />
  );
}

/// Subtle pill badge for source type — tinted, not saturated.
/// For clipboard items, the badge shows the CONTENT TYPE instead of "БУФЕР":
///   • text  → TXT (slate)
///   • text that looks like a URL → URL (cyan)
///   • image (screenshot) → IMG (emerald)
///   • files (copied file) → uppercased extension (PDF/DOCX/PNG/...) by type
/// This matches the user's "variant D" requirement.
function SourceBadgePill({ result }: { result: SearchResult }) {
  const source = result.source;

  // For clipboard items, compute the badge label from kind + content.
  if (source === "clipboard") {
    const clip = result as Extract<SearchResult, { source: "clipboard" }>;
    const { bg, color, label } = clipboardBadge(clip);
    return (
      <span className="badge-pill" style={{ background: bg, color }}>
        {label}
      </span>
    );
  }

  // For non-clipboard sources, use the static styles map.
  const styles: Record<string, { bg: string; color: string; label: string }> = {
    note:      { bg: "rgba(168,85,247,0.15)",  color: "#C084FC", label: "OBSIDIAN" },
    onenote:   { bg: "rgba(147,51,234,0.15)",  color: "#C084FC", label: "ONENOTE" },
    file:      { bg: "rgba(59,130,246,0.15)",  color: "#60A5FA", label: "ФАЙЛ" },
    app:       { bg: "rgba(99,102,241,0.15)",  color: "#818CF8", label: "ПРИЛОЖЕНИЕ" },
    web:       { bg: "rgba(6,182,212,0.15)",   color: "#22D3EE", label: "ИНТЕРНЕТ" },
    itsm:      { bg: "rgba(59,130,246,0.15)",  color: "#60A5FA", label: "ITSM" },
    confl:     { bg: "rgba(99,102,241,0.15)",  color: "#818CF8", label: "SW CONF" },
    jira:      { bg: "rgba(59,130,246,0.15)",  color: "#60A5FA", label: "SW JIRA" },
  };
  const s = styles[source] ?? { bg: "rgba(100,116,139,0.12)", color: "#94A3B8", label: source.toUpperCase() };
  return (
    <span className="badge-pill" style={{ background: s.bg, color: s.color }}>
      {s.label}
    </span>
  );
}

/// Compute the badge label + color for a clipboard item based on its kind
/// and content. Returns the user's "variant D" mapping:
///   • kind = "text"   → if content matches URL regex → "URL", else "TXT"
///   • kind = "image"  → "IMG"
///   • kind = "files"  → uppercased extension of the first file (PDF/DOCX/...)
///                       or "FILE" if the extension is unknown / directory
function clipboardBadge(clip: Extract<SearchResult, { source: "clipboard" }>): { bg: string; color: string; label: string } {
  if (clip.kind === "image") {
    return { bg: "rgba(52,211,153,0.15)", color: "#34D399", label: "IMG" };
  }
  if (clip.kind === "files") {
    const firstPath = clip.content.split("\n")[0] ?? "";
    const ext = extOfPath(firstPath);
    if (!ext) {
      // No extension → likely a directory or unknown file type.
      return { bg: "rgba(100,116,139,0.12)", color: "#94A3B8", label: "FILE" };
    }
    // Pick a color based on the file extension family.
    const color = badgeColorForExt(ext);
    return { bg: `${color}22`, color, label: ext.toUpperCase() };
  }
  // kind === "text" (or unknown) → check if it's a URL.
  if (isUrlContent(clip.content)) {
    return { bg: "rgba(6,182,212,0.15)", color: "#22D3EE", label: "URL" };
  }
  return { bg: "rgba(148,163,184,0.15)", color: "#94A3B8", label: "TXT" };
}

/// Pick a hex color for a file-extension badge. Mirrors the icon color
/// family so the badge visually matches the file-type icon.
function badgeColorForExt(ext: string): string {
  const e = ext.toLowerCase();
  if (["pdf"].includes(e)) return "#F87171"; // red
  if (["doc", "docx"].includes(e)) return "#60A5FA"; // blue
  if (["xls", "xlsx", "csv"].includes(e)) return "#34D399"; // green
  if (["ppt", "pptx"].includes(e)) return "#FB923C"; // orange
  if (["jpg", "jpeg", "png", "gif", "webp", "svg", "bmp", "ico", "tiff"].includes(e)) return "#A78BFA"; // purple
  if (["mp3", "wav", "flac", "ogg", "aac", "m4a"].includes(e)) return "#F472B6"; // pink
  if (["mp4", "avi", "mov", "mkv", "webm"].includes(e)) return "#FB923C"; // orange
  if (["eml", "emlx"].includes(e)) return "#FBBF24"; // amber
  if (["zip", "rar", "7z", "tar", "gz"].includes(e)) return "#FB923C"; // orange
  return "#94A3B8"; // slate (default)
}

export const ResultRow = memo(RowInner);
