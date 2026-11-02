// ─────────────────────────────────────────────────────────────────────────────
// Platform-specific app scanning.
// ─────────────────────────────────────────────────────────────────────────────

use super::AppInfo;

/// Scan the system for installed apps. Returns all found apps; the caller
/// caches them and searches the cache on every keystroke.
pub fn scan_installed_apps() -> Vec<AppInfo> {
    #[cfg(target_os = "windows")]
    {
        scan_windows()
    }
    #[cfg(target_os = "macos")]
    {
        scan_macos()
    }
    #[cfg(target_os = "linux")]
    {
        scan_linux()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Vec::new()
    }
}

// ── Windows ──────────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
fn scan_windows() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
    let pf86 = std::env::var("ProgramFiles(x86)")
        .unwrap_or_else(|_| "C:\\Program Files (x86)".into());
    let la = std::env::var("LOCALAPPDATA").unwrap_or_default();

    let mut dirs = vec![pf, pf86];
    if !la.is_empty() {
        dirs.push(format!("{}\\Microsoft\\WindowsApps", la));
    }

    for dir in dirs {
        scan_dir_for_exes(std::path::Path::new(&dir), &mut apps, 2);
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.exec_path.eq_ignore_ascii_case(&b.exec_path));
    apps
}

#[cfg(target_os = "windows")]
fn scan_dir_for_exes(dir: &std::path::Path, out: &mut Vec<AppInfo>, depth: u8) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            // Skip common non-app subfolders.
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') || name == "node_modules" || name == "temp" {
                continue;
            }
            scan_dir_for_exes(&path, out, depth - 1);
        } else if path.extension().and_then(|e| e.to_str()) == Some("exe") {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            // Skip uninstallers and helpers.
            let nl = name.to_lowercase();
            if nl.contains("unins") || nl == "setup" || nl.contains("helper") {
                continue;
            }
            out.push(AppInfo {
                name,
                exec_path: path.to_string_lossy().into_owned(),
                bundle_id: None,
                version: None,
                icon_path: Some(path.to_string_lossy().into_owned()),
            });
        }
    }
}

// ── macOS ────────────────────────────────────────────────────────────────────
#[cfg(target_os = "macos")]
fn scan_macos() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    let dirs = [
        "/Applications",
        "/System/Applications",
        "/System/Applications/Utilities",
    ];
    let home = std::env::var("HOME").unwrap_or_default();
    let home_apps = format!("{}/Applications", home);

    for dir in dirs.iter().map(std::path::Path::new).chain(std::iter::once(
        std::path::Path::new(&home_apps),
    )) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("app") {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_string();
                    if name.is_empty() {
                        continue;
                    }
                    apps.push(AppInfo {
                        name,
                        exec_path: path.to_string_lossy().into_owned(),
                        bundle_id: None,
                        version: None,
                        icon_path: None,
                    });
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps.dedup_by(|a, b| a.name == b.name);
    apps
}

// ── Linux ────────────────────────────────────────────────────────────────────
#[cfg(target_os = "linux")]
fn scan_linux() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    let dirs = ["/usr/share/applications", "/usr/local/share/applications"];
    let home = std::env::var("HOME").unwrap_or_default();
    let home_apps = format!("{}/.local/share/applications", home);

    for dir in dirs.iter().map(std::path::Path::new).chain(std::iter::once(
        std::path::Path::new(&home_apps),
    )) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                    continue;
                }
                if let Ok(text) = std::fs::read_to_string(&path) {
                    let mut name = None;
                    let mut exec = None;
                    let mut icon = None;
                    let mut no_display = false;
                    for line in text.lines() {
                        if line.starts_with("Name=") {
                            name = line[5..].trim().to_string().into();
                        } else if line.starts_with("Exec=") {
                            exec = line[5..].trim().to_string().into();
                        } else if line.starts_with("Icon=") {
                            icon = line[5..].trim().to_string().into();
                        } else if line.starts_with("NoDisplay=true") {
                            no_display = true;
                        }
                    }
                    if no_display {
                        continue;
                    }
                    if let (Some(n), Some(e)) = (name, exec) {
                        // Strip %f / %u placeholders from Exec.
                        let exec_clean = e
                            .split_whitespace()
                            .next()
                            .unwrap_or(&e)
                            .to_string();
                        apps.push(AppInfo {
                            name: n,
                            exec_path: exec_clean,
                            bundle_id: None,
                            version: None,
                            icon_path: icon,
                        });
                    }
                }
            }
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    apps.dedup_by(|a, b| a.name == b.name);
    apps
}
