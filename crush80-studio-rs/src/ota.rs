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
use crate::keycodes::BoardEdition;
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
    pub sha256: String,
    pub sha256_verified: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OtaFlashPolicy {
    pub detected_edition: Option<BoardEdition>,
    pub allow_unknown_source_crc: bool,
    pub allow_unknown_image: bool,
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

/// Pure-Rust SHA-256 (FIPS 180-4) returning lowercase 64-char hex.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = Vec::with_capacity((data.len() + 9).div_ceil(64) * 64);
    msg.extend_from_slice(data);
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut hex = String::with_capacity(64);
    for word in h {
        hex.push_str(&format!("{:08x}", word));
    }
    hex
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
        0x676B_6326 => Some("Ember"),
        _ => None,
    }
}

/// Known-good SHA-256 digests for verified Crush 80 Pro RGB firmware binaries.
pub fn known_firmware_sha256(crc: u32) -> Option<&'static str> {
    match crc {
        0x2F4F_4974 => Some("3b6c6755b1a3b263d6e55c2a4415d7e91689ac43f897b3be970b236ef970de3a"),
        0x0AAC_EB06 => Some("56125ed4ec7a6edf5e05780f8fc86055863ecc39d0bf3ead2dc21bfce8a3e11c"),
        0x676B_6326 => Some("58b8689c1d4af5e27dc32cf4646f2d1921507d72f5d61cf0a9a9b611ebbb2aae"),
        _ => None,
    }
}

/// Returns true if `crc` belongs to a verified Crush 80 Pro RGB firmware family.
pub fn is_known_pro_rgb_source_crc(crc: u32) -> bool {
    known_firmware_name_opt(crc).is_some()
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
    let sha256 = sha256_hex(bytes);
    let sha256_verified = known_firmware_sha256(ota_crc)
        .map(|expected| expected.eq_ignore_ascii_case(&sha256))
        .unwrap_or(false);
    // If a known CRC has a pinned SHA-256 and the file does NOT match it, treat as untrusted/modified!
    let sha_mismatch = known_firmware_sha256(ota_crc).is_some() && !sha256_verified;
    if sha_mismatch {
        return Err(format!(
            "Firmware SHA-256 mismatch for known CRC 0x{:08X} (file may be tampered or corrupted)",
            ota_crc
        ));
    }
    let (name, is_known) = match known_firmware_name_opt(ota_crc) {
        Some(n) => (n.to_string(), true),
        None => (format!("Custom {:08X}", ota_crc), false),
    };
    Ok(FirmwareInfo {
        size: bytes.len(),
        ota_crc,
        name,
        is_known,
        sha256,
        sha256_verified,
    })
}

