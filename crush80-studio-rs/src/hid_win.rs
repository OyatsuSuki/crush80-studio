//! Pure-Rust native Win32 HID driver for WOBKEY Crush 80 Pro Reboot (USB 0x5055 + 2.4G 0x5088).
//! Uses SetupAPI + HID + Overlapped ReadFile/WriteFile directly via `windows-sys` (zero C deps).

use serde::{Deserialize, Serialize};
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
use windows_sys::Win32::Devices::HumanInterfaceDevice::*;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::IO::*;
use windows_sys::Win32::System::Threading::*;

const VID: u16 = 0x320F;
const PID_USB: u16 = 0x5055;
const PID_24G: u16 = 0x5088;
const USAGE_VIA: u16 = 0xFF60;
const USAGE_OTA: u16 = 0xFFEF;
const KEYMAP_BYTES: usize = 1024;
const MACRO_BYTES: usize = 512;
const CHUNK_SIZE: usize = 16;

#[derive(Clone, Debug, Default)]
pub struct DeviceStatus {
    pub connected: bool,
    pub pid: u16,
    pub transport: String,
    pub firmware_crc: Option<u32>,
    pub firmware_name: String,
    pub via_protocol: u16,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbState {
    pub brightness: u8,
    pub effect: u8,
    pub speed: u8,
    pub hue: u8,
    pub saturation: u8,
    #[serde(default)]
    pub rainbow: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardwareConfig {
    pub win_lock: bool,        // CONFIG[21]: 0xFF=locked
    pub mac_mode: bool,        // CONFIG[26]: 0x01=macOS
    pub low_latency: bool,     // CONFIG[29]: 0xFF=ISR fast debounce
    pub logo_mode: u8,         // CONFIG[32]
    pub logo_color: u8,        // CONFIG[33]: 0xFF=rainbow
    pub logo_on: bool,         // CONFIG[35]: 0x00=ON, 0x01=OFF
    pub side_mode: u8,         // CONFIG[36]
    pub side_color: u8,        // CONFIG[37]: 0xFF=rainbow
    pub side_on: bool,         // CONFIG[39]: 0x00=ON, 0x01=OFF
    pub sys_rgb_on: bool,      // CONFIG[40]: 0x00=ON, 0x01=OFF
    pub tapping_term_ms: u16,  // CONFIG[60]: 10ms units (0 => 200ms)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery: Option<u8>,   // Live battery % (0..100) via VIA id=0xFE
    #[serde(default)]
    pub prosuite_supported: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FullSnapshot {
    pub timestamp: String,
    pub keymap_hex: String,
    pub macros_hex: String,
    pub rgb: RgbState,
    #[serde(default)]
    pub hw: Option<HardwareConfig>,
}

pub(crate) struct HidMatch {
    path: Vec<u16>,
    pub(crate) pid: u16,
    pub(crate) usage_page: u16,
    input_len: usize,
    output_len: usize,
}

fn find_hid_devices(target_usage_page: u16) -> Vec<HidMatch> {
    unsafe {
        let mut hid_guid = zeroed();
        HidD_GetHidGuid(&mut hid_guid);

        let dev_info = SetupDiGetClassDevsW(
            &hid_guid,
            null(),
            null_mut(),
            DIGCF_PRESENT | DIGCF_DEVICEINTERFACE,
        );
        if dev_info == INVALID_HANDLE_VALUE as isize {
            return Vec::new();
        }

        let mut results = Vec::new();
        let mut idx: u32 = 0;
        loop {
            let mut iface_data: SP_DEVICE_INTERFACE_DATA = zeroed();
            iface_data.cbSize = size_of::<SP_DEVICE_INTERFACE_DATA>() as u32;
            if SetupDiEnumDeviceInterfaces(dev_info, null(), &hid_guid, idx, &mut iface_data) == 0 {
                break;
            }
            idx += 1;

            let mut req_size: u32 = 0;
            SetupDiGetDeviceInterfaceDetailW(dev_info, &iface_data, null_mut(), 0, &mut req_size, null_mut());
            if req_size == 0 {
                continue;
            }

            let mut buf = vec![0u8; req_size as usize];
            let detail = buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
            (*detail).cbSize = if size_of::<usize>() == 8 { 8 } else { 6 };

            if SetupDiGetDeviceInterfaceDetailW(
                dev_info,
                &iface_data,
                detail,
                req_size,
                null_mut(),
                null_mut(),
            ) == 0
            {
                continue;
            }

            // Extract null-terminated UTF-16 path
            let path_ptr = &(*detail).DevicePath as *const u16;
            let mut path = Vec::new();
            let mut p = path_ptr;
            while *p != 0 {
                path.push(*p);
                p = p.add(1);
            }
            path.push(0);

            // Query attributes with zero access (works even if exclusively held)
            let h = CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                continue;
            }

            let mut attrs: HIDD_ATTRIBUTES = zeroed();
            attrs.Size = size_of::<HIDD_ATTRIBUTES>() as u32;
            let ok_attr = HidD_GetAttributes(h, &mut attrs) != 0;

            if ok_attr && attrs.VendorID == VID && (attrs.ProductID == PID_USB || attrs.ProductID == PID_24G) {
                let mut ppd: isize = 0;
                if HidD_GetPreparsedData(h, &mut ppd) != 0 {
                    let mut caps: HIDP_CAPS = zeroed();
                    if HidP_GetCaps(ppd, &mut caps) == HIDP_STATUS_SUCCESS {
                        if caps.UsagePage == target_usage_page {
                            results.push(HidMatch {
                                path: path.clone(),
                                pid: attrs.ProductID,
                                usage_page: caps.UsagePage,
                                input_len: caps.InputReportByteLength as usize,
                                output_len: caps.OutputReportByteLength as usize,
                            });
                        }
                    }
                    HidD_FreePreparsedData(ppd);
                }
            }
            CloseHandle(h);
        }

        SetupDiDestroyDeviceInfoList(dev_info);
        results
    }
}

fn find_hid_device(target_usage_page: u16) -> Option<HidMatch> {
    let mut devs = find_hid_devices(target_usage_page);
    // Prefer wired USB (PID_USB) if both wired and 2.4G dongle are present
    if let Some(pos) = devs.iter().position(|d| d.pid == PID_USB) {
        return Some(devs.swap_remove(pos));
    }
    devs.into_iter().next()
}

pub(crate) struct WinHidHandle {
    handle: HANDLE,
    input_len: usize,
    output_len: usize,
}

impl WinHidHandle {
    fn open(m: &HidMatch) -> Result<Self, String> {
        unsafe {
            let h = CreateFileW(
                m.path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                null_mut(),
            );
            if h == INVALID_HANDLE_VALUE {
                return Err(format!("无法打开 HID 接口 (0x{:04X}), GetLastError={}", m.usage_page, GetLastError()));
            }
            Ok(Self {
                handle: h,
                input_len: m.input_len.max(33),
                output_len: m.output_len.max(33),
            })
        }
    }

    pub(crate) fn write_report(&self, data: &[u8]) -> Result<(), String> {
        let mut buf = vec![0u8; self.output_len];
        let copy_len = data.len().min(buf.len());
        buf[..copy_len].copy_from_slice(&data[..copy_len]);

        unsafe {
            let ev = CreateEventW(null(), 1, 0, null());
            let mut ov: OVERLAPPED = zeroed();
            ov.hEvent = ev;
            let mut written: u32 = 0;
            let ok = WriteFile(
                self.handle,
                buf.as_ptr(),
                buf.len() as u32,
                &mut written,
                &mut ov,
            );
            if ok == 0 {
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    CloseHandle(ev);
                    return Err(format!("HID 写入失败: err={}", err));
                }
                let wait = WaitForSingleObject(ev, 2000);
                if wait != WAIT_OBJECT_0 {
                    CancelIo(self.handle);
                    CloseHandle(ev);
                    return Err("HID 写入超时".into());
                }
                GetOverlappedResult(self.handle, &ov, &mut written, 0);
            }
            CloseHandle(ev);
            Ok(())
        }
    }

    pub(crate) fn read_report(&self, timeout_ms: u32) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; self.input_len];
        unsafe {
            let ev = CreateEventW(null(), 1, 0, null());
            let mut ov: OVERLAPPED = zeroed();
            ov.hEvent = ev;
            let mut read_bytes: u32 = 0;
            let ok = ReadFile(
                self.handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read_bytes,
                &mut ov,
            );
            if ok == 0 {
                let err = GetLastError();
                if err != ERROR_IO_PENDING {
                    CloseHandle(ev);
                    return Err(format!("HID 读取失败: err={}", err));
                }
                let wait = WaitForSingleObject(ev, timeout_ms);
                if wait != WAIT_OBJECT_0 {
                    CancelIo(self.handle);
                    CloseHandle(ev);
                    return Err("HID 读取超时".into());
                }
                GetOverlappedResult(self.handle, &ov, &mut read_bytes, 0);
            }
            CloseHandle(ev);
            buf.truncate(read_bytes as usize);
            Ok(buf)
        }
    }
}

