// ─────────────────────────────────────────────────────────────────────────────
// Platform-specific helpers: get the frontmost application + clipboard files.
//
// On macOS this would use NSWorkspace.frontmostApplication.bundleIdentifier.
// On Windows we use GetForegroundWindow + GetWindowThreadProcessId +
// QueryFullProcessImageNameW.
// On Linux we return `unknown` — getting the active window requires talking to
// X11 or the Wayland compositor, and is out of scope for v1.
// ─────────────────────────────────────────────────────────────────────────────

pub struct FrontmostApp {
    /// A stable identifier — bundle id on macOS, full exe path on Windows.
    pub app_id: Option<String>,
    /// A friendly display name — e.g. "Safari", "chrome.exe".
    pub app_name: Option<String>,
}

/// Read file paths from the clipboard (when the user copied files in Explorer).
/// Returns the list of copied file paths, or `None` if the clipboard doesn't
/// contain files.
pub fn get_clipboard_files() -> Option<Vec<String>> {
    #[cfg(target_os = "windows")]
    {
        get_clipboard_files_windows()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Write file paths to the clipboard (so the user can paste them in Explorer).
pub fn set_clipboard_files(files: &[String]) -> bool {
    #[cfg(target_os = "windows")]
    {
        set_clipboard_files_windows(files)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = files;
        false
    }
}

/// Set image (PNG bytes) on the clipboard via Win32 API.
/// This avoids arboard's clipboard-open conflict with our watcher thread.
pub fn set_clipboard_image(png_bytes: &[u8]) -> bool {
    #[cfg(target_os = "windows")]
    {
        set_clipboard_image_windows(png_bytes)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = png_bytes;
        false
    }
}

// ── Windows: read files via CF_HDROP ─────────────────────────────────────────
#[cfg(target_os = "windows")]
fn get_clipboard_files_windows() -> Option<Vec<String>> {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, OpenClipboard,
    };
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DragQueryFileW;
    use windows::Win32::Foundation::HWND;

    // SAFETY: OpenClipboard with null HWND opens the clipboard for the current
    // task. We must close it with CloseClipboard before returning.
    let Ok(()) = (unsafe { OpenClipboard(HWND::default()) }) else {
        return None;
    };

    struct ClipGuard;
    impl Drop for ClipGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    let _guard = ClipGuard;

    // CF_HDROP is a u16 clipboard format value; GetClipboardData wants u32.
    let cf_hdrop: u32 = CF_HDROP.0 as u32;

    let handle = unsafe { GetClipboardData(cf_hdrop) }.ok()?;
    if handle.0.is_null() {
        return None;
    }

    // DragQueryFileW expects an HDROP, which wraps a `*mut c_void` in
    // windows-rs 0.58. We extract the raw pointer from the HANDLE and wrap it.
    use windows::Win32::UI::Shell::HDROP;
    let hdrop = HDROP(handle.0);

    // DragQueryFileW with index 0xFFFFFFFF returns the count of files.
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFFFFFF, None) };
    if count == 0 {
        return None;
    }

    let mut files = Vec::with_capacity(count as usize);
    for i in 0..count {
        // Get the file path length first.
        let len = unsafe { DragQueryFileW(hdrop, i, None) };
        if len == 0 {
            continue;
        }
        // Allocate a buffer and get the path. windows-rs 0.58 DragQueryFileW
        // takes `Option<&mut [u16]>` — we pass a mutable slice.
        let mut buf = vec![0u16; (len as usize) + 1];
        let written = unsafe {
            DragQueryFileW(hdrop, i, Some(&mut buf))
        };
        if written > 0 {
            // Truncate to the actual number of chars written.
            buf.truncate(written as usize);
            let path = String::from_utf16_lossy(&buf);
            files.push(path);
        }
    }

    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

