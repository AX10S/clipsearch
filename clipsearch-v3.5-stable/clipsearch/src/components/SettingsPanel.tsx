// Settings panel — modal overlay inside the launcher window.
// Uses CSS variables for full theme integration.

import { useEffect, useState, useRef, useCallback } from "react";
import { useStore } from "@/store/useStore";
import { t } from "@/i18n";
import * as tauri from "@/lib/tauri";
import type { Settings } from "@/lib/tauri";

const labelStyle: React.CSSProperties = {
  display: "block", fontSize: "var(--fs-13)", fontWeight: 500,
  color: "var(--text-secondary)", marginBottom: "6px",
};

const inputStyle: React.CSSProperties = {
  width: "100%", padding: "8px 12px", fontSize: "var(--fs-14)",
  borderRadius: "var(--r-button)", background: "var(--bg-icon)",
  border: "1px solid var(--border)", color: "var(--text-primary)",
  outline: "none",
};

const btnActive: React.CSSProperties = {
  flex: 1, padding: "8px 12px", fontSize: "var(--fs-13)", fontWeight: 500,
  borderRadius: "var(--r-button)", background: "var(--accent)", color: "#FFF",
  border: "none", cursor: "default",
};

const btnInactive: React.CSSProperties = {
  flex: 1, padding: "8px 12px", fontSize: "var(--fs-13)", fontWeight: 500,
  borderRadius: "var(--r-button)", background: "var(--bg-icon)", color: "var(--text-secondary)",
  border: "1px solid var(--border)", cursor: "default",
};

const btnPrimary: React.CSSProperties = {
  padding: "8px 12px", fontSize: "var(--fs-14)", fontWeight: 500,
  borderRadius: "var(--r-button)", background: "var(--accent)", color: "#FFF",
  border: "none", cursor: "default",
};