/// Core anti-brick guard executed inside `ota.rs` before sending Start OTA.
pub fn check_ota_policy(
    info: &FirmwareInfo,
    current_crc: u32,
    policy: &OtaFlashPolicy,
) -> Result<(), String> {
    if policy.detected_edition == Some(BoardEdition::LiteNoRgb) {
        return Err(
            "安全硬锁：检测到 Crush 80 Lite（无光版）硬件矩阵特征，已永久禁止刷入 Pro RGB 固件以防变砖"
                .into(),
        );
    }
    if !is_known_pro_rgb_source_crc(current_crc) && !policy.allow_unknown_source_crc {
        return Err(format!(
            "已拦截：当前设备固件 CRC 0x{:08X} 不在已知 Pro RGB 源固件白名单中（如需强制刷写请在确认弹窗勾选高风险选项）",
            current_crc
        ));
    }
    if !info.is_known && !policy.allow_unknown_image {
        return Err(format!(
            "已拦截：目标固件 {} (CRC 0x{:08X}) 不在已知受信任镜像列表中（如需刷入自定义固件请在确认弹窗勾选高风险选项）",
            info.name, info.ota_crc
        ));
    }
    Ok(())
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

#[derive(Debug, PartialEq, Eq)]
enum EndDecision {
    ExplicitSuccess,
    FatalError(String),
    VerifyAfterReboot(String),
}

fn evaluate_end_response(res: Result<OtaResponse, String>) -> EndDecision {
    match res {
        Ok(OtaResponse::Result(0)) => EndDecision::ExplicitSuccess,
        Ok(OtaResponse::Result(code)) => {
            EndDecision::FatalError(format!("Device rejected OTA finish: error {}", code))
        }
        Ok(other) => EndDecision::VerifyAfterReboot(format!(
            "Unexpected final OTA reply ({:?}); verifying after reboot...",
            other
        )),
        Err(e) => EndDecision::VerifyAfterReboot(format!(
            "End packet reply interrupted ({}); verifying after reboot...",
            e
        )),
    }
}

fn is_post_reboot_crc_verified(
    initial_crc: u32,
    expected_crc: u32,
    observed_crc: u32,
    got_explicit_ack: bool,
    saw_disconnect: bool,
) -> bool {
    if observed_crc != expected_crc {
        return false;
    }
    if expected_crc == initial_crc && !got_explicit_ack && !saw_disconnect {
        return false;
    }
    true
}

/// Spawn a background thread that validates and flashes `image` to a wired USB Crush 80 (PID 0x5055).
/// Pauses live HID polling via `Crush80Hid::is_ota_busy()` for the entire flash + reboot verification.
pub fn flash_firmware_async(
    hid: Arc<Crush80Hid>,
    image: Vec<u8>,
    policy: OtaFlashPolicy,
    progress: Arc<Mutex<OtaProgress>>,
) {
    if let Ok(mut p) = progress.lock() {
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
        // Returns Ok((initial_crc, got_explicit_ack)) or Err((should_verify_reboot, initial_crc, err_msg))
        let transfer_res: Result<(u32, bool), (bool, u32, String)> = (|| {
            let _hid_lock = hid.lock_hid();
            let (handle, matched) = hid.open_ota_handle().map_err(|e| (false, 0, e))?;
            if matched.pid != PID_USB {
                return Err((
                    false,
                    0,
                    format!(
                        "Refusing non-USB device PID 0x{:04X}; wired USB (0x5055) is required",
                        matched.pid
                    ),
                ));
            }

            // Query current version & CRC and enforce core anti-brick policy BEFORE Start OTA
            let (version, current_crc) = query_ota_info(&handle).map_err(|e| (false, 0, e))?;
            check_ota_policy(&info, current_crc, &policy)
                .map_err(|e| (false, current_crc, e))?;

            set_progress(
                &progress,
                0.04,
                format!(
                    "Connected (ver {:08X}, CRC {:08X}). Starting OTA...",
                    version, current_crc
                ),
            );

            // Start OTA
            let start_pkt =
                build_ota_packet(&[0x02, 0x02, 0x00, 0x01, 0xFF]).map_err(|e| (false, current_crc, e))?;
            handle.write_report(&start_pkt).map_err(|e| (false, current_crc, e))?;
            let start_resp = handle.read_report(10_000).map_err(|e| (false, current_crc, e))?;
            match parse_ota_response(&start_resp).map_err(|e| (false, current_crc, e))? {
                OtaResponse::OtaAck => {}
                OtaResponse::Result(code) if code != 0 => {
                    return Err((
                        false,
                        current_crc,
                        format!("Device rejected OTA start: error {}", code),
                    ));
                }
                other => {
                    return Err((
                        false,
                        current_crc,
                        format!("Unexpected response to OTA start: {:?}", other),
                    ));
                }
            }

            // Prepare 16-byte padded blocks (3 blocks per 64-byte HID packet)
            let count = image.len().div_ceil(16);
            let mut padded = Vec::with_capacity(count * 16);
            padded.extend_from_slice(&image);
            padded.resize(count * 16, 0xFF);

            let mut pkt_idx = 0usize;
            for first in (0..count).step_by(3) {
                let pkt = build_data_packet(&padded, first, count)
                    .map_err(|e| (false, current_crc, e))?;
                handle.write_report(&pkt).map_err(|e| (false, current_crc, e))?;
                let resp = handle.read_report(10_000).map_err(|e| (false, current_crc, e))?;
                match parse_ota_response(&resp).map_err(|e| (false, current_crc, e))? {
                    OtaResponse::OtaAck => {}
                    OtaResponse::Result(code) if code != 0 => {
                        return Err((
                            false,
                            current_crc,
                            format!("Device rejected block {}: OTA error {}", first, code),
                        ));
                    }
                    other => {
                        return Err((
                            false,
                            current_crc,
                            format!("Unexpected response at block {}: {:?}", first, other),
                        ));
                    }
                }
                pkt_idx += 1;
                let sent_blocks = (first + 3).min(count);
                if pkt_idx.is_multiple_of(16) || sent_blocks == count {
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
            let end_pkt = build_end_packet(count).map_err(|e| (false, current_crc, e))?;
            handle.write_report(&end_pkt).map_err(|e| (true, current_crc, e))?;
            let end_res = handle
                .read_report(10_000)
                .and_then(|resp| parse_ota_response(&resp));
            match evaluate_end_response(end_res) {
                EndDecision::ExplicitSuccess => Ok((current_crc, true)),
                EndDecision::FatalError(msg) => Err((false, current_crc, msg)),
                EndDecision::VerifyAfterReboot(msg) => Err((true, current_crc, msg)),
            }
        })();

        let (initial_crc, got_explicit_ack) = match transfer_res {
            Ok((icrc, ack)) => {
                set_progress(
                    &progress,
                    0.92,
                    "OTA accepted! Waiting for keyboard reboot & CRC verification...",
                );
                (icrc, ack)
            }
            Err((false, _, err)) => {
                fail_progress(&progress, err);
                return;
            }
            Err((true, icrc, err)) => {
                set_progress(
                    &progress,
                    0.92,
                    format!("{} Checking CRC after reboot...", err),
                );
                (icrc, false)
            }
        };

        // 3. Post-restart CRC verification (up to 20s on a fresh HID handle)
        let start_wait = Instant::now();
        let timeout = Duration::from_secs(20);
        let mut last_error = "No query completed".to_string();
        let mut saw_disconnect = false;

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
                Ok((_, crc))
                    if is_post_reboot_crc_verified(
                        initial_crc,
                        expected_crc,
                        crc,
                        got_explicit_ack,
                        saw_disconnect,
                    ) =>
                {
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
                    if crc == expected_crc {
                        last_error = format!(
                            "CRC {:08X} matches pre-flash image; waiting for reboot confirmation",
                            crc
                        );
                    } else {
                        last_error = format!("CRC {:08X}, expected {:08X}", crc, expected_crc);
                    }
                }
                Err(e) => {
                    saw_disconnect = true;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keycodes::BoardEdition;

    fn make_valid_synthetic_image(tag_byte: u8) -> Vec<u8> {
        let mut img = vec![0xFFu8; EXPECTED_IMAGE_LEN];
        img[24..28].copy_from_slice(&(EXPECTED_IMAGE_LEN as u32).to_le_bytes());
        img[32..36].copy_from_slice(b"KNLT");
        img[64] = tag_byte;
        let trail = !crc32_ieee(&img[..EXPECTED_IMAGE_LEN - 4]);
        img[EXPECTED_IMAGE_LEN - 4..].copy_from_slice(&trail.to_le_bytes());
        img
    }

    #[test]
    fn test_crc32_and_crc16_known_vectors() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    #[test]
    fn test_sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_validate_firmware_image_edge_cases() {
        let valid = make_valid_synthetic_image(0x42);
        let info = validate_firmware_image(&valid).unwrap();
        assert_eq!(info.size, EXPECTED_IMAGE_LEN);
        assert_eq!(info.sha256.len(), 64);
        assert!(!info.is_known);

        assert!(validate_firmware_image(&valid[..100]).is_err());

        let mut bad_magic = valid.clone();
        bad_magic[32] = b'X';
        assert!(validate_firmware_image(&bad_magic).is_err());

        let mut bad_size = valid.clone();
        bad_size[24] ^= 1;
        assert!(validate_firmware_image(&bad_size).is_err());

        let mut bad_crc = valid.clone();
        bad_crc[100] ^= 0xFF;
        assert!(validate_firmware_image(&bad_crc).is_err());
    }

    #[test]
    fn test_ota_packet_builders_and_parser() {
        let pkt = build_ota_packet(&[0x01, 0x00, 0x00]).unwrap();
        assert_eq!(pkt[0], 0x05);
        assert_eq!(&pkt[1..4], &[0x01, 0x00, 0x00]);
        assert!(pkt[4..].iter().all(|&b| b == 0xFF));

        let padded = vec![0xABu8; 48];
        let dp = build_data_packet(&padded, 0, 3).unwrap();
        assert_eq!(dp[0], 0x05);
        assert_eq!(dp[1], 0x02);
        assert_eq!(dp[2], 60); // 3 blocks * 20 bytes
        assert_eq!(dp[3], 0x00);

        let ep = build_end_packet(10).unwrap();
        assert_eq!(&ep[1..6], &[0x02, 0x06, 0x00, 0x02, 0xFF]);
        let last = u16::from_le_bytes([ep[6], ep[7]]);
        let neg_last = u16::from_le_bytes([ep[8], ep[9]]);
        assert_eq!(last, 9);
        assert_eq!(last.wrapping_add(neg_last), 0);

        assert_eq!(
            parse_ota_response(&[0x05, 0x02, 0x03, 0x00, 0x06, 0xFF, 0x00]).unwrap(),
            OtaResponse::Result(0)
        );
        assert_eq!(
            parse_ota_response(&[0x05, 0x02, 0x03, 0x00, 0x06, 0xFF, 0x07]).unwrap(),
            OtaResponse::Result(7)
        );
        assert_eq!(
            parse_ota_response(&[0x05, 0x01, 0x08, 0x00, 1, 0, 0, 0, 0x06, 0xEB, 0xAC, 0x0A]).unwrap(),
            OtaResponse::Info {
                version: 1,
                crc: 0x0AAC_EB06
            }
        );
        assert!(parse_ota_response(&[0x05, 0x01]).is_err());
    }

    #[test]
    fn test_check_ota_policy_anti_brick() {
        let known_info = FirmwareInfo {
            size: EXPECTED_IMAGE_LEN,
            ota_crc: 0x0AAC_EB06,
            name: "Ember".into(),
            is_known: true,
            sha256: "56125ed4ec7a6edf5e05780f8fc86055863ecc39d0bf3ead2dc21bfce8a3e11c".into(),
            sha256_verified: true,
        };
        let custom_info = FirmwareInfo {
            size: EXPECTED_IMAGE_LEN,
            ota_crc: 0x1234_5678,
            name: "Custom 12345678".into(),
            is_known: false,
            sha256: "00".repeat(32),
            sha256_verified: false,
        };

        // Lite Non-RGB is ALWAYS hard-blocked even if override flags are set
        let lite_policy = OtaFlashPolicy {
            detected_edition: Some(BoardEdition::LiteNoRgb),
            allow_unknown_source_crc: true,
            allow_unknown_image: true,
        };
        assert!(check_ota_policy(&known_info, 0xCA2D_85DC, &lite_policy).is_err());

        // Known Pro RGB source + known image passes under default strict policy
        let strict = OtaFlashPolicy::default();
        assert!(check_ota_policy(&known_info, 0xCA2D_85DC, &strict).is_ok());

        // Unknown source CRC rejected unless allow_unknown_source_crc
        assert!(check_ota_policy(&known_info, 0xDEAD_BEEF, &strict).is_err());
        let allow_src = OtaFlashPolicy {
            allow_unknown_source_crc: true,
            ..Default::default()
        };
        assert!(check_ota_policy(&known_info, 0xDEAD_BEEF, &allow_src).is_ok());

        // Unknown image rejected unless allow_unknown_image
        assert!(check_ota_policy(&custom_info, 0xCA2D_85DC, &strict).is_err());
        let allow_img = OtaFlashPolicy {
            allow_unknown_image: true,
            ..Default::default()
        };
        assert!(check_ota_policy(&custom_info, 0xCA2D_85DC, &allow_img).is_ok());
    }

    #[test]
    fn test_end_ota_explicit_nonzero_result_fails_immediately() {
        let decision = evaluate_end_response(Ok(OtaResponse::Result(5)));
        assert_eq!(decision, EndDecision::FatalError("Device rejected OTA finish: error 5".into()));
        let ack = evaluate_end_response(Ok(OtaResponse::Result(0)));
        assert_eq!(ack, EndDecision::ExplicitSuccess);
        let dropped = evaluate_end_response(Err("HID 读取超时".into()));
        assert!(matches!(dropped, EndDecision::VerifyAfterReboot(_)));
    }

    #[test]
    fn test_same_crc_reflash_requires_reboot_or_explicit_ack() {
        // Without explicit Result(0), seeing the same CRC before disconnect must NOT verify!
        assert!(!is_post_reboot_crc_verified(0x0AAC_EB06, 0x0AAC_EB06, 0x0AAC_EB06, false, false));
        // Once disconnect was observed, matching CRC verifies.
        assert!(is_post_reboot_crc_verified(0x0AAC_EB06, 0x0AAC_EB06, 0x0AAC_EB06, false, true));
        // With explicit Result(0), matching CRC verifies.
        assert!(is_post_reboot_crc_verified(0x0AAC_EB06, 0x0AAC_EB06, 0x0AAC_EB06, true, false));
        // Different CRC verifies immediately on match.
        assert!(is_post_reboot_crc_verified(0xCA2D_85DC, 0x0AAC_EB06, 0x0AAC_EB06, false, false));
    }
}