// ── Windows: write files via CF_HDROP ────────────────────────────────────────
#[cfg(target_os = "windows")]
fn set_clipboard_files_windows(files: &[String]) -> bool {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::Foundation::HWND;

    if files.is_empty() {
        return false;
    }

    // Build a DROPFILES structure (20 bytes) followed by UTF-16 file paths,
    // each null-terminated, with an extra null at the end of the list.
    let mut wide_paths: Vec<u16> = Vec::new();
    for f in files {
        let mut wide: Vec<u16> = f.encode_utf16().collect();
        wide.push(0); // null terminator per path
        wide_paths.extend(wide);
    }
    wide_paths.push(0); // extra null = end of list

    let dropfiles_size = 20usize; // sizeof(DROPFILES)
    let total_size = dropfiles_size + wide_paths.len() * 2;

    let Ok(()) = (unsafe { OpenClipboard(HWND::default()) }) else {
        return false;
    };

    struct ClipGuard;
    impl Drop for ClipGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    let _guard = ClipGuard;

    let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, total_size) };
    let hmem = match hmem {
        Ok(h) => h,
        Err(_) => return false,
    };

    let ptr = unsafe { GlobalLock(hmem) };
    if ptr.is_null() {
        // GlobalFree not available in this windows-rs version; the OS reclaims
        // the memory when the clipboard is emptied or the process exits.
        return false;
    }

    // DROPFILES header: pFiles=20, pt=(0,0), fNC=0, fWide=1
    let dropfiles: [u8; 20] = [
        20, 0, 0, 0, // pFiles = 20 (offset to file list)
        0, 0, 0, 0, // pt.x
        0, 0, 0, 0, // pt.y
        0, 0, 0, 0, // fNC = 0
        1, 0, 0, 0, // fWide = 1 (UTF-16)
    ];
    unsafe {
        std::ptr::copy_nonoverlapping(dropfiles.as_ptr(), ptr as *mut u8, 20);
        let path_ptr = (ptr as *mut u8).add(20) as *mut u16;
        std::ptr::copy_nonoverlapping(wide_paths.as_ptr(), path_ptr, wide_paths.len());
    }
    let _ = unsafe { GlobalUnlock(hmem) };

    let _ = unsafe { EmptyClipboard() };
    let cf_hdrop: u32 = CF_HDROP.0 as u32;
    // SetClipboardData wants a HANDLE, not HGLOBAL. Convert via .0 (the raw
    // pointer) — both are transparent wrappers around `*mut c_void`.
    use windows::Win32::Foundation::HANDLE;
    let handle_arg = HANDLE(hmem.0);
    let result = unsafe { SetClipboardData(cf_hdrop, handle_arg) };
    result.is_ok()
}

// ── Windows: set image via Win32 API (CF_DIB) ────────────────────────────────
#[cfg(target_os = "windows")]
fn set_clipboard_image_windows(png_bytes: &[u8]) -> bool {
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard,
    };

    // Decode PNG → RGBA → CF_DIB
    let img = match image::load_from_memory(png_bytes) {
        Ok(img) => img,
        Err(e) => {
            tracing::warn!(error = ?e, "failed to decode PNG for clipboard");
            return false;
        }
    };
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let dib = build_dib_from_rgba(&rgba, w, h);
    if dib.is_empty() {
        return false;
    }

    let Ok(()) = (unsafe { OpenClipboard(windows::Win32::Foundation::HWND::default()) }) else {
        return false;
    };

    struct ClipGuard;
    impl Drop for ClipGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }
    let _guard = ClipGuard;

    let _ = unsafe { EmptyClipboard() };

    // CF_DIB = 8 (standard Windows clipboard format for DIB)
    let cf_dib: u32 = 8;
    set_data_on_clipboard(cf_dib, &dib)
}

/// Build a CF_DIB (BITMAPINFO + pixel data) from RGBA pixels.
/// The DIB uses 32-bit BGRA format (BI_BITFIELDS).
#[cfg(target_os = "windows")]
fn build_dib_from_rgba(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    // BITMAPINFOHEADER (40 bytes)
    let header_size: u32 = 40;
    let bpp: u16 = 32;
    let row_size = ((width as usize * 4 + 3) & !3) as usize; // 4-byte aligned
    let image_size = row_size * height as usize;
    let total_size = header_size as usize + image_size;

    let mut out = Vec::with_capacity(total_size);

    // BITMAPINFOHEADER
    out.extend_from_slice(&header_size.to_le_bytes());   // biSize
    out.extend_from_slice(&(width as i32).to_le_bytes()); // biWidth
    out.extend_from_slice(&(height as i32).to_le_bytes());// biHeight (positive = bottom-up)
    out.extend_from_slice(&1u16.to_le_bytes());           // biPlanes
    out.extend_from_slice(&bpp.to_le_bytes());            // biBitCount
    out.extend_from_slice(&0u32.to_le_bytes());           // biCompression = BI_RGB
    out.extend_from_slice(&(image_size as u32).to_le_bytes()); // biSizeImage
    out.extend_from_slice(&0i32.to_le_bytes());           // biXPelsPerMeter
    out.extend_from_slice(&0i32.to_le_bytes());           // biYPelsPerMeter
    out.extend_from_slice(&0u32.to_le_bytes());           // biClrUsed
    out.extend_from_slice(&0u32.to_le_bytes());           // biClrImportant

    // Pixel data — RGBA → BGRA, bottom-up rows
    for y in (0..height).rev() {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let r = rgba[i];
            let g = rgba[i + 1];
            let b = rgba[i + 2];
            let a = rgba[i + 3];
            out.push(b); // B
            out.push(g); // G
            out.push(r); // R
            out.push(a); // A
        }
        // Pad row to 4-byte boundary
        let padding = row_size - (width as usize * 4);
        for _ in 0..padding {
            out.push(0);
        }
    }

    out
}

