//! Pure-Rust Telink wired USB OTA firmware validator and flasher for WOBKEY Crush 80 Pro Reboot.
//!
//! Protocol matches `wobkey-crush80-reboot-pro-firmware/tools/crush80.py` and `flash_win.py`:
//! - Interface: VID 0x320F, PID 0x5055 (USB wired ONLY — 2.4G dongle 0x5088 is strictly refused)
//! - Usage Page: 0xFFEF, Report ID 5, 64-byte packets padded with 0xFF
//! - Info query: `[0x01, 0x00, 0x00]` -> `[0x05, 0x01, 0x08, 0x00, ver(u32 LE), crc(u32 LE)]`
//! - Start OTA:  `[0x02, 0x02, 0x00, 0x01, 0xFF]`
//! - Data pkt:   `[0x02, len, 0x00]` + up to 3 blocks of `[idx(u16 LE) | 16B data | crc16_modbus(u16 LE)]`
//! - End OTA:    `[0x02, 0x06, 0x00, 0x02, 0xFF, last(u16 LE), (-last & 0xFFFF)(u16 LE)]`
//! - Final ACK:  `[0x05, 0x02, 0x03, 0x00, 0x06, 0xFF, 0x00]` followed by keyboard reboot & CRC re-query.

use crate::hid_win::{Crush80Hid, WinHidHandle};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const EXPECTED_IMAGE_LEN: usize = 124_436;
const PID_USB: u16 = 0x5055;
const OTA_REPORT_ID: u8 = 0x05;
const OTA_PACKET_SIZE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareInfo {
    pub size: usize,
    pub ota_crc: u32,
    pub name: String,
    pub is_known: bool,
}

#[derive(Clone, Debug, Default)]
pub struct OtaProgress {
    pub running: bool,
    pub fraction: f32,
    pub stage: String,
    pub done: bool,
    pub error: Option<String>,
    pub new_crc: Option<u32>,
}

/// Pure-Rust IEEE 802.3 CRC-32 (identical to `zlib.crc32`).
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Modbus / Telink OTA CRC-16 (init 0xFFFF, reflected poly 0xA001).
fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            if (crc & 1) != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

pub fn known_firmware_name_opt(crc: u32) -> Option<&'static str> {
    match crc {
        0xCA2D_85DC => Some("Official WOBLAB 0108"),
        0xFE1A_A4C2 => Some("Community RC2"),
        0x350A_D5C5 => Some("NativeHeld preview"),
        0x239D_8875 => Some("NativeHeld refactored"),
        0x1BF4_AA83 => Some("NativeHeld QMK/VIA Tap-Hold"),
        0x13DA_DB12 => Some("NativeHeld QMK/VIA Watchdog"),
        0x2F4F_4974 => Some("NativeHeld"),
        0xAF71_3C2C => Some("Ember"),
        0x99FC_FDBD => Some("Ember"),
        0x4859_A8C5 => Some("Ember"),
        0xC03D_5821 => Some("Ember"),
        0x6C8D_7399 => Some("Ember"),
        0x0AAC_EB06 => Some("Ember"),
        _ => None,
    }
}

pub fn known_firmware_name(crc: u32) -> String {
    match known_firmware_name_opt(crc) {
        Some(n) => n.to_string(),
        None => format!("Custom {:08X}", crc),
    }
}

