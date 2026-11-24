//! Pure-Rust native Win32 HID driver for WOBKEY Crush 80 Pro Reboot (USB 0x5055 + 2.4G 0x5088).
//! Uses SetupAPI + HID + Overlapped ReadFile/WriteFile directly via `windows-sys` (zero C deps).

use crate::keycodes::BoardEdition;
use serde::{Deserialize, Serialize};
use std::mem::{offset_of, size_of, zeroed};
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

pub(crate) trait HidTransport {
    fn write_report(&self, data: &[u8]) -> Result<(), String>;
    fn read_report(&self, timeout_ms: u32) -> Result<Vec<u8>, String>;
}

#[derive(Clone, Debug, Default)]
pub struct DeviceStatus {
    pub connected: bool,
    pub pid: u16,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<BoardEdition>,
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
            let min_size = size_of::<SP_DEVICE_INTERFACE_DETAIL_DATA_W>();
            if (req_size as usize) < min_size {
                continue;
            }

            // Allocate 4-byte aligned backing storage to satisfy SP_DEVICE_INTERFACE_DETAIL_DATA_W alignment.
            let mut aligned_buf = vec![0u32; (req_size as usize).div_ceil(4)];
            let detail = aligned_buf.as_mut_ptr() as *mut SP_DEVICE_INTERFACE_DETAIL_DATA_W;
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

            // Bounded extraction of null-terminated UTF-16 path within `req_size`.
            let path_offset = offset_of!(SP_DEVICE_INTERFACE_DETAIL_DATA_W, DevicePath);
            let max_u16 = (req_size as usize).saturating_sub(path_offset) / size_of::<u16>();
            if max_u16 == 0 {
                continue;
            }
            let path_ptr = &(*detail).DevicePath as *const u16;
            let raw_slice = std::slice::from_raw_parts(path_ptr, max_u16);
            let nul_pos = raw_slice.iter().position(|&c| c == 0).unwrap_or(raw_slice.len());
            let mut path = Vec::with_capacity(nul_pos + 1);
            path.extend_from_slice(&raw_slice[..nul_pos]);
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
                    if HidP_GetCaps(ppd, &mut caps) == HIDP_STATUS_SUCCESS
                        && caps.UsagePage == target_usage_page
                    {
                        results.push(HidMatch {
                            path: path.clone(),
                            pid: attrs.ProductID,
                            usage_page: caps.UsagePage,
                            input_len: caps.InputReportByteLength as usize,
                            output_len: caps.OutputReportByteLength as usize,
                        });
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
            if ev.is_null() {
                return Err(format!("创建 HID 写入事件失败: err={}", GetLastError()));
            }
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
                    // Must wait for cancelled IRP completion before freeing `ov` and `buf`.
                    CancelIoEx(self.handle, &ov);
                    let mut cancelled_bytes: u32 = 0;
                    GetOverlappedResult(self.handle, &ov, &mut cancelled_bytes, 1);
                    CloseHandle(ev);
                    return Err("HID 写入超时".into());
                }
                if GetOverlappedResult(self.handle, &ov, &mut written, 0) == 0 {
                    let ov_err = GetLastError();
                    CloseHandle(ev);
                    return Err(format!("HID 写入结果异常: err={}", ov_err));
                }
            }
            CloseHandle(ev);
            if (written as usize) != buf.len() {
                return Err(format!(
                    "HID 写入不完整: 期望 {} 字节，实际 {}",
                    buf.len(),
                    written
                ));
            }
            Ok(())
        }
    }

    pub(crate) fn read_report(&self, timeout_ms: u32) -> Result<Vec<u8>, String> {
        let mut buf = vec![0u8; self.input_len];
        unsafe {
            let ev = CreateEventW(null(), 1, 0, null());
            if ev.is_null() {
                return Err(format!("创建 HID 读取事件失败: err={}", GetLastError()));
            }
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
                    // Must wait for cancelled IRP completion before freeing `ov` and `buf`.
                    CancelIoEx(self.handle, &ov);
                    let mut cancelled_bytes: u32 = 0;
                    GetOverlappedResult(self.handle, &ov, &mut cancelled_bytes, 1);
                    CloseHandle(ev);
                    return Err("HID 读取超时".into());
                }
                if GetOverlappedResult(self.handle, &ov, &mut read_bytes, 0) == 0 {
                    let ov_err = GetLastError();
                    CloseHandle(ev);
                    return Err(format!("HID 读取结果异常: err={}", ov_err));
                }
            }
            CloseHandle(ev);
            buf.truncate(read_bytes as usize);
            Ok(buf)
        }
    }
}

