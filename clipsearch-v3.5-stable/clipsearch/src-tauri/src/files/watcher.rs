// ─────────────────────────────────────────────────────────────────────────────
// Filesystem watcher for the files module.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent};
use tokio::sync::mpsc;

use super::Cmd;

pub fn spawn(folder: PathBuf, tx: mpsc::Sender<Cmd>) -> Result<()> {
    let (bridge_tx, bridge_rx) = std::sync::mpsc::channel::<Vec<DebouncedEvent>>();

    let mut debouncer = new_debouncer(Duration::from_millis(300), move |res| {
        if let Ok(events) = res {
            let _ = bridge_tx.send(events);
        }
    })?;

    debouncer
        .watcher()
        .watch(&folder, notify::RecursiveMode::Recursive)?;

    std::mem::forget(debouncer);

    tracing::info!(folder = ?folder, "file watcher started");

    let folder_clone = folder.clone();
    std::thread::Builder::new()
        .name("files-watcher-bridge".into())
        .spawn(move || {
            let root = folder_clone.to_string_lossy().to_string();
            while let Ok(events) = bridge_rx.recv() {
                for ev in events {
                    let path = ev.path;
                    let cmd = if path.exists() {
                        Cmd::UpsertFile(path, root.clone())
                    } else {
                        Cmd::RemoveFile(path)
                    };
                    let _ = tx.try_send(cmd);
                }
            }
        })?;

    Ok(())
}
