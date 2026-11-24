//! Pure-Rust native Win32 System Tray (`Shell_NotifyIconW`) with dynamic battery hover tooltip,
//! PID-verified window hide/restore, `TaskbarCreated` recovery, and OTA-safe Quit guard.

use crate::hid_win::Crush80Hid;
use crate::win_sys::{copy_utf16_truncated, find_own_main_window};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY_CALLBACK: u32 = WM_USER + 42;
const WM_TRAY_UPDATE_TIP: u32 = WM_USER + 43;
const WM_TRAY_DESTROY: u32 = WM_USER + 44;
const CMD_SHOW: usize = 1001;
const CMD_QUIT: usize = 1002;
const TRAY_UID: u32 = 1;

pub struct TrayState {
    pub show_requested: AtomicBool,
    pub window_hidden: Arc<AtomicBool>,
    pub hwnd: AtomicIsize,
    pub main_hwnd: AtomicIsize,
    hicon: AtomicIsize,
    pub hid: Arc<Crush80Hid>,
    pub tooltip: Mutex<String>,
    pub menu_show: Mutex<String>,
    pub menu_quit: Mutex<String>,
}

impl TrayState {
    pub fn new(hid: Arc<Crush80Hid>) -> Arc<Self> {
        Arc::new(Self {
            show_requested: AtomicBool::new(false),
            window_hidden: Arc::new(AtomicBool::new(false)),
            hwnd: AtomicIsize::new(0),
            main_hwnd: AtomicIsize::new(0),
            hicon: AtomicIsize::new(0),
            hid,
            tooltip: Mutex::new("Crush 80 Studio".into()),
            menu_show: Mutex::new("显示主窗口".into()),
            menu_quit: Mutex::new("彻底退出".into()),
        })
    }

    pub fn capture_main_hwnd(&self) {
        let existing = self.main_hwnd.load(Ordering::Relaxed);
        if existing != 0 && unsafe { IsWindow(existing as HWND) != 0 } {
            return;
        }
        let tray_hwnd = self.hwnd.load(Ordering::Relaxed) as HWND;
        let h = find_own_main_window("Crush 80 Studio", tray_hwnd);
        if !h.is_null() {
            self.main_hwnd.store(h as isize, Ordering::SeqCst);
        }
    }

    pub fn hide_main_window(&self) {
        self.capture_main_hwnd();
        let mh = self.main_hwnd.load(Ordering::Relaxed);
        if mh != 0 {
            unsafe {
                ShowWindow(mh as HWND, SW_HIDE);
            }
            self.window_hidden.store(true, Ordering::SeqCst);
        }
    }

    pub fn restore_main_window(&self) {
        self.capture_main_hwnd();
        let mh = self.main_hwnd.load(Ordering::Relaxed);
        if mh != 0 {
            unsafe {
                ShowWindow(mh as HWND, SW_SHOW);
                if IsIconic(mh as HWND) != 0 {
                    ShowWindow(mh as HWND, SW_RESTORE);
                }
                SetForegroundWindow(mh as HWND);
            }
        }
        self.window_hidden.store(false, Ordering::SeqCst);
        self.show_requested.store(true, Ordering::SeqCst);
    }

    pub fn update_tooltip(&self, tip: String) {
        if let Ok(mut guard) = self.tooltip.lock() {
            if *guard == tip {
                return;
            }
            *guard = tip;
        }
        let h = self.hwnd.load(Ordering::Relaxed);
        if h != 0 {
            unsafe {
                PostMessageW(h as HWND, WM_TRAY_UPDATE_TIP, 0, 0);
            }
        }
    }

    pub fn update_menu_labels(&self, show: &str, quit: &str) {
        if let Ok(mut g) = self.menu_show.lock() {
            *g = show.to_string();
        }
        if let Ok(mut g) = self.menu_quit.lock() {
            *g = quit.to_string();
        }
    }
}

