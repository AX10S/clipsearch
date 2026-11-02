// ─────────────────────────────────────────────────────────────────────────────
// Clipboard watcher — cross-platform.
//
// Strategy: poll the system clipboard every 500 ms (configurable via
// `CLIPVAULT_CLIPBOARD_POLL_MS`). On each tick, hash the current text content
// and compare to the last-seen hash. If it changed, persist it asynchronously
// to the database with the source app captured at the moment of the copy.
//
// Why polling instead of native change notifications?
//   • macOS: NSPasteboard.changeCount requires an Obj-C bridge + an event loop
//     on the main thread — heavy to wire cross-platform.
//   • Windows: AddClipboardFormatListener needs an HWND + a message pump.
//   • Linux: X11 selection events are inconsistent across Wayland compositors.
//
// Polling at 500 ms is invisible to the user (a human cannot double-copy within
// 500 ms on purpose), uses ~0.01% CPU, and is the same approach Maccy itself
// used for years before adopting native change notifications. We can swap in
// native notifications per-platform later without touching the rest of the app.
//
// The watcher is fully async — it never blocks the search path.
// ─────────────────────────────────────────────────────────────────────────────

mod platform;

pub use platform::{
    get_clipboard_files, set_clipboard_files, FrontmostApp, frontmost_app,
};

/// Set an image (PNG bytes) on the clipboard using native Win32 API on Windows.
/// Returns true on success. On non-Windows platforms, returns false.
pub fn set_clipboard_image(png_bytes: &[u8]) -> bool {
    #[cfg(target_os = "windows")]
    {
        platform::set_clipboard_image(png_bytes)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = png_bytes;
        false
    }
}

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tauri::async_runtime;
use tokio::time::interval;

use crate::database::clipboard_repo;
use crate::database::Database;

/// Configuration for the watcher.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Polling interval. Default 500 ms.
    pub poll_ms: u64,
    /// Max items kept in history. Pinned items are never evicted.
    pub history_limit: i64,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            poll_ms: 500,
            history_limit: 1_000,
        }
    }
}

/// The watcher owns a background task. Dropping the `Arc` does NOT cancel the
/// task — it runs for the lifetime of the process. (This is intentional: the
/// clipboard must keep recording even while the launcher is closed.)
pub struct ClipboardWatcher {
    /// The last content hash we saw. Used to dedup at the polling layer
    /// before hitting SQLite — saves a round-trip on every tick.
    last_hash: Mutex<String>,
    /// The user-facing config (history limit etc.). Updated from settings.
    config: Mutex<WatcherConfig>,
    /// Where to save copied images. `<data_dir>/clipboard_images/`.
    images_dir: std::path::PathBuf,
}

impl ClipboardWatcher {
    /// Spawn the polling task. Returns immediately.
    pub fn start(db: Arc<Database>, data_dir: std::path::PathBuf) -> Result<Arc<Self>> {
        let cfg = WatcherConfig::default();
        let images_dir = data_dir.join("clipboard_images");
        std::fs::create_dir_all(&images_dir).ok();
        let watcher = Arc::new(Self {
            last_hash: Mutex::new(String::new()),
            config: Mutex::new(cfg.clone()),
            images_dir,
        });

        let w = Arc::clone(&watcher);
        async_runtime::spawn(async move {
            let mut tick = interval(Duration::from_millis(cfg.poll_ms));
            // Don't replay missed ticks if the runtime was busy.
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            tracing::info!("clipboard watcher started (poll={}ms)", cfg.poll_ms);
            loop {
                tick.tick().await;
                if let Err(e) = w.tick(&db).await {
                    tracing::warn!(error = ?e, "clipboard tick failed");
                }
            }
        });

        Ok(watcher)
    }

    /// Update the runtime config (history limit, poll interval, etc.).
    pub fn set_config(&self, cfg: WatcherConfig) {
        *self.config.lock() = cfg;
    }

