//! The notification icon lives only while this window is hidden.
use gpui_kit::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM},
    UI::{Shell::*, WindowsAndMessaging::*},
};
const EVENT: u32 = WM_APP + 91;
const SUBCLASS: usize = 0x5346;
static EXIT: AtomicBool = AtomicBool::new(false);
fn taskbar_created() -> u32 {
    static MSG: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *MSG.get_or_init(|| {
        let name: Vec<_> = "TaskbarCreated\0".encode_utf16().collect();
        unsafe { RegisterWindowMessageW(name.as_ptr()) }
    })
}
pub(super) fn take_exit() -> bool {
    EXIT.swap(false, Ordering::Relaxed)
}
fn icon(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: EVENT,
        ..Default::default()
    };
    // Both the application window and the system icon have a longer lifetime than the notification.
    unsafe {
        data.hIcon = SendMessageW(hwnd, WM_GETICON, ICON_SMALL as usize, 0) as HICON;
        if data.hIcon.is_null() {
            data.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }
    }
    for (slot, c) in data.szTip.iter_mut().zip("ShipForge".encode_utf16()) {
        *slot = c;
    }
    data
}
pub(super) fn hide(window: &Window) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return false;
    };
    let RawWindowHandle::Win32(raw) = handle.as_raw() else {
        return false;
    };
    let hwnd = raw.hwnd.get() as HWND;
    taskbar_created();
    unsafe {
        if SetWindowSubclass(hwnd, Some(callback), SUBCLASS, 0) == 0 {
            return false;
        }
        if Shell_NotifyIconW(NIM_ADD, &icon(hwnd)) == 0 {
            RemoveWindowSubclass(hwnd, Some(callback), SUBCLASS);
            return false;
        }
        ShowWindow(hwnd, SW_HIDE);
    }
    true
}
unsafe fn restore(hwnd: HWND) {
    unsafe {
        Shell_NotifyIconW(NIM_DELETE, &icon(hwnd));
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
}
unsafe extern "system" fn callback(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if msg == EVENT {
            match lp as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => restore(hwnd),
                WM_RBUTTONUP => {
                    let menu = CreatePopupMenu();
                    if menu.is_null() {
                        restore(hwnd);
                        return 0;
                    }
                    let open: Vec<_> = "打开 ShipForge\0".encode_utf16().collect();
                    let quit: Vec<_> = "退出 ShipForge\0".encode_utf16().collect();
                    AppendMenuW(menu, MF_STRING, 1, open.as_ptr());
                    AppendMenuW(menu, MF_STRING, 2, quit.as_ptr());
                    let mut p = POINT::default();
                    GetCursorPos(&mut p);
                    SetForegroundWindow(hwnd);
                    let choice = TrackPopupMenu(
                        menu,
                        TPM_RETURNCMD | TPM_RIGHTBUTTON,
                        p.x,
                        p.y,
                        0,
                        hwnd,
                        std::ptr::null(),
                    );
                    DestroyMenu(menu);
                    if choice == 1 || choice == 2 {
                        restore(hwnd);
                    }
                    if choice == 2 {
                        EXIT.store(true, Ordering::Relaxed);
                        PostMessageW(hwnd, WM_CLOSE, 0, 0);
                    }
                }
                _ => {}
            }
            return 0;
        }
        if msg != 0 && msg == taskbar_created() && IsWindowVisible(hwnd) == 0 {
            Shell_NotifyIconW(NIM_ADD, &icon(hwnd));
        }
        if msg == WM_NCDESTROY {
            Shell_NotifyIconW(NIM_DELETE, &icon(hwnd));
            RemoveWindowSubclass(hwnd, Some(callback), id);
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }
}
#[cfg(test)]
pub(super) fn test_hidden_icon(window: &Window) {
    assert!(
        hide(window),
        "Explorer notification area did not accept the icon"
    );
    let RawWindowHandle::Win32(raw) = HasWindowHandle::window_handle(window).unwrap().as_raw()
    else {
        panic!("expected Window HWND")
    };
    let hwnd = raw.hwnd.get() as HWND;
    unsafe {
        assert_eq!(IsWindowVisible(hwnd), 0);
        assert_ne!(Shell_NotifyIconW(NIM_DELETE, &icon(hwnd)), 0);
        RemoveWindowSubclass(hwnd, Some(callback), SUBCLASS);
    }
}
