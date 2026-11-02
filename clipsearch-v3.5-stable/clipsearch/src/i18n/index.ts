// ─────────────────────────────────────────────────────────────────────────────
// i18n — minimal, hand-rolled. No `i18next` dependency for ~10 strings.
// Two locales: `en` and `ru`. The active locale is auto-detected from
// `navigator.language` and can be overridden via `setLocale`.
// ─────────────────────────────────────────────────────────────────────────────

export type Locale = "en" | "ru";

const STRINGS = {
  en: {
    "search.placeholder": "Search clipboard and notes…",
    "search.empty.title": "Nothing here yet",
    "search.empty.body": "Copy something, or select an Obsidian vault to start indexing.",
    "search.no_results": "No results",
    "search.results.count_one": "{{count}} result",
    "search.results.count_other": "{{count}} results",
    "source.clipboard": "Clipboard",
    "source.note": "Obsidian",
    "action.copy": "Copy",
    "action.open": "Open in Obsidian",
    "action.reveal": "Reveal in file manager",
    "action.pin": "Pin",
    "action.unpin": "Unpin",
    "action.delete": "Delete",
    "settings.title": "Settings",
    "settings.vault_path": "Obsidian vault path",
    "settings.vault_path.placeholder": "Not selected",
    "settings.vault_path.pick": "Pick directory…",
    "settings.hotkey": "Global hotkey",
    "settings.history_limit": "Clipboard history limit",
    "settings.theme": "Theme",
    "settings.theme.system": "System",
    "settings.theme.light": "Light",
    "settings.theme.dark": "Dark",
    "settings.reindex": "Re-index vault",
    "settings.clear": "Clear clipboard history",
    "index.scanning": "Indexing…",
    "index.scanned": "{{done}} / {{total}}",
    "index.notes": "{{count}} notes",
    "status.ready": "Ready",
  },
  ru: {
    "search.placeholder": "Поиск",
    "search.empty.title": "Здесь пока пусто",
    "search.empty.body": "Скопируйте что-нибудь или выберите хранилище Obsidian для индексации.",
    "search.no_results": "Ничего не найдено",
    "search.results.count_one": "{{count}} результат",
    "search.results.count_other": "{{count}} результатов",
    "source.clipboard": "Буфер",
    "source.note": "Obsidian",
    "action.copy": "Копировать",
    "action.open": "Открыть в Obsidian",
    "action.reveal": "Показать в файловом менеджере",
    "action.pin": "Закрепить",
    "action.unpin": "Открепить",
    "action.delete": "Удалить",
    "settings.title": "Настройки",
    "settings.vault_path": "Путь к хранилищу Obsidian",
    "settings.vault_path.placeholder": "Не выбран",
    "settings.vault_path.pick": "Выбрать папку…",
    "settings.hotkey": "Глобальная горячая клавиша",
    "settings.history_limit": "Лимит истории буфера",
    "settings.theme": "Тема",
    "settings.theme.system": "Системная",
    "settings.theme.light": "Светлая",
    "settings.theme.dark": "Тёмная",
    "settings.reindex": "Переиндексировать хранилище",
    "settings.clear": "Очистить историю буфера",
    "index.scanning": "Индексация…",
    "index.scanned": "{{done}} / {{total}}",
    "index.notes": "{{count}} заметок",
    "status.ready": "Готово",
  },
} as const;

export type StringKey = keyof typeof STRINGS["en"];

let currentLocale: Locale = detectLocale();

function detectLocale(): Locale {
  const lang = (navigator.language || "en").toLowerCase();
  if (lang.startsWith("ru")) return "ru";
  return "en";
}

export function getLocale(): Locale {
  return currentLocale;
}

export function setLocale(locale: Locale): void {
  currentLocale = locale;
}

/** Translate a key, substituting `{{placeholders}}`. */
export function t(key: StringKey, vars?: Record<string, string | number>): string {
  let s: string = (STRINGS[currentLocale] as Record<string, string>)[key] ?? key;
  if (vars) {
    for (const [k, v] of Object.entries(vars)) {
      s = s.replace(new RegExp(`\\{\\{${k}\\}\\}`, "g"), String(v));
    }
  }
  return s;
}

/** Pluralization helper (English-style for now: one vs other). */
export function tn(
  key: string,
  count: number,
  vars?: Record<string, string | number>
): string {
  const pluralKey = count === 1 ? `${key}_one` : `${key}_other`;
  return t(pluralKey as StringKey, { count, ...vars });
}
