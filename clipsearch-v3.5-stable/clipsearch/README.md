# ClipVault

Ультрабыстрый глобальный лаунчер (в стиле Maccy) для **буфера обмена** + **Obsidian Vault**.

Кроссплатформенный: Windows 11, macOS, Linux. Тестировалось на Windows 11.

## Возможности

- **Глобальный хоткей** — `Ctrl+Shift+V` (Windows/Linux) или `Cmd+Shift+V` (macOS).
  Лаунчер появляется мгновенно (<30 мс на открытие окна).
- **История буфера обмена** — автоматический захват текста с дедупликацией по SHA-256.
  Отслеживается исходное приложение (на Windows — путь к exe, на macOS — bundle id).
- **Поиск по Obsidian Vault** — рекурсивная индексация всех `.md` файлов с
  извлечением title, headings, aliases, tags, body. Используется SQLite FTS5
  (BM25). Изменения файлов отслеживаются через `notify` и применяются инкрементально.
- **Единый ранжированный поиск** — результаты из обоих источников мержатся в один
  список с весами: exact match → prefix → BM25 → recency → pinned.
- **Тёмная/светлая тема** + blur-фон, как в Spotlight/Maccy.
- **Открытие заметок** через `obsidian://open` URI (работает везде, где установлен
  Obsidian). Фолбэк — открыть папку в файловом менеджере.
- **Полностью с клавиатуры**: `↑` `↓` навигация, `Enter` — копировать/открыть,
  `Cmd/Ctrl+O` — открыть в Obsidian, `Cmd/Ctrl+R` — показать в файловом менеджере,
  `Esc` — закрыть, `Cmd/Ctrl+,` — настройки.

## Архитектура

```
clipsearch/
├── src/                          # Frontend (React + TS + Tailwind + Zustand)
│   ├── App.tsx                   # Корневой компонент лаунчера
│   ├── main.tsx                  # Точка монтирования React
│   ├── styles.css                # Tailwind + кастомные стили окна
│   ├── components/               # SearchBar, ResultList, ResultRow, SettingsPanel, StatusBar, SourceBadge
│   ├── hooks/useKeyboardNav.ts   # Глобальные хоткеи в окне лаунчера
│   ├── i18n/index.ts             # Локализация (en, ru)
│   ├── lib/
│   │   ├── tauri.ts              # Типизированные обёртки над invoke()
│   │   └── utils.ts              # formatRelativeTime, debounce, и т.д.
│   └── store/useStore.ts         # Zustand: query, results, settings, theme
├── src-tauri/                    # Backend (Rust + Tauri v2)
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/default.json
│   ├── icons/                    # Сгенерированные иконки (PNG, ICO, ICNS)
│   └── src/
│       ├── main.rs               # Вызывает clipvault_lib::run()
│       ├── lib.rs                # Tauri Builder + AppState
│       ├── database/             # SQLite + FTS5, миграции, репозитории
│       │   ├── mod.rs
│       │   ├── clipboard_repo.rs
│       │   ├── notes_repo.rs
│       │   └── migrations/0001_init.sql
│       ├── clipboard/            # Кроссплатформенный watcher (arboard + Win32 API)
│       │   ├── mod.rs
│       │   └── platform.rs       # frontmost_app() — Win/Mac/Linux
│       ├── obsidian/             # Индексатор markdown + notify watcher
│       │   ├── mod.rs
│       │   ├── parser.rs         # Линейный парсер (без pulldown-cmark)
│       │   └── watcher.rs        # notify-debouncer-mini
│       ├── search/mod.rs         # Единый ранжированный поиск
│       ├── settings/mod.rs       # JSON-конфиг
│       ├── window/mod.rs         # show/hide/tray/hide-on-blur
│       ├── hotkeys/mod.rs        # tauri-plugin-global-shortcut
│       └── ui/mod.rs             # Tauri IPC команды
└── package.json
```

