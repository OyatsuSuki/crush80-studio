//! Native Windows system integration:
//! - Single-instance mutex guard (`CreateMutexW`) with existing-window restore
//! - Windows auto-start registry helper (`HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)
//! - Startup `--minimized` argument detection
//! - Native Win32 Open/Save file dialogs (`comdlg32.dll`)

use std::ffi::{c_void, OsString};
use std::mem::{size_of, zeroed};
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, BOOL, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, HANDLE, HINSTANCE,
    HWND, LPARAM,
};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentProcessId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowW, GetWindowTextW, GetWindowThreadProcessId, SetForegroundWindow,
    ShowWindow, SW_RESTORE, SW_SHOW,
};

const APP_WINDOW_TITLE: &str = "Crush 80 Studio";
const SINGLE_INSTANCE_MUTEX_NAME: &str = "Local\\Crush80Studio_SingleInstance_v1";
const RUN_REG_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_REG_VALUE_NAME: &str = "Crush80Studio";

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `text` into `dst` as NUL-terminated UTF-16 without ever splitting a UTF-16 surrogate pair.
pub fn copy_utf16_truncated(dst: &mut [u16], text: &str) {
    dst.fill(0);
    if dst.len() <= 1 {
        return;
    }
    let max_units = dst.len() - 1;
    let mut pos = 0usize;
    let mut tmp = [0u16; 2];
    for ch in text.chars() {
        let encoded = ch.encode_utf16(&mut tmp);
        if pos + encoded.len() > max_units {
            break;
        }
        dst[pos..pos + encoded.len()].copy_from_slice(encoded);
        pos += encoded.len();
    }
    dst[pos] = 0;
}

struct EnumOwnCtx<'a> {
    pid: u32,
    exclude: HWND,
    title_prefix: &'a str,
    found: HWND,
}

unsafe extern "system" fn enum_own_win_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = &mut *(lparam as *mut EnumOwnCtx<'_>);
    if hwnd == ctx.exclude {
        return 1;
    }
    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != ctx.pid {
        return 1;
    }
    let mut title_buf = [0u16; 256];
    let len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    if len > 0 {
        let title = String::from_utf16_lossy(&title_buf[..len as usize]);
        if title.starts_with(ctx.title_prefix) {
            ctx.found = hwnd;
            return 0; // stop enumerating
        }
    }
    1
}

/// Finds a top-level window belonging specifically to the current process whose title starts with `title_prefix`.
pub fn find_own_main_window(title_prefix: &str, exclude_hwnd: HWND) -> HWND {
    unsafe {
        let mut ctx = EnumOwnCtx {
            pid: GetCurrentProcessId(),
            exclude: exclude_hwnd,
            title_prefix,
            found: null_mut(),
        };
        EnumWindows(Some(enum_own_win_proc), &mut ctx as *mut _ as LPARAM);
        ctx.found
    }
}

