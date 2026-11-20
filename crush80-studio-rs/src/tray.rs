//! Pure-Rust native Win32 System Tray (`Shell_NotifyIconW`) with dynamic battery hover tooltip,
//! direct Win32 `ShowWindow` hide/restore, and immediate `std::process::exit(0)` on Quit.

use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
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
    pub hwnd: AtomicIsize,
    pub main_hwnd: AtomicIsize,
    pub tooltip: Mutex<String>,
    pub menu_show: Mutex<String>,
    pub menu_quit: Mutex<String>,
}

impl TrayState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            show_requested: AtomicBool::new(false),
            hwnd: AtomicIsize::new(0),
            main_hwnd: AtomicIsize::new(0),
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
        unsafe {
            let title = to_wide_null("Crush 80 Studio");
            let mut h = FindWindowW(null(), title.as_ptr());
            if h.is_null() {
                let fg = GetForegroundWindow();
                if !fg.is_null() && fg as isize != self.hwnd.load(Ordering::Relaxed) {
                    let mut pid: u32 = 0;
                    GetWindowThreadProcessId(fg, &mut pid);
                    if pid == GetCurrentProcessId() {
                        h = fg;
                    }
                }
            }
            if !h.is_null() {
                self.main_hwnd.store(h as isize, Ordering::SeqCst);
            }
        }
    }

    pub fn hide_main_window(&self) {
        self.capture_main_hwnd();
        let mh = self.main_hwnd.load(Ordering::Relaxed);
        if mh != 0 {
            unsafe {
                ShowWindow(mh as HWND, SW_HIDE);
            }
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
        self.show_requested.store(true, Ordering::SeqCst);
    }

    pub fn update_tooltip(&self, tip: String) {
        {
            let mut guard = self.tooltip.lock().unwrap();
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
        *self.menu_show.lock().unwrap() = show.to_string();
        *self.menu_quit.lock().unwrap() = quit.to_string();
    }
}

static GLOBAL_TRAY: Mutex<Option<Arc<TrayState>>> = Mutex::new(None);

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn fill_tip(dst: &mut [u16; 128], text: &str) {
    dst.fill(0);
    for (i, ch) in text.encode_utf16().take(127).enumerate() {
        dst[i] = ch;
    }
}

unsafe extern "system" fn tray_wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY_CALLBACK => {
            let ev = (lparam as u32) & 0xFFFF;
            if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                if let Some(st) = GLOBAL_TRAY.lock().unwrap().as_ref() {
                    st.restore_main_window();
                }
            } else if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                let (lbl_show, lbl_quit) = {
                    if let Some(st) = GLOBAL_TRAY.lock().unwrap().as_ref() {
                        (st.menu_show.lock().unwrap().clone(), st.menu_quit.lock().unwrap().clone())
                    } else {
                        ("Show".into(), "Quit".into())
                    }
                };
                let hmenu = CreatePopupMenu();
                if !hmenu.is_null() {
                    let w_show = to_wide_null(&lbl_show);
                    let w_quit = to_wide_null(&lbl_quit);
                    AppendMenuW(hmenu, MF_STRING, CMD_SHOW, w_show.as_ptr());
                    AppendMenuW(hmenu, MF_SEPARATOR, 0, null());
                    AppendMenuW(hmenu, MF_STRING, CMD_QUIT, w_quit.as_ptr());

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
                    DestroyMenu(hmenu);

                    if cmd == CMD_SHOW {
                        if let Some(st) = GLOBAL_TRAY.lock().unwrap().as_ref() {
                            st.restore_main_window();
                        }
                    } else if cmd == CMD_QUIT {
                        // Delete tray icon and exit process immediately (works even when main window is hidden!)
                        let mut nid: NOTIFYICONDATAW = zeroed();
                        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
                        nid.hWnd = hwnd;
                        nid.uID = TRAY_UID;
                        Shell_NotifyIconW(NIM_DELETE, &nid);
                        std::process::exit(0);
                    }
                }
            }
            0
        }
        WM_TRAY_UPDATE_TIP => {
            let tip = GLOBAL_TRAY
                .lock()
                .unwrap()
                .as_ref()
                .map(|st| st.tooltip.lock().unwrap().clone())
                .unwrap_or_else(|| "Crush 80 Studio".into());
            let mut nid: NOTIFYICONDATAW = zeroed();
            nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            nid.hWnd = hwnd;
            nid.uID = TRAY_UID;
            nid.uFlags = NIF_TIP;
            fill_tip(&mut nid.szTip, &tip);
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
    *GLOBAL_TRAY.lock().unwrap() = Some(state.clone());
    thread::spawn(move || unsafe {
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

        let mut hicon = LoadIconW(hinst, 1 as *const u16);
        if hicon.is_null() {
            hicon = LoadIconW(null_mut(), IDI_APPLICATION);
        }

        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = TRAY_UID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY_CALLBACK;
        nid.hIcon = hicon;
        let tip = state.tooltip.lock().unwrap().clone();
        fill_tip(&mut nid.szTip, &tip);
        Shell_NotifyIconW(NIM_ADD, &nid);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Hides the main `"Crush 80 Studio"` window to the system tray.
/// Works both after `spawn_tray` and early during `--minimized` startup.
pub fn hide_main_window() {
    if let Some(st) = GLOBAL_TRAY.lock().unwrap().as_ref() {
        st.hide_main_window();
        return;
    }
    unsafe {
        let title = to_wide_null("Crush 80 Studio");
        let h = FindWindowW(null(), title.as_ptr());
        if !h.is_null() {
            ShowWindow(h, SW_HIDE);
        }
    }
}