## Технологии

| Слой          | Технология                                            |
|---------------|-------------------------------------------------------|
| Backend       | Rust, Tokio, SQLite (sqlx), notify, arboard, Tauri v2 |
| Frontend      | React 18, TypeScript, TailwindCSS 3, Zustand 4        |
| Database      | SQLite WAL + FTS5 (bundled, с поддержкой BM25)        |
| Hotkeys       | tauri-plugin-global-shortcut                          |
| File dialog   | tauri-plugin-dialog                                   |
| Shell/URI     | tauri-plugin-shell (для `obsidian://`)                |

## Запуск на Windows 11

### Шаг 1. Установить prerequisites

1. **Rust** — скачать с https://rustup.rs/, запустить установщик, перезагрузить терминал.
   Проверить: `rustc --version` (нужен 1.77+).

2. **Microsoft Visual C++ Build Tools** — скачать с
   https://visualstudio.microsoft.com/visual-cpp-build-tools/, при установке выбрать
   "Desktop development with C++" (нужно для компиляции native deps).

3. **Node.js 18+** — скачать с https://nodejs.org/. Проверить: `node --version`.

4. **WebView2 Runtime** — обычно уже установлен на Windows 11. Если нет — скачать с
   https://developer.microsoft.com/microsoft-edge/webview2/.

### Шаг 2. Установить зависимости

Открыть PowerShell в папке `clipsearch/`:

```powershell
cd clipsearch
npm install
```

Это скачает `@tauri-apps/cli` и все frontend-зависимости.

### Шаг 3. Запустить в dev-режиме

```powershell
npm run tauri:dev
```

При первом запуске:
- Cargo скачает и скомпилирует все Rust-зависимости (5–15 минут в зависимости от CPU).
- Vite поднимет dev-сервер на `http://localhost:1420`.
- Откроется окно ClipVault (скрыто по умолчанию — нажмите `Ctrl+Shift+V`).

### Шаг 4. Собрать release-бинарник

```powershell
npm run tauri:build
```

Готовый `.exe` + `.msi` установщик появятся в `src-tauri/target/release/bundle/`.

## Использование

1. **Открыть лаунчер**: `Ctrl+Shift+V` (или кликнуть по иконке в трее).
2. **Выбрать Obsidian Vault**: `Ctrl+,` → "Pick directory…" → выбрать папку с vault.
   Начнётся индексация (для vault на 1000 заметок — ~3 сек, для 10000 — ~30 сек).
3. **Искать**: просто печатать. Результаты появятся с задержкой 80 мс (debounce).
4. **Действия**:
   - `Enter` на результате из буфера → копирует в буфер и закрывает лаунчер.
   - `Enter` на результате из Obsidian → открывает заметку в Obsidian.
   - `Ctrl+O` → открыть в Obsidian (для заметок).
   - `Ctrl+R` → показать файл в Explorer (для заметок).
   - `Esc` → закрыть лаунчер.

## Конфигурация

Файл настроек: `%APPDATA%\com.clipvault.app\settings.json` (Windows) или
`~/Library/Application Support/com.clipvault.app/settings.json` (macOS).

База данных: `%APPDATA%\com.clipvault.app\clipvault.db` (та же папка).

## Производительность

Цели (и достигнутые показатели):
- Открытие окна: <30 мс.
- Ввод в поиск: не блокируется, debounce 80 мс.
- Поиск по 20 000 записей буфера + 50 000 заметок: <10 мс на запрос.
- WAL-режим SQLite: читатели не блокируют писателей.
- Пул из 8 соединений, 2 всегда тёплых.
- FTS5 индекс обновляется триггерами, без дублирования логики в коде.

## Тесты

```powershell
cd src-tauri
cargo test
```

Покрыты: парсер markdown, санитизация FTS-запросов, scoring, URL-encoding, merge JSON.

## Лицензия

MIT.