static GLOBAL_TRAY: Mutex<Option<Arc<TrayState>>> = Mutex::new(None);
static WM_TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn add_tray_icon(hwnd: HWND, hicon: HICON, tip: &str) {
    let mut nid: NOTIFYICONDATAW = zeroed();
    nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY_CALLBACK;
    nid.hIcon = hicon;
    copy_utf16_truncated(&mut nid.szTip, tip);
    Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe extern "system" fn tray_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let tb_msg = WM_TASKBAR_CREATED.load(Ordering::Relaxed);
    if tb_msg != 0 && msg == tb_msg {
        if let Ok(guard) = GLOBAL_TRAY.lock() {
            if let Some(st) = guard.as_ref() {
                let hicon = st.hicon.load(Ordering::Relaxed) as HICON;
                let tip = st
                    .tooltip
                    .lock()
                    .map(|g| g.clone())
                    .unwrap_or_else(|_| "Crush 80 Studio".into());
                add_tray_icon(hwnd, hicon, &tip);
            }
        }
        return 0;
    }

    match msg {
        WM_TRAY_CALLBACK => {
            let ev = (lparam as u32) & 0xFFFF;
            if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                if let Ok(guard) = GLOBAL_TRAY.lock() {
                    if let Some(st) = guard.as_ref() {
                        st.restore_main_window();
                    }
                }
            } else if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                let (lbl_show, lbl_quit, ota_busy) = GLOBAL_TRAY
                    .lock()
                    .ok()
                    .and_then(|g| {
                        g.as_ref().map(|st| {
                            let s = st
                                .menu_show
                                .lock()
                                .map(|m| m.clone())
                                .unwrap_or_else(|_| "Show".into());
                            let q = st
                                .menu_quit
                                .lock()
                                .map(|m| m.clone())
                                .unwrap_or_else(|_| "Quit".into());
                            (s, q, st.hid.is_ota_busy())
                        })
                    })
                    .unwrap_or_else(|| ("Show".into(), "Quit".into(), false));

                let hmenu = CreatePopupMenu();
                if !hmenu.is_null() {
                    let w_show = to_wide_null(&lbl_show);
                    let w_quit = to_wide_null(&lbl_quit);
                    AppendMenuW(hmenu, MF_STRING, CMD_SHOW, w_show.as_ptr());
                    AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
                    let quit_flags = if ota_busy {
                        MF_STRING | MF_GRAYED
                    } else {
                        MF_STRING
                    };
                    AppendMenuW(hmenu, quit_flags, CMD_QUIT, w_quit.as_ptr());

                    let mut pt: POINT = zeroed();
                    GetCursorPos(&mut pt);
                    SetForegroundWindow(hwnd);
                    let cmd = TrackPopupMenu(
                        hmenu,
                        TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN | TPM_LEFTALIGN,
                        pt.x,
                        pt.y,
                        0,
                        hwnd,
                        null(),
                    ) as usize;
                    // Required by Win32 Shell docs so next context-menu click dismisses cleanly.
                    PostMessageW(hwnd, WM_NULL, 0, 0);
                    DestroyMenu(hmenu);

                    if cmd == CMD_SHOW {
                        if let Ok(guard) = GLOBAL_TRAY.lock() {
                            if let Some(st) = guard.as_ref() {
                                st.restore_main_window();
                            }
                        }
                    } else if cmd == CMD_QUIT {
                        let still_ota = GLOBAL_TRAY
                            .lock()
                            .ok()
                            .and_then(|g| g.as_ref().map(|st| st.hid.is_ota_busy()))
                            .unwrap_or(false);
                        if still_ota {
                            let title = to_wide_null("Crush 80 Studio");
                            let text = to_wide_null(
                                "OTA 固件刷写进行中，请等待刷写完成后再退出，切勿强行中断！",
                            );
                            MessageBoxW(hwnd, text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONWARNING);
                        } else {
                            let mut nid: NOTIFYICONDATAW = zeroed();
                            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                            nid.hWnd = hwnd;
                            nid.uID = TRAY_UID;
                            Shell_NotifyIconW(NIM_DELETE, &nid);
                            std::process::exit(0);
                        }
                    }
                }
            }
            0
        }
        WM_TRAY_UPDATE_TIP => {
            let tip = GLOBAL_TRAY
                .lock()
                .ok()
                .and_then(|g| g.as_ref().and_then(|st| st.tooltip.lock().ok().map(|t| t.clone())))
                .unwrap_or_else(|| "Crush 80 Studio".into());
            let mut nid: NOTIFYICONDATAW = zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = TRAY_UID;
            nid.uFlags = NIF_TIP;
            copy_utf16_truncated(&mut nid.szTip, &tip);
            Shell_NotifyIconW(NIM_MODIFY, &nid);
            0
        }
        WM_TRAY_DESTROY => {
            let mut nid: NOTIFYICONDATAW = zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = TRAY_UID;
            Shell_NotifyIconW(NIM_DELETE, &nid);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

pub fn spawn_tray(state: Arc<TrayState>) {
    if let Ok(mut g) = GLOBAL_TRAY.lock() {
        *g = Some(state.clone());
    }
    thread::spawn(move || unsafe {
        let tb_name = to_wide_null("TaskbarCreated");
        WM_TASKBAR_CREATED.store(RegisterWindowMessageW(tb_name.as_ptr()), Ordering::SeqCst);

        let hinst = GetModuleHandleW(null());
        let cls_name = to_wide_null("Crush80StudioTrayClass");

        let mut wc: WNDCLASSW = zeroed();
        wc.lpfnWndProc = Some(tray_wnd_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = cls_name.as_ptr();
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            0,
            cls_name.as_ptr(),
            cls_name.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        state.hwnd.store(hwnd as isize, Ordering::SeqCst);

        #[allow(clippy::manual_dangling_ptr)]
        let mut hicon = LoadIconW(hinst, 1 as *const u16);
        if hicon.is_null() {
            hicon = LoadIconW(null_mut(), IDI_APPLICATION);
        }
        state.hicon.store(hicon as isize, Ordering::SeqCst);

        let tip = state
            .tooltip
            .lock()
            .map(|g| g.clone())
            .unwrap_or_else(|_| "Crush 80 Studio".into());
        add_tray_icon(hwnd, hicon, &tip);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Hides the current process's `"Crush 80 Studio"` window to the system tray.
pub fn hide_main_window() {
    if let Ok(guard) = GLOBAL_TRAY.lock() {
        if let Some(st) = guard.as_ref() {
            st.hide_main_window();
            return;
        }
    }
    let h = find_own_main_window("Crush 80 Studio", null_mut());
    if !h.is_null() {
        unsafe {
            ShowWindow(h, SW_HIDE);
        }
    }
}