/// Restores and focuses an existing "Crush 80 Studio" window if present.
fn restore_existing_window() {
    unsafe {
        let title = to_wide_null(APP_WINDOW_TITLE);
        let hwnd = FindWindowW(null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_SHOW);
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

#[link(name = "user32")]
extern "system" {
    fn GetAsyncKeyState(vkey: i32) -> i16;
}

/// Polls hardware-distinguished modifier & special keys directly from Win32:
/// `[LCtrl, RCtrl, LShift, RShift, LAlt, RAlt, LWin, RWin, CapsLock, PrtSc, ScrLk, Pause, Menu, F13]`.
pub fn poll_win32_special_keys() -> [bool; 14] {
    const VKS: [i32; 14] = [
        0xA2, // VK_LCONTROL
        0xA3, // VK_RCONTROL
        0xA0, // VK_LSHIFT
        0xA1, // VK_RSHIFT
        0xA4, // VK_LMENU (LAlt)
        0xA5, // VK_RMENU (RAlt)
        0x5B, // VK_LWIN
        0x5C, // VK_RWIN
        0x14, // VK_CAPITAL (CapsLock)
        0x2C, // VK_SNAPSHOT (PrtSc)
        0x91, // VK_SCROLL (ScrLk)
        0x13, // VK_PAUSE
        0x5D, // VK_APPS (Menu)
        0x7C, // VK_F13
    ];
    let mut out = [false; 14];
    unsafe {
        for (i, &vk) in VKS.iter().enumerate() {
            out[i] = (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
        }
    }
    out
}

// ============================================================================
// 1. Single-instance guard
// ============================================================================

/// RAII guard holding the named single-instance Windows mutex.
/// The mutex handle is closed automatically when dropped.
pub struct SingleInstanceGuard {
    handle: isize,
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if self.handle != 0 {
            unsafe {
                CloseHandle(self.handle as HANDLE);
            }
            self.handle = 0;
        }
    }
}

/// Attempts to acquire the global single-instance mutex.
///
/// If another instance is already running, restores and focuses the existing
/// `"Crush 80 Studio"` window and returns `None` so the duplicate process can exit.
pub fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    unsafe {
        let name = to_wide_null(SINGLE_INSTANCE_MUTEX_NAME);
        let handle = CreateMutexW(null(), 0, name.as_ptr());
        let err = GetLastError();

        if handle.is_null() {
            if err == ERROR_ACCESS_DENIED {
                // Another (possibly elevated) instance already owns the mutex.
                restore_existing_window();
                return None;
            }
            // Unexpected OS error: do not block startup.
            return Some(SingleInstanceGuard { handle: 0 });
        }

        if err == ERROR_ALREADY_EXISTS {
            CloseHandle(handle);
            restore_existing_window();
            return None;
        }

        Some(SingleInstanceGuard {
            handle: handle as isize,
        })
    }
}

// ============================================================================
// 2. Windows Auto-Start (HKCU\...\Run via advapi32.dll)
// ============================================================================

#[allow(clippy::upper_case_acronyms)]
type HKEY = *mut c_void;

// Sign-extended 0x80000001 matches Win32 `((HKEY)(ULONG_PTR)((LONG)0x80000001))` on both x86 and x64.
const HKEY_CURRENT_USER: HKEY = (-2147483647i32 as isize) as HKEY;
const KEY_READ: u32 = 0x0002_0019;
const KEY_SET_VALUE: u32 = 0x0002;
const REG_SZ: u32 = 1;
const ERROR_SUCCESS: i32 = 0;
const ERROR_FILE_NOT_FOUND: i32 = 2;

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(
        hkey: HKEY,
        lp_sub_key: *const u16,
        ul_options: u32,
        sam_desired: u32,
        phk_result: *mut HKEY,
    ) -> i32;

    fn RegQueryValueExW(
        hkey: HKEY,
        lp_value_name: *const u16,
        lp_reserved: *mut u32,
        lp_type: *mut u32,
        lp_data: *mut u8,
        lpcb_data: *mut u32,
    ) -> i32;

    fn RegSetValueExW(
        hkey: HKEY,
        lp_value_name: *const u16,
        reserved: u32,
        dw_type: u32,
        lp_data: *const u8,
        cb_data: u32,
    ) -> i32;

    fn RegDeleteValueW(hkey: HKEY, lp_value_name: *const u16) -> i32;

    fn RegCloseKey(hkey: HKEY) -> i32;
}

/// Returns `true` if `Crush80Studio` is registered in `HKCU\...\Run` and points to the current executable.
pub fn is_autostart_enabled() -> bool {
    let Ok(cur_exe) = std::env::current_exe() else {
        return false;
    };
    let cur_norm = cur_exe.to_string_lossy().trim().to_ascii_lowercase();
    unsafe {
        let subkey = to_wide_null(RUN_REG_SUBKEY);
        let mut hkey: HKEY = null_mut();
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_READ, &mut hkey) != ERROR_SUCCESS
        {
            return false;
        }

        let val_name = to_wide_null(RUN_REG_VALUE_NAME);
        let mut val_type: u32 = 0;
        let mut cb_data: u32 = 0;
        let status = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            &mut val_type,
            null_mut(),
            &mut cb_data,
        );
        if status != ERROR_SUCCESS || val_type != REG_SZ || !(4..=65536).contains(&cb_data) {
            RegCloseKey(hkey);
            return false;
        }
        let mut buf = vec![0u16; (cb_data as usize).div_ceil(2)];
        let mut cb2 = cb_data;
        let status2 = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            &mut val_type,
            buf.as_mut_ptr() as *mut u8,
            &mut cb2,
        );
        RegCloseKey(hkey);
        if status2 != ERROR_SUCCESS {
            return false;
        }
        let nul = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let reg_val = String::from_utf16_lossy(&buf[..nul]).trim().to_ascii_lowercase();
        let expected_quoted = format!("\"{}\"", cur_norm);
        reg_val.starts_with(&expected_quoted) || reg_val.starts_with(&cur_norm)
    }
}

