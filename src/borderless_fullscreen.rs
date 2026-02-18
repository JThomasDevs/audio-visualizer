//! Borderless windowed "fullscreen" (no exclusive fullscreen mode switch).
//! On Windows we remove window decorations and size to the primary monitor.

#[cfg(windows)]
pub use windows_impl::*;

#[cfg(not(windows))]
pub use fallback::*;

#[cfg(windows)]
mod windows_impl {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetSystemMetrics, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SM_CXSCREEN,
        SM_CYSCREEN, SWP_FRAMECHANGED, SWP_NOZORDER, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_VISIBLE,
    };

    const WINDOW_TITLE: &str = "Audio Visualizer";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn find_our_window() -> Option<HWND> {
        let title = wide(WINDOW_TITLE);
        let hwnd = unsafe {
            FindWindowW(
                PCWSTR::null(),
                PCWSTR::from_raw(title.as_ptr()),
            )
        };
        if hwnd.0 == 0 {
            None
        } else {
            Some(hwnd)
        }
    }

    /// Primary monitor size in pixels (work area or full screen).
    pub fn primary_monitor_size() -> (i32, i32) {
        unsafe {
            let w = GetSystemMetrics(SM_CXSCREEN);
            let h = GetSystemMetrics(SM_CYSCREEN);
            (w, h)
        }
    }

    /// Make the app window borderless and size it to the primary monitor. Returns saved (x, y, w, h) for restore.
    pub fn enter_borderless_fullscreen(saved_pos: (i32, i32), saved_size: (i32, i32)) -> Option<((i32, i32), (i32, i32))> {
        let hwnd = find_our_window()?;
        let (mon_w, mon_h) = primary_monitor_size();
        unsafe {
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_POPUP.0 | WS_VISIBLE.0) as isize);
            let _ = SetWindowPos(
                hwnd,
                HWND(0),
                0,
                0,
                mon_w,
                mon_h,
                SWP_FRAMECHANGED | SWP_NOZORDER,
            );
        }
        Some((saved_pos, saved_size))
    }

    /// Restore window decorations and size/position.
    pub fn exit_borderless_fullscreen(restore_pos: (i32, i32), restore_size: (i32, i32)) -> bool {
        let hwnd = match find_our_window() {
            Some(h) => h,
            None => return false,
        };
        unsafe {
            let _ = SetWindowLongPtrW(hwnd, GWL_STYLE, (WS_OVERLAPPEDWINDOW.0 | WS_VISIBLE.0) as isize);
            let _ = SetWindowPos(
                hwnd,
                HWND(0),
                restore_pos.0,
                restore_pos.1,
                restore_size.0,
                restore_size.1,
                SWP_FRAMECHANGED | SWP_NOZORDER,
            );
        }
        true
    }
}

#[cfg(not(windows))]
mod fallback {
    /// Non-Windows: no-op for "get monitor size" (caller uses requested size).
    pub fn primary_monitor_size() -> (i32, i32) {
        (0, 0)
    }

    /// Non-Windows: no native borderless; caller uses miniquad size/position only.
    pub fn enter_borderless_fullscreen(_saved_pos: (i32, i32), _saved_size: (i32, i32)) -> Option<((i32, i32), (i32, i32))> {
        None
    }

    pub fn exit_borderless_fullscreen(_restore_pos: (i32, i32), _restore_size: (i32, i32)) -> bool {
        false
    }
}