export function SettingsPanel() {
  const settings = useStore((s) => s.settings);
  const indexerStatus = useStore((s) => s.indexerStatus);
  const closeSettings = useStore((s) => s.closeSettings);
  const pickVault = useStore((s) => s.pickVault);
  const reindex = useStore((s) => s.reindex);
  const updateSettings = useStore((s) => s.updateSettings);
  const clearClipboard = useStore((s) => s.clearClipboard);

  const [draft, setDraft] = useState<Settings | null>(null);
  const [isDirty, setIsDirty] = useState(false);
  // Autostart is owned by the OS, not settings.json — we keep a separate
  // state for it and load the live value from the OS on panel mount.
  const [autostartEnabled, setAutostartEnabled] = useState<boolean | null>(null);

  useEffect(() => {
    if (settings) {
      setDraft({ ...settings });
      setIsDirty(false);
    }
  }, [settings]);

  // Load the live autostart state from the OS (registry on Windows,
  // LaunchAgent plist on macOS) once when the panel opens.
  useEffect(() => {
    tauri.isAutostartEnabled()
      .then(setAutostartEnabled)
      .catch(() => setAutostartEnabled(false));
  }, []);

  if (!settings || !draft) return null;

  const patchDraft = (patch: Partial<Settings>) => {
    setDraft((prev) => (prev ? { ...prev, ...patch } : prev));
    setIsDirty(true);
  };

  const onPickVault = async () => {
    const path = await tauri.pickVaultDirectory();
    if (path) {
      await pickVault(path);
      setDraft((prev) => (prev ? { ...prev, obsidian_vault_path: path } : prev));
    }
  };

  const onSave = async () => {
    if (isDirty) {
      await updateSettings({
        hotkey: draft.hotkey,
        clipboard_history_limit: draft.clipboard_history_limit,
        theme: draft.theme,
        web_search_provider: draft.web_search_provider,
        indexed_folders: draft.indexed_folders,
        enabled_sources: draft.enabled_sources,
        window_width: draft.window_width,
        window_height: draft.window_height,
        clipboard_poll_ms: draft.clipboard_poll_ms,
        show_on_active_space: draft.show_on_active_space,
        compact_mode: draft.compact_mode,
      });
    }
    closeSettings();
  };

  const onCancel = () => {
    setDraft(settings ? { ...settings } : null);
    setIsDirty(false);
    closeSettings();
  };

  return (
    <div
      className="absolute inset-0 z-50 flex items-center justify-center"
      style={{ background: "rgba(0,0,0,0.5)", backdropFilter: "blur(4px)" }}
      onClick={onCancel}
    >
      <div
        className="w-[88%] max-w-md max-h-[88%] overflow-y-auto"
        style={{
          borderRadius: "var(--r-card)",
          background: "var(--bg-card)",
          border: "1px solid var(--border)",
          boxShadow: "0 20px 60px rgba(0,0,0,0.5)",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3" style={{ borderBottom: "1px solid var(--border)" }}>
          <h2 style={{ fontSize: "var(--fs-18)", fontWeight: 600, color: "var(--text-primary)" }}>
            {t("settings.title")}
          </h2>
          <button
            style={{ color: "var(--text-muted)", fontSize: "var(--fs-14)", padding: "4px 8px", borderRadius: "8px", cursor: "default", background: "transparent", border: "none" }}
            onClick={onCancel}
            onMouseEnter={(e) => (e.currentTarget.style.color = "var(--text-primary)")}
            onMouseLeave={(e) => (e.currentTarget.style.color = "var(--text-muted)")}
          >
            ✕
          </button>
        </div>

        {/* Body */}
        <div className="px-5 py-4 space-y-5">
          {/* Vault path */}
          <div>
            <label style={labelStyle}>{t("settings.vault_path")}</label>
            <div className="flex gap-2">
              <input
                type="text"
                readOnly
                value={draft.obsidian_vault_path ?? ""}
                placeholder={t("settings.vault_path.placeholder")}
                style={{ ...inputStyle, flex: 1 }}
              />
              <button style={btnPrimary} onClick={onPickVault}>
                {t("settings.vault_path.pick")}
              </button>
            </div>
            {indexerStatus && indexerStatus.vault_path && (
              <div style={{ marginTop: "6px", fontSize: "var(--fs-11)", color: "var(--text-muted)" }}>
                {indexerStatus.scanning ? t("index.scanning") : t("index.notes", { count: indexerStatus.indexed_count })}
                {indexerStatus.last_scan_ms > 0 && (
                  <span className="ml-2 tnum">· {(indexerStatus.last_scan_ms / 1000).toFixed(1)}s</span>
                )}
              </div>
            )}
            {draft.obsidian_vault_path && (
              <button
                style={{ marginTop: "8px", fontSize: "var(--fs-11)", color: "var(--accent)", background: "transparent", border: "none", cursor: "default", padding: 0 }}
                onClick={() => reindex()}
              >
                {t("settings.reindex")}
              </button>
            )}
          </div>

          {/* Hotkey */}
          <div>
            <label style={labelStyle}>{t("settings.hotkey")}</label>
            <HotkeyInput
              value={draft.hotkey}
              onChange={(v) => patchDraft({ hotkey: v })}
            />
            <p style={{ marginTop: "4px", fontSize: "var(--fs-11)", color: "var(--text-muted)" }}>
              Нажмите сочетание клавиш для записи
            </p>
          </div>

          {/* History limit */}
          <div>
            <label style={labelStyle}>{t("settings.history_limit")}</label>
            <input
              type="number"
              min={0}
              max={100000}
              value={draft.clipboard_history_limit}
              onChange={(e) => patchDraft({ clipboard_history_limit: parseInt(e.target.value, 10) || 0 })}
              style={inputStyle}
            />
          </div>

          {/* Theme */}
          <div>
            <label style={labelStyle}>{t("settings.theme")}</label>
            <div className="flex gap-2">
              {["light", "dark"].map((opt) => (
                <button
                  key={opt}
                  style={draft.theme === opt ? btnActive : btnInactive}
                  onClick={() => patchDraft({ theme: opt })}
                >
                  {opt === "light" ? "Светлая" : "Тёмная"}
                </button>
              ))}
            </div>
          </div>

          {/* View mode — small (standard) / large */}
          <div>
            <label style={labelStyle}>Режим просмотра</label>
            <div className="flex gap-2">
              {[true, false].map((opt) => (
                <button
                  key={String(opt)}
                  style={draft.compact_mode === opt ? btnActive : btnInactive}
                  onClick={() => patchDraft({ compact_mode: opt })}
                >
                  {opt ? "Стандартный" : "Крупный"}
                </button>
              ))}
            </div>
          </div>

          {/* Autostart — runs at OS login */}
          <div>
            <label style={labelStyle}>Автозапуск при включении компьютера</label>
            <div className="flex gap-2">
              {[true, false].map((opt) => (
                <button
                  key={String(opt)}
                  style={autostartEnabled === opt ? btnActive : btnInactive}
                  onClick={async () => {
                    try {
                      if (opt) {
                        await tauri.enableAutostart();
                      } else {
                        await tauri.disableAutostart();
                      }
                      setAutostartEnabled(opt);
                    } catch (e) {
                      console.error("autostart toggle failed", e);
                    }
                  }}
                >
                  {opt ? "Включено" : "Отключено"}
                </button>
              ))}
            </div>
          </div>

          {/* Web search provider */}
          <div>
            <label style={labelStyle}>Web search provider</label>
            <div className="flex gap-2">
              {["google", "bing", "duckduckgo", "yandex"].map((opt) => (
                <button
                  key={opt}
                  style={draft.web_search_provider === opt ? btnActive : btnInactive}
                  onClick={() => patchDraft({ web_search_provider: opt })}
                >
                  {opt.charAt(0).toUpperCase() + opt.slice(1)}
                </button>
              ))}
            </div>
          </div>

          {/* Indexed folders */}
          <div>
            <label style={labelStyle}>Indexed folders (file search)</label>
            <div className="space-y-1 mb-2">
              {draft.indexed_folders.length === 0 ? (
                <div style={{ fontSize: "var(--fs-11)", color: "var(--text-muted)", fontStyle: "italic" }}>
                  Using defaults: Documents, Desktop, Downloads
                </div>
              ) : (
                draft.indexed_folders.map((f) => (
                  <div
                    key={f}
                    className="flex items-center justify-between"
                    style={{ fontSize: "var(--fs-11)", padding: "4px 8px", borderRadius: "6px", background: "var(--bg-icon)" }}
                  >
                    <span style={{ color: "var(--text-secondary)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{f}</span>
                    <button
                      style={{ color: "#EF4444", marginLeft: "8px", flexShrink: 0, background: "transparent", border: "none", cursor: "default", fontSize: "var(--fs-11)" }}
                      onClick={() => patchDraft({ indexed_folders: draft.indexed_folders.filter((x) => x !== f) })}
                    >
                      ✕
                    </button>
                  </div>
                ))
              )}
            </div>
            <button style={btnPrimary} onClick={async () => {
              const path = await tauri.pickVaultDirectory();
              if (path) patchDraft({ indexed_folders: [...draft.indexed_folders, path] });
            }}>
              + Add folder
            </button>
            <p style={{ marginTop: "6px", fontSize: "var(--fs-11)", color: "var(--text-muted)" }}>
              Папки для поиска файлов, OneNote (.one) и других документов.
            </p>
          </div>

          {/* Enabled sources */}
          <div>
            <label style={labelStyle}>Enabled sources</label>
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "8px" }}>
              {(["clipboard", "obsidian", "onenote", "files", "web", "itsm", "confl", "jira"] as const).map((src) => (
                <label
                  key={src}
                  className="flex items-center gap-2"
                  style={{ fontSize: "var(--fs-13)", color: "var(--text-secondary)", cursor: "pointer" }}
                >
                  <input
                    type="checkbox"
                    checked={draft.enabled_sources[src] ?? false}
                    onChange={(e) =>
                      patchDraft({
                        enabled_sources: { ...draft.enabled_sources, [src]: e.target.checked },
                      })
                    }
                  />
                  <span style={{ fontWeight: 500, textTransform: "capitalize", color: "var(--text-primary)" }}>{src}</span>
                </label>
              ))}
            </div>
          </div>

          {/* Danger zone */}
          <div style={{ paddingTop: "8px", borderTop: "1px solid var(--border)" }}>
            <button
              style={{ fontSize: "var(--fs-12)", color: "#EF4444", background: "transparent", border: "none", cursor: "default", padding: 0 }}
              onClick={() => { if (confirm(t("settings.clear") + "?")) clearClipboard(); }}
            >
              {t("settings.clear")}
            </button>
          </div>
        </div>

        {/* Footer — Cancel / Save */}
        <div className="flex justify-end gap-2 px-5 py-3" style={{ borderTop: "1px solid var(--border)" }}>
          <button
            style={{
              padding: "8px 16px", fontSize: "var(--fs-13)", fontWeight: 500,
              borderRadius: "var(--r-button)", background: "var(--bg-icon)",
              color: "var(--text-secondary)", border: "none", cursor: "default",
            }}
            onClick={onCancel}
          >
            Отменить
          </button>
          <button
            style={{
              padding: "8px 16px", fontSize: "var(--fs-13)", fontWeight: 600,
              borderRadius: "var(--r-button)",
              background: isDirty ? "var(--accent)" : "var(--bg-icon)",
              color: isDirty ? "#FFF" : "var(--text-muted)",
              border: "none", cursor: "default",
            }}
            onClick={onSave}
            disabled={!isDirty}
          >
            Сохранить
          </button>
        </div>
      </div>
    </div>
  );
}

/// ── HotkeyInput ─────────────────────────────────────────────────────────────
function HotkeyInput({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [capturing, setCapturing] = useState(false);
  const inputRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (!capturing) return;
      e.preventDefault();
      e.stopPropagation();
      const isModOnly = e.key === "Control" || e.key === "Shift" || e.key === "Alt" || e.key === "Meta";
      if (isModOnly) return;

      const parts: string[] = [];
      const isMac = navigator.userAgent.includes("Mac");
      if (e.ctrlKey || e.metaKey) parts.push(isMac ? "CmdOrCtrl" : "Ctrl");
      if (e.shiftKey) parts.push("Shift");
      if (e.altKey) parts.push("Alt");

      let keyName = e.code;
      if (e.code === "Space") keyName = "Space";
      else if (e.code === "Enter") keyName = "Enter";
      else if (e.code === "Tab") keyName = "Tab";
      else if (e.code === "Escape") { setCapturing(false); return; }
      else if (e.code === "ArrowUp") keyName = "Up";
      else if (e.code === "ArrowDown") keyName = "Down";
      else if (e.code === "ArrowLeft") keyName = "Left";
      else if (e.code === "ArrowRight") keyName = "Right";

      parts.push(keyName);
      onChange(parts.join("+"));
      setCapturing(false);
      inputRef.current?.blur();
    },
    [capturing, onChange]
  );

  useEffect(() => {
    if (capturing) {
      window.addEventListener("keydown", handleKeyDown, true);
      return () => window.removeEventListener("keydown", handleKeyDown, true);
    }
  }, [capturing, handleKeyDown]);

  return (
    <div
      ref={inputRef}
      tabIndex={0}
      onClick={() => setCapturing(true)}
      onBlur={() => setCapturing(false)}
      style={{
        width: "100%", padding: "8px 12px", fontSize: "var(--fs-14)",
        background: capturing ? "var(--accent-faint)" : "var(--bg-icon)",
        color: "var(--text-primary)",
        border: capturing ? "1px solid var(--accent)" : "1px solid var(--border)",
        borderRadius: "var(--r-button)", outline: "none", cursor: "default",
        minHeight: "36px", display: "flex", alignItems: "center", justifyContent: "center", fontWeight: 500,
      }}
    >
      {capturing ? "Нажмите сочетание клавиш…" : value || "Нажмите для записи"}
    </div>
  );
}
