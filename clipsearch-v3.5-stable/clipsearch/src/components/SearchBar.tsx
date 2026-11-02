// SearchBar — premium search input (56px height, 18px radius, large icon).
//
// Uses CSS Variables from the design system. No Tailwind utility classes for
// colors — everything goes through var(--...).

import { useEffect, useRef } from "react";
import { useStore } from "@/store/useStore";
import { t } from "@/i18n";

export function SearchBar() {
  const query = useStore((s) => s.query);
  const setQuery = useStore((s) => s.setQuery);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    import("@/lib/tauri").then((tauri) => {
      tauri.onLauncherShown(() => {
        setQuery("");
        inputRef.current?.focus();
      }).then((un) => (unlisten = un));
    });
    return () => unlisten?.();
  }, [setQuery]);

  return (
    <div
      className="no-drag card flex items-center gap-3"
      style={{
        height: "56px",
        padding: "0 20px",
        borderRadius: "var(--r-input)",
        flex: "1 1 auto",
        minWidth: "0",
      }}
      data-tauri-drag-region={false}
    >
      {/* Search icon — large, muted */}
      <svg
        className="shrink-0"
        style={{ width: "24px", height: "24px", color: "var(--text-muted)" }}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      >
        <circle cx="11" cy="11" r="7" />
        <line x1="21" y1="21" x2="16.65" y2="16.65" />
      </svg>

      <input
        ref={inputRef}
        type="text"
        className="search-input flex-1 bg-transparent outline-none"
        style={{
          fontSize: "var(--fs-18)",
          fontWeight: 500,
          color: "var(--text-primary)",
        }}
        placeholder={t("search.placeholder")}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        spellCheck={false}
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
      />

      {query && (
        <button
          className="shrink-0 transition-colors"
          style={{
            color: "var(--text-muted)",
            fontSize: "var(--fs-14)",
            padding: "4px 10px",
            borderRadius: "8px",
            cursor: "default",
          }}
          onClick={() => setQuery("")}
          onMouseEnter={(e) => (e.currentTarget.style.color = "var(--text-primary)")}
          onMouseLeave={(e) => (e.currentTarget.style.color = "var(--text-muted)")}
          aria-label="Clear"
        >
          ✕
        </button>
      )}
    </div>
  );
}
