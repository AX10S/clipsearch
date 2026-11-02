// ActionBar — bottom panel (52px) with key capsules and action hints.
// Uses CSS Variables. Key capsules are 28px height, 8px radius.

import { useStore } from "@/store/useStore";
import { t } from "@/i18n";
import { isMac } from "@/lib/utils";

export function ActionBar() {
  const results = useStore((s) => s.results);
  const selectedIndex = useStore((s) => s.selectedIndex);
  const activeFilter = useStore((s) => s.activeFilter);
  const favoriteKeys = useStore((s) => s.favoriteKeys);
  const theme = useStore((s) => s.theme);
  void theme; // theme is used implicitly through CSS vars

  const mod = isMac() ? "⌘" : "Ctrl";

  const selected = (() => {
    const favoriteKeysInner = useStore.getState().favoriteKeys;
    const filtered = activeFilter === "all"
      ? results
      : activeFilter === "favorites"
      ? results.filter((r) => {
          if ("pinned" in r && r.pinned) return true;
          if (r.source === "file" || r.source === "app" || r.source === "web" || r.source === "itsm") {
            const key = favKeyOf(r);
            return favoriteKeysInner.has(`${r.source}:${key}`);
          }
          return false;
        })
      : results.filter((r) => r.source === activeFilter);
    return filtered[selectedIndex];
  })();

  const isFav = (() => {
    if (!selected) return false;
    if ("pinned" in selected && selected.pinned) return true;
    if (selected.source === "file" || selected.source === "app" || selected.source === "web" || selected.source === "itsm") {
      const key = favKeyOf(selected);
      return favoriteKeys.has(`${selected.source}:${key}`);
    }
    return false;
  })();

  const totalShown = activeFilter === "all"
    ? results.length
    : activeFilter === "favorites"
    ? results.filter((r) => {
        if ("pinned" in r && r.pinned) return true;
        if (r.source === "file" || r.source === "app" || r.source === "web" || r.source === "itsm") {
          return favoriteKeys.has(`${r.source}:${favKeyOf(r)}`);
        }
        return false;
      }).length
    : results.filter((r) => r.source === activeFilter).length;

  return (
    <div
      className="no-drag flex items-center justify-between"
      style={{ height: "44px", padding: "0 8px", flexShrink: 0 }}
    >
      {/* Left: result count */}
      <div className="flex items-center gap-3">
        <span style={{ fontSize: "var(--fs-13)", color: "var(--text-muted)", fontWeight: 500 }}>
          {totalShown > 0 ? `${totalShown} результатов` : t("status.ready")}
        </span>
      </div>

      {/* Right: key capsules */}
      <div className="flex items-center gap-2">
        {selected && (
          <span className="flex items-center gap-1.5">
            <span style={{ fontSize: "var(--fs-13)", color: "var(--text-muted)" }}>
              {isFav ? "Открепить" : "Закрепить"}
            </span>
            <kbd className="key-capsule">{mod} P</kbd>
          </span>
        )}
        {selected && (selected.source === "file" || selected.source === "app" || selected.source === "note") && (
          <span className="flex items-center gap-1.5">
            <span style={{ fontSize: "var(--fs-13)", color: "var(--text-muted)" }}>Папка</span>
            <kbd className="key-capsule">{mod} ↵</kbd>
          </span>
        )}
        <kbd className="key-capsule">↵</kbd>
        <kbd className="key-capsule">↑↓</kbd>
        <kbd className="key-capsule">Esc</kbd>
      </div>
    </div>
  );
}

function favKeyOf(r: import("@/lib/tauri").SearchResult): string {
  switch (r.source) {
    case "file": return r.path;
    case "app": return r.exec_path;
    case "web": return r.url;
    case "itsm": return r.url;
    default: return "";
  }
}
