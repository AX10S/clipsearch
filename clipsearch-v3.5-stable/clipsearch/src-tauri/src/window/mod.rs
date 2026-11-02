// ─────────────────────────────────────────────────────────────────────────────
// Window management — show / hide / focus the floating launcher window.
//
// The launcher is a single always-on-top, transparent, decoration-less window.
// It's hidden on startup and toggled by the global hotkey. On macOS we'd
// additionally call `NSApp.activateIgnoringOtherApps` to steal focus; on
// Windows we use `SetForegroundWindow` via Tauri's `set_focus`.
//
// We also wire up the tray icon menu and the "hide on blur" behavior here so
// the rest of the app doesn't have to know about Tauri window events.
// ─────────────────────────────────────────────────────────────────────────────

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

/// Toggle the launcher window: show + focus if hidden, hide if visible.
pub fn toggle(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        if win.is_visible().unwrap_or(false) {
            let _ = win.hide();
        } else {
            show_and_focus(app, &win);
        }
    }
}

/// Show the launcher and steal focus.
pub fn show_launcher(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        show_and_focus(app, &win);
    }
}

/// Hide the launcher.
pub fn hide_launcher(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.hide();
    }
}

fn show_and_focus(app: &AppHandle, win: &tauri::WebviewWindow) {
    let _ = win.center();
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.set_always_on_top(true);

    let _ = app.emit("launcher://shown", ());
}

/// Build the tray icon menu. Clicking the tray icon toggles the launcher.
pub fn setup_tray(app: &AppHandle) -> anyhow::Result<()> {
    use tauri::menu::{Menu, MenuItem};

    let toggle_item = MenuItem::with_id(app, "toggle", "Toggle Launcher", true, None::<&str>)?;
    let reindex_item = MenuItem::with_id(app, "reindex", "Re-index Vault", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit ClipVault", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle_item, &reindex_item, &quit_item])?;

    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_menu(Some(menu));
        let app_clone = app.clone();
        tray.on_menu_event(move |_app, event| match event.id.as_ref() {
            "toggle" => toggle(&app_clone),
            "reindex" => {
                if let Some(state) = app_clone.try_state::<crate::AppState>() {
                    let obs = state.obsidian.lock();
                    if let Some(idx) = obs.as_ref() {
                        idx.reindex_now();
                    }
                }
            }
            "quit" => {
                app_clone.exit(0);
            }
            _ => {}
        });

        let app_clone2 = app.clone();
        tray.on_tray_icon_event(move |_, _event| {
            toggle(&app_clone2);
        });
    }

    Ok(())
}

/// Wire up window events — currently only "hide on blur".
/// Must be called AFTER the main window exists.
pub fn setup_window_events(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let app_clone = app.clone();
        win.on_window_event(move |event| {
            if let WindowEvent::Focused(false) = event {
                hide_launcher(&app_clone);
            }
        });
    }
}