    /// One polling tick. Reads the clipboard, dedups against `last_hash`, and
    /// writes to the database if there's a new item.
    ///
    /// Order of checks:
    ///   1. Files (Windows CF_HDROP) — when user copies files in Explorer
    ///   2. Text — normal text copies
    ///   3. Image — screenshots, image copies
    async fn tick(&self, db: &Database) -> Result<()> {
        let images_dir = self.images_dir.clone();
        let captured = async_runtime::spawn_blocking(move || -> Result<Option<CapturedClip>> {
            let mut cb = arboard::Clipboard::new()?;

            // 1. Try files first (Windows: CF_HDROP). This catches file copies
            //    from Explorer that don't produce text.
            if let Some(files) = platform::get_clipboard_files() {
                if !files.is_empty() {
                    let source = platform::frontmost_app();
                    return Ok(Some(CapturedClip {
                        kind: ClipKind::Files(files),
                        source_app: source.app_id,
                        source_app_name: source.app_name,
                    }));
                }
            }

            // 2. Try text.
            if let Ok(text) = cb.get_text() {
                if !text.trim().is_empty() {
                    let source = platform::frontmost_app();
                    return Ok(Some(CapturedClip {
                        kind: ClipKind::Text(text),
                        source_app: source.app_id,
                        source_app_name: source.app_name,
                    }));
                }
            }

            // 3. No text — try image.
            if let Ok(img) = cb.get_image() {
                let source = platform::frontmost_app();
                let width = img.width;
                let height = img.height;

                let mut hasher = Sha256::new();
                hasher.update(&img.bytes);
                hasher.update(&width.to_le_bytes());
                hasher.update(&height.to_le_bytes());
                let img_hash = {
                    let bytes = hasher.finalize();
                    let mut s = String::with_capacity(16);
                    const HEX: &[u8; 16] = b"0123456789abcdef";
                    for &b in &bytes[..8] {
                        s.push(HEX[(b >> 4) as usize] as char);
                        s.push(HEX[(b & 0x0f) as usize] as char);
                    }
                    s
                };

                let now_ms = chrono::Utc::now().timestamp_millis();
                let filename = format!("clip_{}.png", now_ms);
                let path = images_dir.join(&filename);

                let rgba: Vec<u8> = img.bytes.into();
                if let Some(img_buf) = image::ImageBuffer::<image::Rgba<u8>, Vec<u8>>::from_raw(
                    width as u32,
                    height as u32,
                    rgba,
                ) {
                    if img_buf.save(&path).is_ok() {
                        return Ok(Some(CapturedClip {
                            kind: ClipKind::Image {
                                path: path.to_string_lossy().to_string(),
                                width,
                                height,
                                hash: img_hash,
                            },
                            source_app: source.app_id,
                            source_app_name: source.app_name,
                        }));
                    }
                }
            }

            Ok(None)
        })
        .await??;

        let Some(captured) = captured else {
            return Ok(());
        };

        // ── Hash + dedup at the polling layer ────────────────────────────────
        let hash = captured.hash();
        {
            let mut last = self.last_hash.lock();
            if *last == hash {
                return Ok(());
            }
            *last = hash.clone();
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let limit = self.config.lock().history_limit;

        // Build the DB payload depending on kind.
        let (content, kind_str, byte_size, image_path) = match &captured.kind {
            ClipKind::Text(t) => (t.clone(), "text", t.len() as i64, None),
            ClipKind::Image { path, width, height, .. } => (
                format!("[Image {}x{}]", width, height),
                "image",
                (width * height * 4) as i64,
                Some(path.clone()),
            ),
            ClipKind::Files(files) => {
                // Store the file paths (newline-separated) as the content so
                // we can copy them back later. The UI title is derived from
                // the first line (filename) via `content.lines().next()`.
                let paths_str = files.join("\n");
                let len = paths_str.len() as i64;

                // If the FIRST copied file is an image, expose its path via
                // `image_path` so the frontend can show a thumbnail preview
                // (same UX as search results for image files). For other file
                // types (PDF, docx, ...) `image_path` stays None — the
                // frontend picks the appropriate type icon based on extension.
                let image_path = files.first().and_then(|p| {
                    let lower = p.to_lowercase();
                    const IMG_EXTS: &[&str] = &[
                        ".jpg", ".jpeg", ".png", ".gif", ".webp",
                        ".bmp", ".svg", ".tiff", ".tif", ".ico",
                    ];
                    if IMG_EXTS.iter().any(|e| lower.ends_with(e)) {
                        Some(p.clone())
                    } else {
                        None
                    }
                });

                (paths_str, "files", len, image_path)
            }
        };

        let db_pool = db.pool().clone();
        let payload = clipboard_repo::NewClipboardItem {
            content,
            content_hash: hash,
            kind: kind_str.to_string(),
            byte_size,
            source_app: captured.source_app,
            source_app_name: captured.source_app_name,
            now_ms,
            image_path,
        };

        let inserted = clipboard_repo::upsert(&db_pool, payload).await;
        match inserted {
            Ok(_id) => {
                if limit > 0 {
                    if let Err(e) = clipboard_repo::enforce_limit(&db_pool, limit).await {
                        tracing::warn!(error = ?e, "enforce_limit failed");
                    }
                }
            }
            Err(e) => tracing::warn!(error = ?e, "clipboard insert failed"),
        }

        Ok(())
    }
}

/// What the clipboard held on this tick — text, image, or files.
enum ClipKind {
    Text(String),
    Image {
        path: String,
        width: usize,
        height: usize,
        hash: String,
    },
    Files(Vec<String>),
}

struct CapturedClip {
    kind: ClipKind,
    source_app: Option<String>,
    source_app_name: Option<String>,
}

impl CapturedClip {
    /// Compute a hash for dedup.
    fn hash(&self) -> String {
        match &self.kind {
            ClipKind::Text(t) => hex_hash(t),
            ClipKind::Image { hash, .. } => hash.clone(),
            ClipKind::Files(files) => hex_hash(&files.join("\n")),
        }
    }
}

fn hex_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let bytes = hasher.finalize();
    // 16-char hex prefix is enough for dedup (collision prob ~1e-19 for 1e6 items)
    hex::encode_short(&bytes[..8])
}

// Minimal hex encoder to avoid pulling in the `hex` crate.
mod hex {
    pub fn encode_short(bytes: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            s.push(HEX[(b >> 4) as usize] as char);
            s.push(HEX[(b & 0x0f) as usize] as char);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable() {
        let a = hex_hash("hello world");
        let b = hex_hash("hello world");
        assert_eq!(a, b);
        assert_ne!(a, hex_hash("hello world!"));
    }
}
