// ─────────────────────────────────────────────────────────────────────────────
// Filesystem watcher — uses `notify` (via `notify-debouncer-mini`) to watch
// the vault for changes. Debounced to avoid thrashing on bulk Git operations.
//
// Events are translated into `Cmd::UpsertFile` / `Cmd::RemoveFile` and sent
// through the same channel as `Cmd::FullScan`. The indexer worker then
// processes them one at a time, which keeps the DB consistent.
// ─────────────────────────────────────────────────────────────────────────────

use std::time::Duration;

use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent};
use tokio::sync::mpsc;

use super::Cmd;

/// Spawn a debounced watcher over `vault`. Returns immediately; the watcher
/// runs on its own thread for the lifetime of the process.
pub fn spawn(vault: std::path::PathBuf, tx: mpsc::Sender<Cmd>) -> Result<()> {
    // Bridge between the sync notify callback and the async mpsc channel.
    // `notify-debouncer-mini` 0.4 changed the callback signature to
    // `FnMut(Result<Vec<DebouncedEvent>, Vec<notify::Error>>)`.
    let (bridge_tx, bridge_rx) = std::sync::mpsc::channel::<Vec<DebouncedEvent>>();

    // `new_debouncer` in 0.4 takes (timeout, callback) — no `Option<Notify>`.
    let mut debouncer = new_debouncer(Duration::from_millis(300), move |res| {
        if let Ok(events) = res {
            let _ = bridge_tx.send(events);
        }
    })?;

    debouncer
        .watcher()
        .watch(&vault, notify::RecursiveMode::Recursive)?;

    // The debouncer keeps a background thread alive as long as it's not dropped.
    // We leak it intentionally — the watcher should run for the process lifetime.
    std::mem::forget(debouncer);

    tracing::info!(vault = ?vault, "filesystem watcher started");

    // Bridge thread: sync channel -> async mpsc.
    std::thread::Builder::new()
        .name("obsidian-watcher-bridge".into())
        .spawn(move || {
            // `vault` is intentionally unused here — we keep it in scope for
            // potential future path resolution, but notify already gives us
            // absolute paths.
            let _ = vault;
            while let Ok(events) = bridge_rx.recv() {
                for ev in events {
                    // `DebouncedEvent` in notify-debouncer-mini 0.4 exposes
                    // `.path` (single PathBuf) — not `.paths`.
                    let path = ev.path;
                    let ext = path.extension().and_then(|e| e.to_str());
                    if ext == Some("md") || ext == Some("canvas") {
                        let cmd = if path.exists() {
                            Cmd::UpsertFile(path)
                        } else {
                            Cmd::RemoveFile(path)
                        };
                        // try_send: don't block the watcher thread if the
                        // channel is full — we'd rather drop an event than
                        // freeze the watcher.
                        let _ = tx.try_send(cmd);
                    }
                }
            }
        })?;

    Ok(())
}