impl Drop for WinHidHandle {
    fn drop(&mut self) {
        unsafe {
            if self.handle != INVALID_HANDLE_VALUE && !self.handle.is_null() {
                CloseHandle(self.handle);
            }
        }
    }
}

pub struct Crush80Hid {
    lock: Mutex<()>,
    ota_busy: AtomicBool,
}

impl Crush80Hid {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
            ota_busy: AtomicBool::new(false),
        }
    }

    pub fn is_ota_busy(&self) -> bool {
        self.ota_busy.load(Ordering::Acquire)
    }

    pub(crate) fn try_begin_ota(&self) -> bool {
        self.ota_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    pub(crate) fn end_ota(&self) {
        self.ota_busy.store(false, Ordering::Release);
    }

    pub(crate) fn lock_hid(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap()
    }

    /// Open the wired USB OTA HID interface (PID 0x5055, Usage Page 0xFFEF).
    /// Strictly refuses 2.4G dongle (PID 0x5088) and multiple wired devices.
    /// Caller should hold `lock_hid()` while performing OTA I/O.
    pub(crate) fn open_ota_handle(&self) -> Result<(WinHidHandle, HidMatch), String> {
        let all_ota = find_hid_devices(USAGE_OTA);
        let has_24g = all_ota.iter().any(|d| d.pid == PID_24G);
        let mut wired: Vec<HidMatch> = all_ota.into_iter().filter(|d| d.pid == PID_USB).collect();
        if wired.is_empty() {
            if has_24g || find_hid_devices(USAGE_VIA).iter().any(|d| d.pid == PID_24G) {
                return Err(
                    "检测到 2.4G 无线模式，OTA 固件刷写仅支持 USB 有线模式 (PID 0x5055)，请连接 USB 数据线并切换至有线模式"
                        .into(),
                );
            }
            return Err("未检测到有线 Crush 80 OTA 接口 (PID 0x5055, Usage 0xFFEF)".into());
        }
        if wired.len() > 1 {
            return Err(format!(
                "检测到 {} 个有线 Crush 80 OTA 接口，请仅连接一台键盘后再刷写",
                wired.len()
            ));
        }
        let m = wired.remove(0);
        let h = WinHidHandle::open(&m)?;
        Ok((h, m))
    }

    fn via_cmd(h: &WinHidHandle, payload: &[u8]) -> Result<[u8; 32], String> {
        let mut pkt = [0u8; 33];
        pkt[0] = 0x00; // Report ID 0
        let n = payload.len().min(32);
        pkt[1..1 + n].copy_from_slice(&payload[..n]);
        h.write_report(&pkt)?;
        let resp = h.read_report(1500)?;
        if resp.len() < 33 {
            return Err(format!("VIA 返回包过短: {} bytes", resp.len()));
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&resp[1..33]);
        Ok(out)
    }

    fn open_via(&self) -> Result<(WinHidHandle, HidMatch), String> {
        let m = find_hid_device(USAGE_VIA)
            .ok_or_else(|| "未检测到 Crush 80，请确认有线或 2.4G 已连接".to_string())?;
        let h = WinHidHandle::open(&m)?;
        Ok((h, m))
    }

    pub fn get_status(&self) -> DeviceStatus {
        if self.is_ota_busy() {
            return DeviceStatus {
                connected: true,
                pid: PID_USB,
                transport: "USB 有线 (OTA)".into(),
                firmware_crc: None,
                firmware_name: "OTA 刷写中...".into(),
                via_protocol: 0,
            };
        }
        let _g = self.lock.lock().unwrap();
        let Some(m) = find_hid_device(USAGE_VIA) else {
            return DeviceStatus::default();
        };

        let mut st = DeviceStatus {
            connected: true,
            pid: m.pid,
            transport: if m.pid == PID_USB { "USB 有线".into() } else { "2.4G 无线".into() },
            firmware_crc: None,
            firmware_name: "2.4G 模式 (仅 USB 可查 CRC)".into(),
            via_protocol: 0,
        };

        if let Ok(h) = WinHidHandle::open(&m) {
            if let Ok(r) = Self::via_cmd(&h, &[0x01]) {
                if r[0] == 0x01 {
                    st.via_protocol = ((r[1] as u16) << 8) | (r[2] as u16);
                }
            }
        }

        if let Some(ota_m) = find_hid_device(USAGE_OTA) {
            for _ in 0..2 {
                if let Ok(h) = WinHidHandle::open(&ota_m) {
                    let mut pkt = [0xFFu8; 64];
                    pkt[0] = 5;
                    pkt[1] = 1;
                    pkt[2] = 0;
                    pkt[3] = 0;
                    if h.write_report(&pkt).is_ok() {
                        if let Ok(resp) = h.read_report(1000) {
                            if resp.len() >= 12 && resp[0..4] == [0x05, 0x01, 0x08, 0x00] {
                                let crc = u32::from_le_bytes([resp[8], resp[9], resp[10], resp[11]]);
                                st.firmware_crc = Some(crc);
                                st.firmware_name = match crc {
                                    0xCA2D85DC => "Official WOBLAB 0108".into(),
                                    0xFE1AA4C2 => "Community RC2".into(),
                                    0x2F4F4974 => "NativeHeld".into(),
                                    0xAF713C2C => "Ember".into(),
                                    0x99FCFDBD => "Ember".into(),
                                    0x4859A8C5 => "Ember".into(),
                                    0xC03D5821 => "Ember".into(),
                                    0x6C8D7399 => "Ember".into(),
                                    0x0AACEB06 => "Ember".into(),
                                    _ => format!("Custom {:08X}", crc),
                                };
                                break;
                            }
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(80));
            }
        }

        st
    }

    pub fn read_keymap(&self) -> Result<[[u16; 128]; 4], String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let mut buf = Vec::with_capacity(KEYMAP_BYTES);
        let mut offset = 0usize;
        while offset < KEYMAP_BYTES {
            let size = CHUNK_SIZE.min(KEYMAP_BYTES - offset);
            let r = Self::via_cmd(&h, &[0x12, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8])?;
            if r[0] != 0x12 {
                return Err(format!("读取键位失败 @ {}", offset));
            }
            buf.extend_from_slice(&r[4..4 + size]);
            offset += size;
        }
        let mut layers = [[0u16; 128]; 4];
        for l in 0..4 {
            for i in 0..128 {
                let p = l * 256 + i * 2;
                layers[l][i] = ((buf[p] as u16) << 8) | (buf[p + 1] as u16);
            }
        }
        Ok(layers)
    }

    pub fn set_key(&self, layer: u8, row: u8, col: u8, keycode: u16) -> Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let r = Self::via_cmd(&h, &[0x05, layer, row, col, (keycode >> 8) as u8, (keycode & 0xFF) as u8])?;
        if r[0] != 0x05 {
            return Err("写入按键失败".into());
        }
        Ok(())
    }

    pub fn read_rgb(&self) -> Result<RgbState, String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let b = Self::via_cmd(&h, &[0x08, 0x03, 1])?[3];
        let e = Self::via_cmd(&h, &[0x08, 0x03, 2])?[3];
        let sp = Self::via_cmd(&h, &[0x08, 0x03, 3])?[3];
        let col = Self::via_cmd(&h, &[0x08, 0x03, 4])?;
        let rb = Self::via_cmd(&h, &[0x08, 0x03, 5, 0xAA])?[3] == 0xFF;
        Ok(RgbState {
            brightness: b,
            effect: e,
            speed: sp,
            hue: col[3],
            saturation: col[4],
            rainbow: rb,
        })
    }

    pub fn set_rgb_partial(
        &self,
        brightness: Option<u8>,
        effect: Option<u8>,
        speed: Option<u8>,
        color: Option<(u8, u8)>,
        rainbow: Option<bool>,
        save: bool,
    ) -> Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        if let Some(v) = brightness {
            Self::via_cmd(&h, &[0x07, 0x03, 1, v.min(9)])?;
        }
        if let Some(v) = effect {
            Self::via_cmd(&h, &[0x07, 0x03, 2, v.min(18)])?;
        }
        if let Some(v) = speed {
            Self::via_cmd(&h, &[0x07, 0x03, 3, v.min(4)])?;
        }
        if let Some((hue, sat)) = color {
            Self::via_cmd(&h, &[0x07, 0x03, 4, hue, sat])?;
        }
        if let Some(rb) = rainbow {
            Self::via_cmd(&h, &[0x07, 0x03, 5, if rb { 0xFF } else { 0x00 }])?;
        }
        if save {
            Self::via_cmd(&h, &[0x09])?;
        }
        Ok(())
    }

    pub fn read_hw_config(&self) -> Result<HardwareConfig, String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        Self::read_hw_config_with(&h)
    }

    fn read_hw_config_with(h: &WinHidHandle) -> Result<HardwareConfig, String> {
        // Probe CONFIG[26] (OS mode, 0 or 1 on ProSuite) with sentinel 0xAA.
        // Stock RGB & Non-RGB firmwares ignore id>=5 and echo back 0xAA untouched!
        let os_probe = Self::via_cmd(h, &[0x08, 0x03, 26, 0xAA])?[3];
        if os_probe == 0xAA {
            return Ok(HardwareConfig {
                logo_on: true,
                side_on: true,
                sys_rgb_on: true,
                tapping_term_ms: 200,
                battery: None,
                prosuite_supported: false,
                ..Default::default()
            });
        }
        let get = |id: u8| -> Result<u8, String> { Ok(Self::via_cmd(h, &[0x08, 0x03, id])?[3]) };
        let tt_units = get(60)?;
        // Sentinel 0xFF ensures older/non-v3 firmware echoes 0xFF (-> None) instead of bogus 0%
        let bat_raw = Self::via_cmd(h, &[0x08, 0x03, 0xFE, 0xFF])
            .map(|r| r[3])
            .unwrap_or(255);
        Ok(HardwareConfig {
            win_lock: get(21)? != 0,
            mac_mode: os_probe != 0,
            low_latency: get(29)? != 0,
            logo_mode: get(32)?,
            logo_color: get(33)?,
            logo_on: get(35)? == 0,
            side_mode: get(36)?,
            side_color: get(37)?,
            side_on: get(39)? == 0,
            sys_rgb_on: get(40)? == 0,
            tapping_term_ms: if tt_units == 0 { 200 } else { (tt_units as u16) * 10 },
            battery: if bat_raw <= 100 { Some(bat_raw) } else { None },
            prosuite_supported: true,
        })
    }

    /// Single-session fast poll of both RGB and HardwareConfig (for real-time bidirectional sync).
    pub fn poll_live_state(&self) -> Result<(RgbState, HardwareConfig, u16), String> {
        if self.is_ota_busy() {
            return Err("OTA in progress".into());
        }
        let _g = self.lock.lock().unwrap();
        let (h, m) = self.open_via()?;
        let b = Self::via_cmd(&h, &[0x08, 0x03, 1])?[3];
        let e = Self::via_cmd(&h, &[0x08, 0x03, 2])?[3];
        let sp = Self::via_cmd(&h, &[0x08, 0x03, 3])?[3];
        let col = Self::via_cmd(&h, &[0x08, 0x03, 4])?;
        let rb = Self::via_cmd(&h, &[0x08, 0x03, 5, 0xAA])?[3] == 0xFF;
        let rgb = RgbState {
            brightness: b,
            effect: e,
            speed: sp,
            hue: col[3],
            saturation: col[4],
            rainbow: rb,
        };
        let hw = Self::read_hw_config_with(&h)?;
        Ok((rgb, hw, m.pid))
    }

    pub fn set_config_byte(&self, id: u8, val: u8, save: bool) -> Result<(), String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        Self::via_cmd(&h, &[0x07, 0x03, id, val])?;
        if save {
            Self::via_cmd(&h, &[0x09])?;
        }
        Ok(())
    }

    /// Triggers native OEM Logo LED flash feedback on Ember firmware (`id=0xFD`):
    /// writes `[B, G, R, count, 0]` to `gp+759..763` (zero flash wear, safely ignored by stock firmware).
    pub fn trigger_led_feedback(&self, count: u8, r: u8, g: u8, b: u8) {
        if count == 0 {
            return;
        }
        let _g = self.lock.lock().unwrap();
        if let Ok((h, _)) = self.open_via() {
            let _ = Self::via_cmd(&h, &[0x07, 0x03, 0xFD, b, g, r, count.min(6), 0]);
        }
    }

    pub fn read_macros(&self) -> Result<(Vec<String>, Vec<u8>), String> {
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let mut buf = Vec::with_capacity(MACRO_BYTES);
        let mut offset = 0usize;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let r = Self::via_cmd(&h, &[0x0E, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8])?;
            if r[0] != 0x0E {
                return Err(format!("读取宏失败 @ {}", offset));
            }
            buf.extend_from_slice(&r[4..4 + size]);
            offset += size;
        }
        let mut slots = Vec::with_capacity(16);
        let mut idx = 0usize;
        for _ in 0..16 {
            let end = buf[idx..].iter().position(|&b| b == 0).map(|p| idx + p).unwrap_or(buf.len());
            let s = String::from_utf8_lossy(&buf[idx..end]).to_string();
            slots.push(s);
            idx = (end + 1).min(buf.len());
        }
        Ok((slots, buf))
    }

    pub fn write_macros(&self, slots: &[String]) -> Result<(), String> {
        let mut buf = Vec::with_capacity(MACRO_BYTES);
        for s in slots.iter().take(16) {
            for b in s.bytes() {
                if (1..=126).contains(&b) {
                    buf.push(b);
                }
            }
            buf.push(0);
        }
        if buf.len() > MACRO_BYTES {
            return Err(format!("宏总长度超过 512 字节 ({})", buf.len()));
        }
        buf.resize(MACRO_BYTES, 0);

        self.write_macro_buffer(&buf)
    }

    /// Write a raw 512-byte VIA macro buffer directly to the keyboard.
    pub fn write_macro_buffer(&self, raw: &[u8]) -> Result<(), String> {
        if raw.len() != MACRO_BYTES {
            return Err(format!("宏缓冲区长度无效: 期望 {} 字节，实际 {}", MACRO_BYTES, raw.len()));
        }
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let mut offset = 0usize;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let mut req = vec![0x0F, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&raw[offset..offset + size]);
            let r = Self::via_cmd(&h, &req)?;
            if r[0] != 0x0F {
                return Err(format!("写入宏失败 @ {}", offset));
            }
            offset += size;
        }
        Ok(())
    }

    pub fn export_snapshot(&self, timestamp: String) -> Result<FullSnapshot, String> {
        let layers = self.read_keymap()?;
        let mut km = Vec::with_capacity(KEYMAP_BYTES);
        for l in &layers {
            for &c in l {
                km.push((c >> 8) as u8);
                km.push((c & 0xFF) as u8);
            }
        }
        let (_, macros) = self.read_macros()?;
        let rgb = self.read_rgb()?;
        let mut hw = self.read_hw_config()?;
        hw.battery = None;
        Ok(FullSnapshot {
            timestamp,
            keymap_hex: to_hex(&km),
            macros_hex: to_hex(&macros),
            rgb,
            hw: Some(hw),
        })
    }

    pub fn apply_snapshot(&self, snap: &FullSnapshot) -> Result<(), String> {
        let km = from_hex(&snap.keymap_hex)?;
        let mc = from_hex(&snap.macros_hex)?;
        if km.len() != KEYMAP_BYTES || mc.len() != MACRO_BYTES {
            return Err("方案文件长度无效".into());
        }
        let _g = self.lock.lock().unwrap();
        let (h, _) = self.open_via()?;
        let mut offset = 0usize;
        while offset < KEYMAP_BYTES {
            let size = CHUNK_SIZE.min(KEYMAP_BYTES - offset);
            let mut req = vec![0x13, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&km[offset..offset + size]);
            Self::via_cmd(&h, &req)?;
            offset += size;
        }
        offset = 0;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let mut req = vec![0x0F, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&mc[offset..offset + size]);
            Self::via_cmd(&h, &req)?;
            offset += size;
        }
        Self::via_cmd(&h, &[0x07, 0x03, 1, snap.rgb.brightness.min(9)])?;
        Self::via_cmd(&h, &[0x07, 0x03, 2, snap.rgb.effect.min(18)])?;
        Self::via_cmd(&h, &[0x07, 0x03, 3, snap.rgb.speed.min(4)])?;
        if snap.rgb.rainbow {
            Self::via_cmd(&h, &[0x07, 0x03, 5, 0xFF])?;
        } else {
            Self::via_cmd(&h, &[0x07, 0x03, 4, snap.rgb.hue, snap.rgb.saturation])?;
        }
        if let Some(hw) = &snap.hw {
            Self::via_cmd(&h, &[0x07, 0x03, 21, if hw.win_lock { 0xFF } else { 0x00 }])?;
            Self::via_cmd(&h, &[0x07, 0x03, 26, if hw.mac_mode { 0x01 } else { 0x00 }])?;
            Self::via_cmd(&h, &[0x07, 0x03, 29, if hw.low_latency { 0xFF } else { 0x00 }])?;
            Self::via_cmd(&h, &[0x07, 0x03, 32, hw.logo_mode])?;
            Self::via_cmd(&h, &[0x07, 0x03, 33, hw.logo_color])?;
            Self::via_cmd(&h, &[0x07, 0x03, 35, if hw.logo_on { 0x00 } else { 0x01 }])?;
            Self::via_cmd(&h, &[0x07, 0x03, 36, hw.side_mode])?;
            Self::via_cmd(&h, &[0x07, 0x03, 37, hw.side_color])?;
            Self::via_cmd(&h, &[0x07, 0x03, 39, if hw.side_on { 0x00 } else { 0x01 }])?;
            Self::via_cmd(&h, &[0x07, 0x03, 40, if hw.sys_rgb_on { 0x00 } else { 0x01 }])?;
            let tt_units = if hw.tapping_term_ms == 0 {
                0
            } else {
                (hw.tapping_term_ms / 10).clamp(1, 255) as u8
            };
            Self::via_cmd(&h, &[0x07, 0x03, 60, tt_units])?;
        }
        Self::via_cmd(&h, &[0x09])?;
        // Mint x2 physical Logo LED pulse to acknowledge Profile / AutoSwitch load
        let _ = Self::via_cmd(&h, &[0x07, 0x03, 0xFD, 153, 211, 52, 2, 0]);
        Ok(())
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn from_hex(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err("无效的 hex 长度".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let b = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "无效的 hex 字符".to_string())?;
        out.push(b);
    }
    Ok(out)
}