impl HidTransport for WinHidHandle {
    fn write_report(&self, data: &[u8]) -> Result<(), String> {
        WinHidHandle::write_report(self, data)
    }

    fn read_report(&self, timeout_ms: u32) -> Result<Vec<u8>, String> {
        WinHidHandle::read_report(self, timeout_ms)
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
        self.lock.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn ensure_not_ota_busy(&self) -> Result<(), String> {
        if self.is_ota_busy() {
            return Err("OTA 刷写进行中，已暂停其他设备操作".into());
        }
        Ok(())
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

    fn via_cmd(h: &impl HidTransport, payload: &[u8]) -> Result<[u8; 32], String> {
        if payload.is_empty() {
            return Err("VIA 指令载荷为空".into());
        }
        let mut pkt = [0u8; 33];
        pkt[0] = 0x00; // Report ID 0
        let n = payload.len().min(32);
        pkt[1..1 + n].copy_from_slice(&payload[..n]);
        h.write_report(&pkt)?;
        let resp = h.read_report(1500)?;
        if resp.len() < 33 {
            return Err(format!("VIA 返回包过短: {} bytes", resp.len()));
        }
        if resp[1] != payload[0] {
            return Err(format!(
                "VIA 响应命令字不匹配: 期望 0x{:02X}，实际 0x{:02X}",
                payload[0], resp[1]
            ));
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
                firmware_crc: None,
                firmware_name: "OTA 刷写中...".into(),
                via_protocol: 0,
            };
        }
        let _g = self.lock_hid();
        let Some(m) = find_hid_device(USAGE_VIA) else {
            return DeviceStatus::default();
        };

        let mut st = DeviceStatus {
            connected: true,
            pid: m.pid,
            firmware_crc: None,
            firmware_name: if m.pid == PID_USB {
                "USB 有线".into()
            } else {
                "2.4G 模式 (仅 USB 可查 CRC)".into()
            },
            via_protocol: 0,
        };

        if let Ok(h) = WinHidHandle::open(&m) {
            if let Ok(r) = Self::via_cmd(&h, &[0x01]) {
                st.via_protocol = ((r[1] as u16) << 8) | (r[2] as u16);
            }
        }

        // Only query USAGE_OTA (0xFFEF) on wired USB (PID 0x5055); 2.4G dongle does not reply.
        if m.pid == PID_USB {
            if let Some(ota_m) = find_hid_devices(USAGE_OTA).into_iter().find(|d| d.pid == PID_USB) {
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
                                    st.firmware_name = crate::ota::known_firmware_name(crc);
                                    break;
                                }
                            }
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(80));
                }
            }
        }

        st
    }

    pub fn read_keymap(&self) -> Result<[[u16; 128]; 4], String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        let mut buf = Vec::with_capacity(KEYMAP_BYTES);
        let mut offset = 0usize;
        while offset < KEYMAP_BYTES {
            let size = CHUNK_SIZE.min(KEYMAP_BYTES - offset);
            let r = Self::via_cmd(&h, &[0x12, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8])?;
            buf.extend_from_slice(&r[4..4 + size]);
            offset += size;
        }
        let mut layers = [[0u16; 128]; 4];
        for (l, layer) in layers.iter_mut().enumerate() {
            for (i, cell) in layer.iter_mut().enumerate() {
                let p = l * 256 + i * 2;
                *cell = ((buf[p] as u16) << 8) | (buf[p + 1] as u16);
            }
        }
        Ok(layers)
    }

    pub fn set_key(&self, layer: u8, row: u8, col: u8, keycode: u16) -> Result<(), String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        Self::via_cmd(&h, &[0x05, layer, row, col, (keycode >> 8) as u8, (keycode & 0xFF) as u8])?;
        Ok(())
    }

    pub fn read_rgb(&self) -> Result<RgbState, String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
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
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
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
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        Self::read_hw_config_with(&h)
    }

    fn read_hw_config_with(h: &impl HidTransport) -> Result<HardwareConfig, String> {
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
        let logo_rb = get(33)? != 0;
        let logo_idx = get(34)?.min(44);
        let side_rb = get(37)? != 0;
        let side_idx = get(38)?.min(44);
        Ok(HardwareConfig {
            win_lock: get(21)? != 0,
            mac_mode: os_probe != 0,
            low_latency: get(29)? != 0,
            logo_mode: get(32)?,
            logo_color: if logo_rb { 0xFF } else { logo_idx },
            logo_on: get(35)? == 0,
            side_mode: get(36)?,
            side_color: if side_rb { 0xFF } else { side_idx },
            side_on: get(39)? == 0,
            sys_rgb_on: get(40)? == 0,
            tapping_term_ms: if tt_units == 0 { 200 } else { (tt_units as u16) * 10 },
            battery: if bat_raw <= 100 { Some(bat_raw) } else { None },
            prosuite_supported: true,
        })
    }

    /// Single-session fast poll of both RGB and HardwareConfig (for real-time bidirectional sync).
    pub fn poll_live_state(&self) -> Result<(RgbState, HardwareConfig, u16), String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
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
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
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
        if count == 0 || self.is_ota_busy() {
            return;
        }
        let _g = self.lock_hid();
        if let Ok((h, _)) = self.open_via() {
            let _ = Self::via_cmd(&h, &[0x07, 0x03, 0xFD, b, g, r, count.min(6), 0]);
        }
    }

    pub fn read_macros(&self) -> Result<(Vec<String>, Vec<u8>), String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        let mut buf = Vec::with_capacity(MACRO_BYTES);
        let mut offset = 0usize;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let r = Self::via_cmd(&h, &[0x0E, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8])?;
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

    /// Write a raw 512-byte VIA macro buffer directly to the keyboard.
    pub fn write_macro_buffer(&self, raw: &[u8]) -> Result<(), String> {
        self.ensure_not_ota_busy()?;
        if raw.len() != MACRO_BYTES {
            return Err(format!("宏缓冲区长度无效: 期望 {} 字节，实际 {}", MACRO_BYTES, raw.len()));
        }
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        let mut offset = 0usize;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let mut req = vec![0x0F, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&raw[offset..offset + size]);
            Self::via_cmd(&h, &req)?;
            offset += size;
        }
        Ok(())
    }

    pub fn export_snapshot_for(
        &self,
        timestamp: String,
        edition: BoardEdition,
    ) -> Result<FullSnapshot, String> {
        self.ensure_not_ota_busy()?;
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
            edition: Some(edition),
        })
    }

    pub fn export_snapshot(&self, timestamp: String) -> Result<FullSnapshot, String> {
        self.export_snapshot_for(timestamp, BoardEdition::ProRgb)
    }

    pub fn apply_snapshot(&self, snap: &FullSnapshot) -> Result<(), String> {
        self.apply_snapshot_checked(snap, None, true)
    }

    pub fn apply_snapshot_checked(
        &self,
        snap: &FullSnapshot,
        current_edition: Option<BoardEdition>,
        save_to_flash: bool,
    ) -> Result<(), String> {
        self.ensure_not_ota_busy()?;
        let _g = self.lock_hid();
        let (h, _) = self.open_via()?;
        Self::apply_snapshot_with(&h, snap, current_edition, save_to_flash)
    }

    pub(crate) fn apply_snapshot_with(
        h: &impl HidTransport,
        snap: &FullSnapshot,
        current_edition: Option<BoardEdition>,
        save_to_flash: bool,
    ) -> Result<(), String> {
        if let (Some(snap_ed), Some(cur_ed)) = (snap.edition, current_edition) {
            if snap_ed != cur_ed {
                return Err(format!(
                    "方案版型不匹配: 方案为 {:?}，当前键盘为 {:?}，已阻止写入以防键位错乱",
                    snap_ed, cur_ed
                ));
            }
        }
        let km = from_hex(&snap.keymap_hex)?;
        let mc = from_hex(&snap.macros_hex)?;
        if km.len() != KEYMAP_BYTES || mc.len() != MACRO_BYTES {
            return Err("方案文件长度无效".into());
        }
        let mut offset = 0usize;
        while offset < KEYMAP_BYTES {
            let size = CHUNK_SIZE.min(KEYMAP_BYTES - offset);
            let mut req = vec![0x13, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&km[offset..offset + size]);
            Self::via_cmd(h, &req)?;
            offset += size;
        }
        offset = 0;
        while offset < MACRO_BYTES {
            let size = CHUNK_SIZE.min(MACRO_BYTES - offset);
            let mut req = vec![0x0F, (offset >> 8) as u8, (offset & 0xFF) as u8, size as u8];
            req.extend_from_slice(&mc[offset..offset + size]);
            Self::via_cmd(h, &req)?;
            offset += size;
        }
        Self::via_cmd(h, &[0x07, 0x03, 1, snap.rgb.brightness.min(9)])?;
        Self::via_cmd(h, &[0x07, 0x03, 2, snap.rgb.effect.min(18)])?;
        Self::via_cmd(h, &[0x07, 0x03, 3, snap.rgb.speed.min(4)])?;
        if snap.rgb.rainbow {
            Self::via_cmd(h, &[0x07, 0x03, 5, 0xFF])?;
        } else {
            Self::via_cmd(h, &[0x07, 0x03, 4, snap.rgb.hue, snap.rgb.saturation])?;
        }
        if let Some(hw) = &snap.hw {
            if hw.prosuite_supported {
                Self::via_cmd(h, &[0x07, 0x03, 21, if hw.win_lock { 0xFF } else { 0x00 }])?;
                Self::via_cmd(h, &[0x07, 0x03, 26, if hw.mac_mode { 0x01 } else { 0x00 }])?;
                Self::via_cmd(h, &[0x07, 0x03, 29, if hw.low_latency { 0xFF } else { 0x00 }])?;
                Self::via_cmd(h, &[0x07, 0x03, 32, hw.logo_mode.min(3)])?;
                if hw.logo_color == 0xFF {
                    Self::via_cmd(h, &[0x07, 0x03, 33, 0xFF])?;
                } else {
                    Self::via_cmd(h, &[0x07, 0x03, 33, 0x00])?;
                    Self::via_cmd(h, &[0x07, 0x03, 34, hw.logo_color.min(44)])?;
                }
                Self::via_cmd(h, &[0x07, 0x03, 35, if hw.logo_on { 0x00 } else { 0x01 }])?;
                Self::via_cmd(h, &[0x07, 0x03, 36, hw.side_mode.min(3)])?;
                if hw.side_color == 0xFF {
                    Self::via_cmd(h, &[0x07, 0x03, 37, 0xFF])?;
                } else {
                    Self::via_cmd(h, &[0x07, 0x03, 37, 0x00])?;
                    Self::via_cmd(h, &[0x07, 0x03, 38, hw.side_color.min(44)])?;
                }
                Self::via_cmd(h, &[0x07, 0x03, 39, if hw.side_on { 0x00 } else { 0x01 }])?;
                Self::via_cmd(h, &[0x07, 0x03, 40, if hw.sys_rgb_on { 0x00 } else { 0x01 }])?;
                let tt_units = if hw.tapping_term_ms == 0 {
                    0
                } else {
                    (hw.tapping_term_ms / 10).clamp(1, 255) as u8
                };
                Self::via_cmd(h, &[0x07, 0x03, 60, tt_units])?;
            }
        }
        if save_to_flash {
            Self::via_cmd(h, &[0x09])?;
        }
        // Mint x2 physical Logo LED pulse to acknowledge Profile / AutoSwitch load
        let _ = Self::via_cmd(h, &[0x07, 0x03, 0xFD, 153, 211, 52, 2, 0]);
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
    if !s.len().is_multiple_of(2) {
        return Err("无效的 hex 长度".into());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    for i in (0..s.len()).step_by(2) {
        let b = u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "无效的 hex 字符".to_string())?;
        out.push(b);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keycodes::BoardEdition;
    use std::cell::RefCell;

    struct MockTransport {
        writes: RefCell<Vec<Vec<u8>>>,
        responses: RefCell<std::collections::VecDeque<Vec<u8>>>,
        echo_mode: bool,
    }

    impl MockTransport {
        fn with_responses(resps: Vec<Vec<u8>>) -> Self {
            Self {
                writes: RefCell::new(Vec::new()),
                responses: RefCell::new(resps.into()),
                echo_mode: false,
            }
        }

        fn echo() -> Self {
            Self {
                writes: RefCell::new(Vec::new()),
                responses: RefCell::new(std::collections::VecDeque::new()),
                echo_mode: true,
            }
        }
    }

    impl HidTransport for MockTransport {
        fn write_report(&self, data: &[u8]) -> Result<(), String> {
            self.writes.borrow_mut().push(data.to_vec());
            if self.echo_mode {
                self.responses.borrow_mut().push_back(data.to_vec());
            }
            Ok(())
        }

        fn read_report(&self, _timeout_ms: u32) -> Result<Vec<u8>, String> {
            self.responses
                .borrow_mut()
                .pop_front()
                .ok_or_else(|| "mock queue empty".into())
        }
    }

    #[test]
    fn test_via_cmd_rejects_mismatched_opcode() {
        let mut wrong = vec![0u8; 33];
        wrong[1] = 0x08; // sent 0x05, got 0x08 back
        let mock = MockTransport::with_responses(vec![wrong]);
        let err = Crush80Hid::via_cmd(&mock, &[0x05, 0, 0, 0, 0, 4]).unwrap_err();
        assert!(err.contains("0x05") && err.contains("0x08"), "unexpected err: {err}");
    }

    #[test]
    fn test_apply_snapshot_rejects_cross_edition() {
        let snap = FullSnapshot {
            timestamp: "2026-11-25".into(),
            keymap_hex: "00".repeat(KEYMAP_BYTES),
            macros_hex: "00".repeat(MACRO_BYTES),
            rgb: RgbState::default(),
            hw: None,
            edition: Some(BoardEdition::ProRgb),
        };
        let mock = MockTransport::echo();
        let err = Crush80Hid::apply_snapshot_with(&mock, &snap, Some(BoardEdition::LiteNoRgb), true)
            .unwrap_err();
        assert!(err.contains("版型") || err.contains("edition"), "unexpected err: {err}");
        assert!(mock.writes.borrow().is_empty());
    }

    #[test]
    fn test_apply_snapshot_ram_only_skips_save_0x09() {
        let hw = HardwareConfig {
            prosuite_supported: true,
            logo_mode: 99, // out-of-range, must clamp to 3
            side_mode: 42, // out-of-range, must clamp to 3
            ..Default::default()
        };
        let snap = FullSnapshot {
            timestamp: "2026-11-25".into(),
            keymap_hex: "00".repeat(KEYMAP_BYTES),
            macros_hex: "00".repeat(MACRO_BYTES),
            rgb: RgbState::default(),
            hw: Some(hw),
            edition: Some(BoardEdition::ProRgb),
        };
        let mock = MockTransport::echo();
        Crush80Hid::apply_snapshot_with(&mock, &snap, Some(BoardEdition::ProRgb), false).unwrap();
        let writes = mock.writes.borrow();
        // Zero 0x09 (save-to-flash) packets when save_to_flash == false
        assert!(!writes.iter().any(|p| p.get(1) == Some(&0x09)));
        // Check clamped logo_mode (CONFIG[32]) & side_mode (CONFIG[36])
        let logo_pkt = writes.iter().find(|p| p[1..4] == [0x07, 0x03, 32]).unwrap();
        let side_pkt = writes.iter().find(|p| p[1..4] == [0x07, 0x03, 36]).unwrap();
        assert_eq!(logo_pkt[4], 3);
        assert_eq!(side_pkt[4], 3);
    }

    #[test]
    fn test_snapshot_backward_compat_without_edition() {
        let json = r#"{
            "timestamp": "2026-01-01",
            "keymap_hex": "0000",
            "macros_hex": "0000",
            "rgb": {"brightness": 5, "effect": 2, "speed": 2, "hue": 0, "saturation": 255}
        }"#;
        let snap: FullSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(snap.edition, None);
        assert!(snap.hw.is_none());
        assert!(!snap.rgb.rainbow);
    }

    #[test]
    fn test_hex_roundtrip_and_invalid_hex() {
        let src = vec![0x00, 0x0A, 0xFF, 0x7F];
        assert_eq!(from_hex(&to_hex(&src)).unwrap(), src);
        assert!(from_hex("abc").is_err());
        assert!(from_hex("zz").is_err());
    }

    #[test]
    fn test_ota_busy_blocks_all_via_ops() {
        let hid = Crush80Hid::new();
        assert!(hid.try_begin_ota());
        assert!(hid.read_keymap().unwrap_err().contains("OTA"));
        assert!(hid.set_key(0, 0, 0, 4).unwrap_err().contains("OTA"));
        assert!(hid.set_rgb_partial(Some(5), None, None, None, None, false).unwrap_err().contains("OTA"));
        assert!(hid.set_config_byte(29, 0xFF, false).unwrap_err().contains("OTA"));
        assert!(hid.write_macro_buffer(&[0u8; MACRO_BYTES]).unwrap_err().contains("OTA"));
        let snap = FullSnapshot {
            timestamp: "t".into(),
            keymap_hex: "00".repeat(KEYMAP_BYTES),
            macros_hex: "00".repeat(MACRO_BYTES),
            rgb: RgbState::default(),
            hw: None,
            edition: None,
        };
        assert!(hid.apply_snapshot_checked(&snap, None, false).unwrap_err().contains("OTA"));
        hid.end_ota();
    }
}