/// Validate a raw Crush 80 Telink firmware `.bin` image before flashing.
pub fn validate_firmware_image(bytes: &[u8]) -> Result<FirmwareInfo, String> {
    if bytes.len() != EXPECTED_IMAGE_LEN {
        return Err(format!(
            "Invalid firmware size: expected {} bytes, got {} bytes",
            EXPECTED_IMAGE_LEN,
            bytes.len()
        ));
    }
    if &bytes[32..36] != b"KNLT" {
        return Err("Invalid Telink firmware header (missing KNLT magic at 32..36)".into());
    }
    let internal_size = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]) as usize;
    if internal_size != EXPECTED_IMAGE_LEN {
        return Err(format!(
            "Internal size mismatch: header declares {} bytes, expected {} bytes",
            internal_size, EXPECTED_IMAGE_LEN
        ));
    }
    let full_crc = crc32_ieee(bytes);
    if full_crc != 0xFFFF_FFFF {
        return Err(format!(
            "Full-image CRC-32 mismatch: got 0x{:08X}, expected 0xFFFFFFFF",
            full_crc
        ));
    }
    let ota_crc = u32::from_le_bytes([
        bytes[EXPECTED_IMAGE_LEN - 4],
        bytes[EXPECTED_IMAGE_LEN - 3],
        bytes[EXPECTED_IMAGE_LEN - 2],
        bytes[EXPECTED_IMAGE_LEN - 1],
    ]);
    let (name, is_known) = match known_firmware_name_opt(ota_crc) {
        Some(n) => (n.to_string(), true),
        None => (format!("Custom {:08X}", ota_crc), false),
    };
    Ok(FirmwareInfo {
        size: bytes.len(),
        ota_crc,
        name,
        is_known,
    })
}

#[derive(Debug, PartialEq, Eq)]
enum OtaResponse {
    Info { version: u32, crc: u32 },
    Result(u8),
    OtaAck,
}

fn build_ota_packet(payload: &[u8]) -> Result<[u8; OTA_PACKET_SIZE], String> {
    if payload.len() > OTA_PACKET_SIZE - 1 {
        return Err(format!("Oversized OTA payload: {} bytes", payload.len()));
    }
    let mut pkt = [0xFFu8; OTA_PACKET_SIZE];
    pkt[0] = OTA_REPORT_ID;
    pkt[1..1 + payload.len()].copy_from_slice(payload);
    Ok(pkt)
}

fn parse_ota_response(data: &[u8]) -> Result<OtaResponse, String> {
    if data.len() < 4 || data[0] != OTA_REPORT_ID {
        return Err(format!("Malformed HID OTA response (len={})", data.len()));
    }
    let size = u16::from_le_bytes([data[2], data[3]]) as usize;
    if size > 60 || data.len() < 4 + size {
        return Err(format!(
            "Malformed OTA payload length: size={}, resp_len={}",
            size,
            data.len()
        ));
    }
    if data.len() >= 7 && data[..6] == [0x05, 0x02, 0x03, 0x00, 0x06, 0xFF] {
        return Ok(OtaResponse::Result(data[6]));
    }
    if data.len() >= 12 && data[..4] == [0x05, 0x01, 0x08, 0x00] {
        let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let crc = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        return Ok(OtaResponse::Info { version, crc });
    }
    if data[1] == 0x02 {
        return Ok(OtaResponse::OtaAck);
    }
    Err(format!(
        "Unexpected OTA response type: {:02X} {:02X} {:02X} {:02X}",
        data[0], data[1], data[2], data[3]
    ))
}

fn query_ota_info(h: &WinHidHandle) -> Result<(u32, u32), String> {
    let pkt = build_ota_packet(&[0x01, 0x00, 0x00])?;
    h.write_report(&pkt)?;
    let resp = h.read_report(5000)?;
    match parse_ota_response(&resp)? {
        OtaResponse::Info { version, crc } => Ok((version, crc)),
        other => Err(format!("Expected OTA info response, got {:?}", other)),
    }
}

fn build_data_packet(padded: &[u8], first: usize, count: usize) -> Result<[u8; OTA_PACKET_SIZE], String> {
    let mut payload = Vec::with_capacity(63);
    payload.push(0x02);
    payload.push(0x00); // placeholder for data length
    payload.push(0x00);
    let end = (first + 3).min(count);
    for index in first..end {
        let mut block = [0u8; 18];
        block[0..2].copy_from_slice(&(index as u16).to_le_bytes());
        block[2..18].copy_from_slice(&padded[index * 16..index * 16 + 16]);
        let crc = crc16_modbus(&block);
        payload.extend_from_slice(&block);
        payload.extend_from_slice(&crc.to_le_bytes());
    }
    payload[1] = (payload.len() - 3) as u8;
    build_ota_packet(&payload)
}