/// Enables or disables Windows auto-start for the current executable.
/// When enabled, writes `"<current_exe_path>" --minimized` under `HKCU\...\Run\Crush80Studio`.
pub fn set_autostart_enabled(enable: bool) -> Result<(), String> {
    unsafe {
        let subkey = to_wide_null(RUN_REG_SUBKEY);
        let mut hkey: HKEY = null_mut();
        let open_status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        );
        if open_status != ERROR_SUCCESS {
            return Err(format!("RegOpenKeyExW failed with code {}", open_status));
        }

        let val_name = to_wide_null(RUN_REG_VALUE_NAME);
        let result = if enable {
            let exe_path = std::env::current_exe()
                .map_err(|e| format!("Failed to resolve current executable path: {}", e));
            match exe_path {
                Ok(path) => {
                    let cmd = format!("\"{}\" --minimized", path.display());
                    let wide = to_wide_null(&cmd);
                    let cb_data = (wide.len() * size_of::<u16>()) as u32;
                    let status = RegSetValueExW(
                        hkey,
                        val_name.as_ptr(),
                        0,
                        REG_SZ,
                        wide.as_ptr() as *const u8,
                        cb_data,
                    );
                    if status == ERROR_SUCCESS {
                        Ok(())
                    } else {
                        Err(format!("RegSetValueExW failed with code {}", status))
                    }
                }
                Err(e) => Err(e),
            }
        } else {
            let status = RegDeleteValueW(hkey, val_name.as_ptr());
            if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("RegDeleteValueW failed with code {}", status))
            }
        };

        RegCloseKey(hkey);
        result
    }
}

// ============================================================================
// 3. Startup argument check
// ============================================================================

/// Returns `true` if `--minimized` was passed on the command line.
pub fn has_minimized_arg() -> bool {
    std::env::args().any(|a| a == "--minimized")
}

// ============================================================================
// 4. Native Win32 Open / Save File Dialogs (comdlg32.dll)
// ============================================================================

const OFN_OVERWRITEPROMPT: u32 = 0x0000_0002;
const OFN_NOCHANGEDIR: u32 = 0x0000_0008;
const OFN_PATHMUSTEXIST: u32 = 0x0000_0800;
const OFN_FILEMUSTEXIST: u32 = 0x0000_1000;
const OFN_EXPLORER: u32 = 0x0008_0000;

#[repr(C)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
struct OPENFILENAMEW {
    lStructSize: u32,
    hwndOwner: HWND,
    hInstance: HINSTANCE,
    lpstrFilter: *const u16,
    lpstrCustomFilter: *mut u16,
    nMaxCustFilter: u32,
    nFilterIndex: u32,
    lpstrFile: *mut u16,
    nMaxFile: u32,
    lpstrFileTitle: *mut u16,
    nMaxFileTitle: u32,
    lpstrInitialDir: *const u16,
    lpstrTitle: *const u16,
    Flags: u32,
    nFileOffset: u16,
    nFileExtension: u16,
    lpstrDefExt: *const u16,
    lCustData: LPARAM,
    lpfnHook: *const c_void,
    lpTemplateName: *const u16,
    pvReserved: *mut c_void,
    dwReserved: u32,
    FlagsEx: u32,
}

#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(lpofn: *mut OPENFILENAMEW) -> i32;
    fn GetSaveFileNameW(lpofn: *mut OPENFILENAMEW) -> i32;
}

fn build_filter_wide(filter_label: &str, filter_ext: &str) -> Vec<u16> {
    let mut out = Vec::new();
    out.extend(filter_label.encode_utf16());
    out.push(0);
    out.extend(filter_ext.encode_utf16());
    out.push(0);
    out.extend("All Files (*.*)".encode_utf16());
    out.push(0);
    out.extend("*.*".encode_utf16());
    out.push(0);
    out.push(0); // double null terminator
    out
}

fn find_owner_hwnd() -> HWND {
    find_own_main_window(APP_WINDOW_TITLE, null_mut())
}

