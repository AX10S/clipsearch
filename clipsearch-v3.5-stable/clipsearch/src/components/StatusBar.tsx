// Status bar at the bottom of the launcher — shows result count + indexing
// status + a settings gear.

import { useStore } from "@/store/useStore";
import { t, tn } from "@/i18n";

export function StatusBar() {
  const results = useStore((s) => s.results);
  const indexerStatus = useStore((s) => s.indexerStatus);
  const openSettings = useStore((s) => s.openSettings);
  const theme = useStore((s) => s.theme);

  const noteCount = indexerStatus?.indexed_count ?? 0;
  const scanning = indexerStatus?.scanning ?? false;

  const metaCls = theme === "dark" ? "text-meta-dark" : "text-meta-light";

  return (
    <div
      className={`no-drag flex items-center justify-between px-5 py-2.5 border-t border-black/10 dark:border-white/10 text-[12px] font-medium ${metaCls}`}
    >
      <div className="flex items-center gap-3">
        <span className="tnum">
          {results.length > 0 ? tn("search.results.count", results.length) : t("status.ready")}
        </span>
        {noteCount > 0 && (
          <span className="tnum opacity-80">
            {t("index.notes", { count: noteCount })}
          </span>
        )}
        {scanning && (
          <span className="flex items-center gap-1.5 text-purple-600 dark:text-purple-400 font-semibold">
            <span className="w-2 h-2 bg-purple-600 dark:bg-purple-400 rounded-full animate-pulse-soft" />
            {t("index.scanning")}
          </span>
        )}
      </div>
      <button
        className="hover:text-black/80 dark:hover:text-white/80 transition-colors p-1"
        onClick={openSettings}
        title={t("settings.title")}
        aria-label={t("settings.title")}
      >
        <svg
          className="w-5 h-5"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <circle cx="12" cy="12" r="3" />
          <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
        </svg>
      </button>
    </div>
  );
}