/// Helper: allocate a global memory block, copy data, set on clipboard.
#[cfg(target_os = "windows")]
fn set_data_on_clipboard(format: u32, data: &[u8]) -> bool {
    use windows::Win32::System::DataExchange::SetClipboardData;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::Foundation::HANDLE;

    let hmem = unsafe { GlobalAlloc(GMEM_MOVEABLE, data.len()) };
    let hmem = match hmem {
        Ok(h) => h,
        Err(_) => return false,
    };

    let ptr = unsafe { GlobalLock(hmem) };
    if ptr.is_null() {
        return false;
    }

    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());
    }
    let _ = unsafe { GlobalUnlock(hmem) };

    let handle_arg = HANDLE(hmem.0);
    let result = unsafe { SetClipboardData(format, handle_arg) };
    result.is_ok()
}

pub fn frontmost_app() -> FrontmostApp {
    #[cfg(target_os = "windows")]
    {
        frontmost_app_windows()
    }
    #[cfg(target_os = "macos")]
    {
        frontmost_app_macos()
    }
    #[cfg(target_os = "linux")]
    {
        FrontmostApp {
            app_id: None,
            app_name: None,
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        FrontmostApp {
            app_id: None,
            app_name: None,
        }
    }
}

// ── Windows ──────────────────────────────────────────────────────────────────
#[cfg(target_os = "windows")]
fn frontmost_app_windows() -> FrontmostApp {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

    // SAFETY: GetForegroundWindow has no invariants; returns null if no fg window.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return FrontmostApp {
            app_id: None,
            app_name: None,
        };
    }

    // GetWindowThreadProcessId in windows-rs 0.58 takes `Option<*mut u32>`.
    let mut pid: u32 = 0;
    let pid_ptr: *mut u32 = &mut pid;
    unsafe { GetWindowThreadProcessId(hwnd, Some(pid_ptr)) };
    if pid == 0 {
        return FrontmostApp {
            app_id: None,
            app_name: None,
        };
    }

    let exe_path = process_exe_path_windows(pid);
    let app_name = exe_path.as_ref().and_then(|p| {
        std::path::Path::new(p)
            .file_name()
            .and_then(|f| f.to_str())
            .map(|s| s.to_string())
    });

    FrontmostApp {
        app_id: exe_path,
        app_name,
    }
}

#[cfg(target_os = "windows")]
fn process_exe_path_windows(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::core::PWSTR;

    // SAFETY: opening a foreign process for query-only info is safe.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };

    // RAII guard to ensure CloseHandle is called even on early return.
    struct CloseGuard(windows::Win32::Foundation::HANDLE);
    impl Drop for CloseGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    let _guard = CloseGuard(handle);

    let mut buf = [0u16; 1024];
    let mut len: u32 = buf.len() as u32;

    // windows-rs 0.58+: QueryFullProcessImageNameW returns `Result<()>` and
    // the second parameter is `PROCESS_NAME_FORMAT` (a struct wrapper around
    // u32), not a raw `BOOL`.
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    if result.is_err() {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

// ── macOS ────────────────────────────────────────────────────────────────────
#[cfg(target_os = "macos")]
fn frontmost_app_macos() -> FrontmostApp {
    // Minimal macOS implementation using the objc runtime. We invoke
    // NSWorkspace.sharedWorkspace.frontmostApplication and read
    // bundleIdentifier + localizedName.
    use objc::class;
    use objc::msg_send;
    use objc::sel;
    use objc::sel_impl;

    unsafe {
        let workspace: *mut objc::runtime::Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return FrontmostApp {
                app_id: None,
                app_name: None,
            };
        }
        let app: *mut objc::runtime::Object = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return FrontmostApp {
                app_id: None,
                app_name: None,
            };
        }
        let bundle_id: *mut objc::runtime::Object = msg_send![app, bundleIdentifier];
        let name: *mut objc::runtime::Object = msg_send![app, localizedName];

        let app_id = nsstring_to_string(bundle_id);
        let app_name = nsstring_to_string(name);
        FrontmostApp { app_id, app_name }
    }
}

#[cfg(target_os = "macos")]
unsafe fn nsstring_to_string(obj: *mut objc::runtime::Object) -> Option<String> {
    if obj.is_null() {
        return None;
    }
    use objc::msg_send;
    use objc::sel;
    use objc::sel_impl;
    let bytes: *const std::os::raw::c_char = msg_send![obj, UTF8String];
    if bytes.is_null() {
        return None;
    }
    let cstr = std::ffi::CStr::from_ptr(bytes);
    cstr.to_str().ok().map(|s| s.to_string())
}