fn wide_buf_to_pathbuf(buf: &[u16]) -> Option<PathBuf> {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if len == 0 {
        return None;
    }
    Some(PathBuf::from(OsString::from_wide(&buf[..len])))
}

/// Opens a native Win32 file picker dialog and returns the selected file path, if any.
///
/// Example: `open_file_dialog("Import Profile", "JSON Profile (*.json)", "*.json")`
pub fn open_file_dialog(title: &str, filter_label: &str, filter_ext: &str) -> Option<PathBuf> {
    let filter_w = build_filter_wide(filter_label, filter_ext);
    let title_w = to_wide_null(title);
    let mut file_buf = vec![0u16; 4096];

    unsafe {
        let mut ofn: OPENFILENAMEW = zeroed();
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = find_owner_hwnd();
        ofn.lpstrFilter = filter_w.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as u32;
        ofn.lpstrTitle = title_w.as_ptr();
        ofn.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_FILEMUSTEXIST | OFN_NOCHANGEDIR;

        if GetOpenFileNameW(&mut ofn) != 0 {
            wide_buf_to_pathbuf(&file_buf)
        } else {
            None
        }
    }
}

/// Opens a native Win32 Save File dialog and returns the chosen destination path, if any.
///
/// Example: `save_file_dialog("Export Profile", "crush80_profile.json", "JSON Profile (*.json)", "*.json")`
pub fn save_file_dialog(
    title: &str,
    default_name: &str,
    filter_label: &str,
    filter_ext: &str,
) -> Option<PathBuf> {
    let filter_w = build_filter_wide(filter_label, filter_ext);
    let title_w = to_wide_null(title);

    let def_ext_str = filter_ext.trim_start_matches('*').trim_start_matches('.');
    let def_ext_w = if !def_ext_str.is_empty()
        && !def_ext_str.contains('*')
        && !def_ext_str.contains(';')
    {
        Some(to_wide_null(def_ext_str))
    } else {
        None
    };

    let mut file_buf = vec![0u16; 4096];
    copy_utf16_truncated(&mut file_buf, default_name);

    unsafe {
        let mut ofn: OPENFILENAMEW = zeroed();
        ofn.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        ofn.hwndOwner = find_owner_hwnd();
        ofn.lpstrFilter = filter_w.as_ptr();
        ofn.nFilterIndex = 1;
        ofn.lpstrFile = file_buf.as_mut_ptr();
        ofn.nMaxFile = file_buf.len() as u32;
        ofn.lpstrTitle = title_w.as_ptr();
        ofn.lpstrDefExt = def_ext_w.as_ref().map_or(null(), |v| v.as_ptr());
        ofn.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_OVERWRITEPROMPT | OFN_NOCHANGEDIR;

        if GetSaveFileNameW(&mut ofn) != 0 {
            wide_buf_to_pathbuf(&file_buf)
        } else {
            None
        }
    }
}

// ============================================================================
// 5. Bluetooth Mode Detection & BLE Battery Level (setupapi.dll)
// ============================================================================

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothBatteryInfo {
    pub name: String,
    pub battery: Option<u8>,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GuidRaw {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DevPropKey {
    fmtid: GuidRaw,
    pid: u32,
}

#[repr(C)]
struct SpDevInfoData {
    cb_size: u32,
    class_guid: GuidRaw,
    dev_inst: u32,
    reserved: usize,
}

const DIGCF_PRESENT: u32 = 0x0000_0002;
const DIGCF_ALLCLASSES: u32 = 0x0000_0004;
const DEVPROP_TYPE_BYTE: u32 = 3;
const DEVPROP_TYPE_GUID: u32 = 13;
const DEVPROP_TYPE_BOOLEAN: u32 = 17;
const DEVPROP_TYPE_STRING: u32 = 18;

const DEVPKEY_DEVICE_DESC: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0xA45C_254E, data2: 0xDF1C, data3: 0x4EFD, data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0] },
    pid: 2,
};
const DEVPKEY_DEVICE_FRIENDLY_NAME: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0xA45C_254E, data2: 0xDF1C, data3: 0x4EFD, data4: [0x80, 0x20, 0x67, 0xD1, 0x46, 0xA8, 0x50, 0xE0] },
    pid: 14,
};
const DEVPKEY_DEVICE_INSTANCE_ID: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0x78C3_4FC8, data2: 0x104A, data3: 0x4ACA, data4: [0x9E, 0xA4, 0x52, 0x4D, 0x52, 0x99, 0x6E, 0x57] },
    pid: 256,
};
const DEVPKEY_DEVICE_CONTAINER_ID: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0x8C7E_D206, data2: 0x3F8A, data3: 0x4827, data4: [0xB3, 0xAB, 0xAE, 0x9E, 0x1F, 0xAE, 0xFC, 0x6C] },
    pid: 2,
};
const DEVPKEY_DEVICE_IS_CONNECTED: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0x83DA_6326, data2: 0x97A6, data3: 0x4088, data4: [0x94, 0x53, 0xA1, 0x92, 0x3F, 0x57, 0x3B, 0x29] },
    pid: 15,
};
const DEVPKEY_BLUETOOTH_BATTERY: DevPropKey = DevPropKey {
    fmtid: GuidRaw { data1: 0x104E_A319, data2: 0x6EE2, data3: 0x4701, data4: [0xBD, 0x47, 0x8D, 0xDB, 0xF4, 0x25, 0xBB, 0xE5] },
    pid: 2,
};