fn build_end_packet(count: usize) -> Result<[u8; OTA_PACKET_SIZE], String> {
    let last = (count - 1) as u16;
    // Two's complement: (-last) & 0xFFFF (verified against stock C# ota_write & crush80.py)
    let neg_last = last.wrapping_neg();
    let last_le = last.to_le_bytes();
    let neg_le = neg_last.to_le_bytes();
    build_ota_packet(&[
        0x02, 0x06, 0x00, 0x02, 0xFF, last_le[0], last_le[1], neg_le[0], neg_le[1],
    ])
}

struct OtaBusyGuard {
    hid: Arc<Crush80Hid>,
}

impl Drop for OtaBusyGuard {
    fn drop(&mut self) {
        self.hid.end_ota();
    }
}

fn set_progress(progress: &Arc<Mutex<OtaProgress>>, fraction: f32, stage: impl Into<String>) {
    if let Ok(mut p) = progress.lock() {
        p.fraction = fraction.clamp(0.0, 1.0);
        p.stage = stage.into();
    }
}

fn fail_progress(progress: &Arc<Mutex<OtaProgress>>, err: String) {
    if let Ok(mut p) = progress.lock() {
        p.running = false;
        p.done = false;
        p.error = Some(err);
    }
}

/// Spawn a background thread that validates and flashes `image` to a wired USB Crush 80 (PID 0x5055).
/// Pauses live HID polling via `Crush80Hid::is_ota_busy()` for the entire flash + reboot verification.
pub fn flash_firmware_async(
    hid: Arc<Crush80Hid>,
    image: Vec<u8>,
    progress: Arc<Mutex<OtaProgress>>,
) {
    {
        let mut p = progress.lock().unwrap();
        p.running = true;
        p.fraction = 0.0;
        p.stage = "Validating firmware image...".into();
        p.done = false;
        p.error = None;
        p.new_crc = None;
    }

    std::thread::spawn(move || {
        if !hid.try_begin_ota() {
            fail_progress(&progress, "Another OTA flash is already in progress".into());
            return;
        }
        let _busy_guard = OtaBusyGuard { hid: hid.clone() };

        // 1. Validate firmware image
        let info = match validate_firmware_image(&image) {
            Ok(i) => i,
            Err(e) => {
                fail_progress(&progress, e);
                return;
            }
        };
        let expected_crc = info.ota_crc;

        set_progress(
            &progress,
            0.02,
            format!("Opening wired USB OTA ({:08X} {})...", expected_crc, info.name),
        );

        // 2. Open wired USB OTA interface and run transfer while holding the HID mutex
        let transfer_res: Result<(), (bool, String)> = (|| {
            let _hid_lock = hid.lock_hid();
            let (handle, matched) = hid.open_ota_handle().map_err(|e| (false, e))?;
            if matched.pid != PID_USB {
                return Err((
                    false,
                    format!(
                        "Refusing non-USB device PID 0x{:04X}; wired USB (0x5055) is required",
                        matched.pid
                    ),
                ));
            }

            // Query current version & CRC
            let (version, current_crc) = query_ota_info(&handle).map_err(|e| (false, e))?;
            set_progress(
                &progress,
                0.04,
                format!(
                    "Connected (ver {:08X}, CRC {:08X}). Starting OTA...",
                    version, current_crc
                ),
            );

            // Start OTA
            let start_pkt = build_ota_packet(&[0x02, 0x02, 0x00, 0x01, 0xFF]).map_err(|e| (false, e))?;
            handle.write_report(&start_pkt).map_err(|e| (false, e))?;
            let start_resp = handle.read_report(10_000).map_err(|e| (false, e))?;
            match parse_ota_response(&start_resp).map_err(|e| (false, e))? {
                OtaResponse::OtaAck => {}
                OtaResponse::Result(code) if code != 0 => {
                    return Err((false, format!("Device rejected OTA start: error {}", code)));
                }
                other => {
                    return Err((false, format!("Unexpected response to OTA start: {:?}", other)));
                }
            }

            // Prepare 16-byte padded blocks (3 blocks per 64-byte HID packet)
            let count = (image.len() + 15) / 16;
            let mut padded = Vec::with_capacity(count * 16);
            padded.extend_from_slice(&image);
            padded.resize(count * 16, 0xFF);

            let mut pkt_idx = 0usize;
            for first in (0..count).step_by(3) {
                let pkt = build_data_packet(&padded, first, count).map_err(|e| (false, e))?;
                handle.write_report(&pkt).map_err(|e| (false, e))?;
                let resp = handle.read_report(10_000).map_err(|e| (false, e))?;
                match parse_ota_response(&resp).map_err(|e| (false, e))? {
                    OtaResponse::OtaAck => {}
                    OtaResponse::Result(code) if code != 0 => {
                        return Err((
                            false,
                            format!("Device rejected block {}: OTA error {}", first, code),
                        ));
                    }
                    other => {
                        return Err((
                            false,
                            format!("Unexpected response at block {}: {:?}", first, other),
                        ));
                    }
                }
                pkt_idx += 1;
                let sent_blocks = (first + 3).min(count);
                if pkt_idx % 16 == 0 || sent_blocks == count {
                    let frac = 0.05 + 0.85 * (sent_blocks as f32 / count as f32);
                    set_progress(
                        &progress,
                        frac,
                        format!("Flashing {}/{} blocks...", sent_blocks, count),
                    );
                }
            }

            // End OTA
            set_progress(&progress, 0.91, "Finalizing OTA & verifying checksum...");
            let end_pkt = build_end_packet(count).map_err(|e| (true, e))?;
            handle.write_report(&end_pkt).map_err(|e| (true, e))?;
            let end_resp = handle.read_report(10_000).map_err(|e| (true, e))?;
            match parse_ota_response(&end_resp).map_err(|e| (true, e))? {
                OtaResponse::Result(0) => Ok(()),
                OtaResponse::Result(code) => Err((
                    true,
                    format!("Device rejected OTA finish: error {}", code),
                )),
                _ => Err((
                    true,
                    "No explicit final OTA success; verifying after reboot...".into(),
                )),
            }
        })();

        if let Err((reached_end, err)) = transfer_res {
            if !reached_end {
                fail_progress(&progress, err);
                return;
            }
            // If we already sent the end packet and the keyboard rebooted before replying,
            // fall through to post-restart CRC verification just like crush80.py.
            set_progress(
                &progress,
                0.92,
                format!("{} Checking CRC after reboot...", err),
            );
        } else {
            set_progress(
                &progress,
                0.92,
                "OTA accepted! Waiting for keyboard reboot & CRC verification...",
            );
        }

        // 3. Post-restart CRC verification (up to 20s on a fresh HID handle)
        let start_wait = Instant::now();
        let timeout = Duration::from_secs(20);
        let mut last_error = "No query completed".to_string();

        while start_wait.elapsed() < timeout {
            std::thread::sleep(Duration::from_secs(1));
            let elapsed_frac = (start_wait.elapsed().as_secs_f32() / 20.0).min(1.0);
            set_progress(
                &progress,
                0.92 + 0.07 * elapsed_frac,
                "Waiting for keyboard to reboot & verifying new CRC...",
            );

            let probe = {
                let _hid_lock = hid.lock_hid();
                hid.open_ota_handle()
                    .and_then(|(h, _)| query_ota_info(&h))
            };

            match probe {
                Ok((_, crc)) if crc == expected_crc => {
                    if let Ok(mut p) = progress.lock() {
                        p.running = false;
                        p.fraction = 1.0;
                        p.done = true;
                        p.error = None;
                        p.new_crc = Some(crc);
                        p.stage = format!("Verified! New CRC {:08X} ({})", crc, info.name);
                    }
                    return;
                }
                Ok((_, crc)) => {
                    last_error = format!("CRC {:08X}, expected {:08X}", crc, expected_crc);
                }
                Err(e) => {
                    last_error = e;
                }
            }
        }

        fail_progress(
            &progress,
            format!(
                "Post-restart CRC not verified: {}. Reconnect cable and check status.",
                last_error
            ),
        );
    });
}
