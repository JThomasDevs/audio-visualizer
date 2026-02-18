//! Stub for focus restore when leaving fullscreen. Replace with real impl if needed.

#[cfg(windows)]
pub fn restore_foreground(_hwnd: Option<isize>) {}

#[cfg(windows)]
pub fn update_last_foreign(prev: Option<isize>) -> Option<isize> {
    prev
}