#[link(name = "setupapi")]
extern "system" {
    fn SetupDiGetClassDevsW(
        class_guid: *const GuidRaw,
        enumerator: *const u16,
        hwnd_parent: HWND,
        flags: u32,
    ) -> HANDLE;
    fn SetupDiEnumDeviceInfo(
        device_info_set: HANDLE,
        member_index: u32,
        device_info_data: *mut SpDevInfoData,
    ) -> i32;
    fn SetupDiGetDevicePropertyW(
        device_info_set: HANDLE,
        device_info_data: *const SpDevInfoData,
        property_key: *const DevPropKey,
        property_type: *mut u32,
        property_buffer: *mut u8,
        property_buffer_size: u32,
        required_size: *mut u32,
        flags: u32,
    ) -> i32;
    fn SetupDiDestroyDeviceInfoList(device_info_set: HANDLE) -> i32;
}

unsafe fn read_devprop_string(h: HANDLE, info: &SpDevInfoData, key: &DevPropKey) -> Option<String> {
    let mut prop_type: u32 = 0;
    let mut buf = vec![0u16; 256];
    let mut req: u32 = 0;
    let ok = SetupDiGetDevicePropertyW(
        h,
        info,
        key,
        &mut prop_type,
        buf.as_mut_ptr() as *mut u8,
        (buf.len() * 2) as u32,
        &mut req,
        0,
    );
    if ok == 0 || prop_type != DEVPROP_TYPE_STRING {
        return None;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if len == 0 {
        return None;
    }
    Some(OsString::from_wide(&buf[..len]).to_string_lossy().into_owned())
}

unsafe fn read_devprop_byte(h: HANDLE, info: &SpDevInfoData, key: &DevPropKey, expected_type: u32) -> Option<u8> {
    let mut prop_type: u32 = 0;
    let mut val: u8 = 0;
    let mut req: u32 = 0;
    let ok = SetupDiGetDevicePropertyW(
        h,
        info,
        key,
        &mut prop_type,
        &mut val,
        1,
        &mut req,
        0,
    );
    if ok != 0 && prop_type == expected_type {
        Some(val)
    } else {
        None
    }
}

unsafe fn read_devprop_guid(h: HANDLE, info: &SpDevInfoData, key: &DevPropKey) -> Option<[u8; 16]> {
    let mut prop_type: u32 = 0;
    let mut val = [0u8; 16];
    let mut req: u32 = 0;
    let ok = SetupDiGetDevicePropertyW(
        h,
        info,
        key,
        &mut prop_type,
        val.as_mut_ptr(),
        16,
        &mut req,
        0,
    );
    if ok != 0 && prop_type == DEVPROP_TYPE_GUID {
        Some(val)
    } else {
        None
    }
}

/// Detects a currently connected Bluetooth Crush 80 keyboard and reads its Windows BLE
/// Battery Service level (`DEVPKEY_Bluetooth_Battery`) when USB/2.4G VIA is unavailable.
pub fn query_bluetooth_crush80() -> Option<BluetoothBatteryInfo> {
    unsafe {
        let h = SetupDiGetClassDevsW(null(), null(), null_mut(), DIGCF_PRESENT | DIGCF_ALLCLASSES);
        if h.is_null() || h == (-1isize as HANDLE) {
            return None;
        }

        let mut matched_name: Option<String> = None;
        let mut matched_battery: Option<u8> = None;
        let mut matched_container: Option<[u8; 16]> = None;

        // Pass 1: find a present Bluetooth Crush 80 device node
        let mut idx: u32 = 0;
        loop {
            let mut info: SpDevInfoData = zeroed();
            info.cb_size = size_of::<SpDevInfoData>() as u32;
            if SetupDiEnumDeviceInfo(h, idx, &mut info) == 0 {
                break;
            }
            idx += 1;

            let inst_id = match read_devprop_string(h, &info, &DEVPKEY_DEVICE_INSTANCE_ID) {
                Some(s) => s,
                None => continue,
            };
            let inst_up = inst_id.to_ascii_uppercase();
            if !(inst_up.starts_with("BTHLE\\")
                || inst_up.starts_with("BTHLEDEVICE\\")
                || inst_up.starts_with("BTHENUM\\"))
            {
                continue;
            }

            let name = read_devprop_string(h, &info, &DEVPKEY_DEVICE_FRIENDLY_NAME)
                .or_else(|| read_devprop_string(h, &info, &DEVPKEY_DEVICE_DESC))
                .unwrap_or_default();
            let low = name.to_ascii_lowercase();
            if !(low.contains("crush 80") || low.contains("crush80")) {
                continue;
            }

            // Skip paired-but-disconnected cached nodes when Windows reports IsConnected = false
            if let Some(conn) = read_devprop_byte(h, &info, &DEVPKEY_DEVICE_IS_CONNECTED, DEVPROP_TYPE_BOOLEAN) {
                if conn == 0 {
                    continue;
                }
            }

            let bat = read_devprop_byte(h, &info, &DEVPKEY_BLUETOOTH_BATTERY, DEVPROP_TYPE_BYTE)
                .filter(|&b| b <= 100);
            let container = read_devprop_guid(h, &info, &DEVPKEY_DEVICE_CONTAINER_ID);

            if matched_name.is_none() || (matched_battery.is_none() && bat.is_some()) {
                matched_name = Some(name);
                if bat.is_some() {
                    matched_battery = bat;
                }
                if container.is_some() {
                    matched_container = container;
                }
            }
            if matched_battery.is_some() {
                break;
            }
        }

        // Pass 2: if battery property lives on a sibling PnP node in the same ContainerId
        if matched_name.is_some() && matched_battery.is_none() {
            if let Some(target_cid) = matched_container {
                let mut idx2: u32 = 0;
                loop {
                    let mut info: SpDevInfoData = zeroed();
                    info.cb_size = size_of::<SpDevInfoData>() as u32;
                    if SetupDiEnumDeviceInfo(h, idx2, &mut info) == 0 {
                        break;
                    }
                    idx2 += 1;

                    if let Some(cid) = read_devprop_guid(h, &info, &DEVPKEY_DEVICE_CONTAINER_ID) {
                        if cid == target_cid {
                            if let Some(bat) = read_devprop_byte(h, &info, &DEVPKEY_BLUETOOTH_BATTERY, DEVPROP_TYPE_BYTE) {
                                if bat <= 100 {
                                    matched_battery = Some(bat);
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        SetupDiDestroyDeviceInfoList(h);

        matched_name.map(|name| BluetoothBatteryInfo {
            name,
            battery: matched_battery,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_utf16_truncated_preserves_surrogate_pairs() {
        // Each 🔥 is 2 UTF-16 code units (surrogate pair).
        // Into a 6-u16 buffer (5 payload + 1 NUL), at most 2 emojis = 4 code units fit cleanly.
        let mut buf = [0xFFFFu16; 6];
        copy_utf16_truncated(&mut buf, "🔥🔥🔥🔥");
        let nul_pos = buf.iter().position(|&c| c == 0).unwrap();
        assert_eq!(nul_pos, 4);
        assert_eq!(String::from_utf16(&buf[..nul_pos]).unwrap(), "🔥🔥");
    }

    #[test]
    fn test_build_filter_wide_double_null_terminated() {
        let w = build_filter_wide("JSON Profile (*.json)", "*.json");
        assert!(w.len() >= 4);
        assert_eq!(w[w.len() - 1], 0);
        assert_eq!(w[w.len() - 2], 0);
    }
}
