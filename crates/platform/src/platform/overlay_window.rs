#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, IsIconic, SetWindowPos, ShowWindow,
    GWL_EXSTYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SW_RESTORE, SW_SHOWNOACTIVATE, WS_EX_TOPMOST, WS_EX_TOOLWINDOW,
};
#[cfg(windows)]
use windows_sys::Win32::Foundation::{HWND, RECT};

pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// The usable desktop area of the primary monitor (the screen minus the taskbar), in physical pixels: (x, y, w, h).
pub fn work_area() -> Option<(i32, i32, i32, i32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA};
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        // SAFETY: SPI_GETWORKAREA writes one RECT through the pointer.
        let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut r as *mut RECT as *mut std::ffi::c_void, 0) };
        (ok != 0).then_some((r.left, r.top, r.right - r.left, r.bottom - r.top))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// The usable area (monitor minus taskbar) of the monitor that holds the physical-pixel rectangle `x, y, w, h` - or, if
/// it is on none of them, the nearest one - as (x, y, w, h). This is what keeps a window on the screen it is actually on
/// instead of the primary one.
pub fn work_area_near(x: i32, y: i32, w: i32, h: i32) -> Option<(i32, i32, i32, i32)> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST};
        let zero = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let probe = RECT { left: x, top: y, right: x + w.max(1), bottom: y + h.max(1) };
        // SAFETY: `probe` outlives the call; MONITOR_DEFAULTTONEAREST always yields a monitor when any is attached.
        let monitor = unsafe { MonitorFromRect(&probe, MONITOR_DEFAULTTONEAREST) };
        if monitor.is_null() {
            return None;
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, rcMonitor: zero, rcWork: zero, dwFlags: 0 };
        // SAFETY: `info.cbSize` is set as the API requires, and the pointer is to a live MONITORINFO.
        let ok = unsafe { GetMonitorInfoW(monitor, &mut info) };
        (ok != 0).then_some((info.rcWork.left, info.rcWork.top, info.rcWork.right - info.rcWork.left, info.rcWork.bottom - info.rcWork.top))
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y, w, h);
        None
    }
}

pub fn show_no_activate(hwnd: *mut std::ffi::c_void) {
    #[cfg(windows)]
    {
        if !hwnd.is_null() {
            unsafe {
                ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE);
            }
        }
    }
}

/// Brings a hidden or minimised window back by talking to Windows directly. A hidden window does not run its own
/// frame loop, so a request queued inside the app would never be processed: this is what makes "Show Overlay" reliable.
pub fn restore_visible(hwnd: *mut std::ffi::c_void) {
    #[cfg(windows)]
    {
        if !hwnd.is_null() {
            unsafe {
                if IsIconic(hwnd as HWND) != 0 {
                    ShowWindow(hwnd as HWND, SW_RESTORE);
                } else {
                    ShowWindow(hwnd as HWND, SW_SHOWNOACTIVATE);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
    }
}

pub fn set_topmost(hwnd: *mut std::ffi::c_void, on: bool) {
    #[cfg(windows)]
    {
        if !hwnd.is_null() {
            unsafe {
                SetWindowPos(
                    hwnd as HWND,
                    if on { HWND_TOPMOST } else { HWND_NOTOPMOST },
                    0, 0, 0, 0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
    }
}

pub fn is_topmost(hwnd: *mut std::ffi::c_void) -> Option<bool> {
    #[cfg(windows)]
    {
        if hwnd.is_null() { return None; }
        let style = unsafe { GetWindowLongPtrW(hwnd as HWND, GWL_EXSTYLE) } as u32;
        Some(style & WS_EX_TOPMOST != 0)
    }
    #[cfg(not(windows))]
    { None }
}

pub fn set_toolwindow(hwnd: *mut std::ffi::c_void, on: bool) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
        if !hwnd.is_null() {
            let h = hwnd as HWND;
            let mut style = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) };
            if on {
                style |= WS_EX_TOOLWINDOW as isize;
            } else {
                style &= !(WS_EX_TOOLWINDOW as isize);
            }
            unsafe {
                SetWindowLongPtrW(h, GWL_EXSTYLE, style);
            }
        }
    }
}

pub fn get_window_rect(hwnd: *mut std::ffi::c_void) -> Option<WindowRect> {
    #[cfg(windows)]
    {
        if hwnd.is_null() { return None; }
        let mut rect: RECT = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let ok = unsafe { GetWindowRect(hwnd as HWND, &mut rect) };
        if ok != 0 {
            Some(WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
            })
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    { None }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_work_area_of_the_monitor_near_a_rectangle_is_a_real_area() {
        // Needs an attached monitor, like every Windows desktop session this suite runs in.
        let Some((_, _, w, h)) = work_area_near(10, 10, 200, 200) else { return };
        assert!(w > 200 && h > 200, "{w} x {h}");
        // A rectangle far outside every screen still resolves to the nearest monitor instead of failing.
        let far = work_area_near(900_000, 900_000, 200, 200).expect("nearest monitor");
        assert!(far.2 > 200 && far.3 > 200);
    }
}
