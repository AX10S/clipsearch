// ─────────────────────────────────────────────────────────────────────────────
// Hotkeys — register the global launcher shortcut.
//
// Tauri v2's `tauri-plugin-global-shortcut` is used. The default binding is
// `CmdOrCtrl+Shift+Space` — `CmdOrCtrl` maps to Cmd on macOS and Ctrl on
// Windows/Linux.
//
// IMPORTANT: If the hotkey is already registered by another app (or by a
// previous crashed instance of ClipVault), we DON'T panic — we log a warning
// and continue. The user can change the hotkey in settings.
// ─────────────────────────────────────────────────────────────────────────────

use anyhow::Result;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings::Settings;

/// Register the configured hotkey. Called at startup.
/// Returns Ok(()) even if registration fails — we don't want to crash the app
/// just because the hotkey is taken.
pub fn register(app: &AppHandle, settings: &Settings) -> Result<()> {
    refresh(app, settings)
}

/// Re-register the hotkey after a settings change.
pub fn refresh(app: &AppHandle, settings: &Settings) -> Result<()> {
    let gs = app.global_shortcut();
    // Unregister all previous shortcuts first — ignore errors (might be none).
    let _ = gs.unregister_all();

    let app_clone = app.clone();
    match gs.on_shortcut(settings.hotkey.as_str(), move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            crate::window::toggle(&app_clone);
        }
    }) {
        Ok(()) => {
            tracing::info!(hotkey = %settings.hotkey, "global shortcut registered");
        }
        Err(e) => {
            // Don't crash — log and continue. The user can change the hotkey
            // in settings (Ctrl+,) or kill the conflicting app.
            tracing::error!(
                hotkey = %settings.hotkey,
                error = ?e,
                "Failed to register global shortcut — it may be in use by another app. \
                 Change the hotkey in settings or close the conflicting application."
            );
        }
    }
    Ok(())
}

/// Unregister everything (called on quit, optional).
pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
