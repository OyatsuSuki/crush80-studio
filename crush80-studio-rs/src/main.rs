#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autoswitch;
mod hid_win;
mod hid_worker;
mod i18n;
mod keycodes;
mod macro_codec;
mod ota;
mod storage;
mod tray;
mod ui;
mod win_sys;

use autoswitch::{load_config, spawn_watcher, AutoSwitchShared};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use hid_win::{Crush80Hid, DeviceStatus, HardwareConfig, RgbState};
use hid_worker::{HidCmd, HidEvent, HidWorkerHandle, PID_BLUETOOTH};
use i18n::{tr, Lang};
use keycodes::{
    decode_keycode_for, detect_edition_from_keymap, factory_default_key_for, BoardEdition,
    FACTORY_KEYMAP,
};
use macro_codec::{decode_macro_buffer, hid_to_name, MacroStep, MACRO_SLOT_COUNT};
use ota::{validate_firmware_image, FirmwareInfo, OtaProgress};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tray::{spawn_tray, TrayState};
use ui::theme::*;
use ui::widgets::*;

pub const PID_USB: u16 = 0x5055;

/// HID keycodes matching the 15 slots returned by `win_sys::poll_win32_special_keys()`:
/// `[LCtrl, RCtrl, LShift, RShift, LAlt, RAlt, LWin, RWin, CapsLock, PrtSc, ScrLk, Pause, Menu, F13, Mute]`.
pub const WIN32_SPECIAL_HIDS: [u8; win_sys::SPECIAL_KEY_COUNT] = [
    0xE0, 0xE4, 0xE1, 0xE5, 0xE2, 0xE6, 0xE3, 0xE7, 0x39, 0x46, 0x47, 0x48, 0x65, 0x68, 0xA8,
];

#[derive(Clone, Debug, Default)]
pub struct TesterKeyStats {
    pub count: u64,
    pub down_at: Option<Instant>,
    pub last_press: Option<Instant>,
    pub last_release: Option<Instant>,
    pub min_dt_ms: Option<u64>,
    pub min_bounce_ms: Option<u64>,
    pub last_hold_ms: Option<u64>,
    pub chatter_count: u64,
}

#[derive(Clone, Debug)]
pub struct TesterEvent {
    pub hid: u8,
    pub name: String,
    pub dt_ms: Option<u64>,
    pub hold_ms: Option<u64>,
    pub is_chatter: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NavTab {
    Keymap,
    Rgb,
    Hardware,
    Macros,
    Profiles,
    AutoSwitch,
    Tester,
    Ota,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PickerTab {
    ProBuilder,
    Basic,
    Layer,
    Wob,
    Macro,
}

pub struct Crush80App {
    pub hid: Arc<Crush80Hid>,
    pub worker: HidWorkerHandle,
    pub status: DeviceStatus,
    pub lang: Lang,
    pub settings_path: PathBuf,
    pub edition: BoardEdition,
    pub edition_path: PathBuf,
    pub tab: NavTab,
    pub picker_tab: PickerTab,
    pub start_time: Instant,
    // System tray & Windows startup
    pub tray: Arc<TrayState>,
    pub start_minimized_pending: bool,
    pub autostart_enabled: bool,
    // Keymap
    pub layers: [[u16; 128]; 4],
    pub current_layer: usize,
    pub selected_key: Option<(u8, u8)>,
    pub raw_hex_input: String,
    pub kc_search: String,
    // Tap-Hold builder
    pub mt_mod: u8,
    pub mt_key: u16,
    pub lt_layer: u8,
    pub lt_key: u16,
    pub mod_c: bool,
    pub mod_s: bool,
    pub mod_a: bool,
    pub mod_g: bool,
    pub mod_key: u16,
    // RGB & Hardware
    pub rgb: RgbState,
    pub hw: HardwareConfig,
    // Visual Macro Recorder
    pub macro_slots: Vec<Vec<MacroStep>>,
    pub active_macro_slot: usize,
    pub macro_recording: bool,
    pub macro_record_delays: bool,
    pub macro_last_event_time: Option<Instant>,
    pub macro_prev_special: [bool; win_sys::SPECIAL_KEY_COUNT],
    pub macro_text_input: String,
    pub macro_delay_input: u16,
    // Profiles
    pub profiles_dir: PathBuf,
    pub profile_names: Vec<String>,
    pub new_profile_name: String,
    // AutoSwitch
    pub autoswitch_path: PathBuf,
    pub autoswitch: Arc<Mutex<AutoSwitchShared>>,
    // Key Tester & Chatter Detector
    pub tester_pressed: HashSet<u8>,
    pub tester_tested: HashSet<u8>,
    pub tester_chatter_keys: HashSet<u8>,
    pub tester_stats: HashMap<u8, TesterKeyStats>,
    pub tester_prev_special: [bool; win_sys::SPECIAL_KEY_COUNT],
    pub tester_threshold_ms: u64,
    pub tester_total: u64,
    pub tester_chatter_count: u64,
    pub tester_min_dt_ms: Option<u64>,
    pub tester_log: VecDeque<TesterEvent>,
    // OTA Firmware Flasher
    pub ota_image_path: Option<PathBuf>,
    pub ota_image_bytes: Option<Vec<u8>>,
    pub ota_image_info: Option<Result<FirmwareInfo, String>>,
    pub ota_progress: Arc<Mutex<OtaProgress>>,
    pub ota_was_running: bool,
    pub ota_confirm_open: bool,
    pub ota_risk_acked: bool,
    // Toast
    pub toast: String,
    pub toast_is_err: bool,
    // Optional CLI screenshot capture (--capture-dir <dir>)
    pub capture_dir: Option<PathBuf>,
    pub capture_step: usize,
    pub capture_wait_frames: u8,
    pub capture_inflight: bool,
}

fn load_lang(path: &PathBuf) -> Lang {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Lang>(&s).ok())
        .unwrap_or(Lang::Zh)
}

fn save_lang(path: &PathBuf, lang: Lang) -> Result<(), String> {
    let s = serde_json::to_string(&lang).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| format!("保存语言设置失败: {}", e))
}

fn load_edition(path: &PathBuf) -> BoardEdition {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<BoardEdition>(&s).ok())
        .unwrap_or(BoardEdition::ProRgb)
}

fn save_edition(path: &PathBuf, ed: BoardEdition) -> Result<(), String> {
    let s = serde_json::to_string(&ed).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| format!("保存版型设置失败: {}", e))
}

impl Crush80App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_theme(&cc.egui_ctx);

        let data_dir = storage::resolve_data_dir();
        let profiles_dir = data_dir.join("profiles");
        let _ = std::fs::create_dir_all(&profiles_dir);
        let autoswitch_path = data_dir.join("autoswitch.json");
        let settings_path = data_dir.join("lang.json");
        let lang = load_lang(&settings_path);
        let edition_path = data_dir.join("edition.json");
        let edition = load_edition(&edition_path);

        let hid = Arc::new(Crush80Hid::new());
        let cfg = load_config(&autoswitch_path);
        let autoswitch = Arc::new(Mutex::new(AutoSwitchShared {
            cfg,
            active_rule_id: None,
            last_exe: String::new(),
        }));
        spawn_watcher(hid.clone(), autoswitch.clone(), profiles_dir.clone());

        let tray = TrayState::new(hid.clone());
        let s = tr(lang);
        tray.update_menu_labels(s.tray_show, s.tray_quit);
        spawn_tray(tray.clone());

        let worker = HidWorkerHandle::spawn(
            hid.clone(),
            tray.clone(),
            cc.egui_ctx.clone(),
            lang,
        );

        let mut app = Self {
            hid,
            worker,
            status: DeviceStatus::default(),
            lang,
            settings_path,
            edition,
            edition_path,
            tab: NavTab::Keymap,
            picker_tab: PickerTab::ProBuilder,
            start_time: Instant::now(),
            tray,
            start_minimized_pending: win_sys::has_minimized_arg(),
            autostart_enabled: win_sys::is_autostart_enabled(),
            layers: FACTORY_KEYMAP,
            current_layer: 0,
            selected_key: Some((3, 0)),
            raw_hex_input: String::new(),
            kc_search: String::new(),
            mt_mod: 1,
            mt_key: 0x29,
            lt_layer: 1,
            lt_key: 0x2C,
            mod_c: true,
            mod_s: true,
            mod_a: false,
            mod_g: false,
            mod_key: 0x19,
            rgb: RgbState::default(),
            hw: HardwareConfig {
                tapping_term_ms: 200,
                logo_on: true,
                side_on: true,
                sys_rgb_on: true,
                ..Default::default()
            },
            macro_slots: vec![Vec::new(); MACRO_SLOT_COUNT],
            active_macro_slot: 0,
            macro_recording: false,
            macro_record_delays: false,
            macro_last_event_time: None,
            macro_prev_special: [false; win_sys::SPECIAL_KEY_COUNT],
            macro_text_input: String::new(),
            macro_delay_input: 50,
            profiles_dir,
            profile_names: Vec::new(),
            new_profile_name: String::new(),
            autoswitch_path,
            autoswitch,
            tester_pressed: HashSet::new(),
            tester_tested: HashSet::new(),
            tester_chatter_keys: HashSet::new(),
            tester_stats: HashMap::new(),
            tester_prev_special: [false; win_sys::SPECIAL_KEY_COUNT],
            tester_threshold_ms: 80,
            tester_total: 0,
            tester_chatter_count: 0,
            tester_min_dt_ms: None,
            tester_log: VecDeque::new(),
            ota_image_path: None,
            ota_image_bytes: None,
            ota_image_info: None,
            ota_progress: Arc::new(Mutex::new(OtaProgress::default())),
            ota_was_running: false,
            ota_confirm_open: false,
            ota_risk_acked: false,
            toast: String::new(),
            toast_is_err: false,
            capture_dir: {
                let mut args = std::env::args().skip(1);
                let mut dir = None;
                while let Some(a) = args.next() {
                    if a == "--capture-dir" {
                        dir = args.next().map(PathBuf::from);
                    }
                }
                dir
            },
            capture_step: 0,
            capture_wait_frames: 6,
            capture_inflight: false,
        };
        app.refresh_profiles();
        app.sync_selected_hex();
        app.request_refresh_all();
        app
    }

    pub fn can_via_write(&self) -> bool {
        self.status.connected && self.status.pid != PID_BLUETOOTH && !self.hid.is_ota_busy()
    }

    pub fn can_ember_write(&self) -> bool {
        self.can_via_write() && self.hw.prosuite_supported
    }

    pub fn mark_ui_edit(&self) {
        self.worker.mark_ui_edit();
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        if let Err(e) = save_lang(&self.settings_path, lang) {
            self.set_toast(e, true);
        }
        let s = tr(lang);
        if self.status.connected && self.status.pid == PID_BLUETOOTH {
            self.status.firmware_name = s.bt_readonly_hint.into();
        }
        self.tray.update_menu_labels(s.tray_show, s.tray_quit);
        self.worker.set_lang(lang);
    }

    pub fn set_toast(&mut self, msg: impl Into<String>, is_err: bool) {
        self.toast = msg.into();
        self.toast_is_err = is_err;
    }

    pub fn refresh_profiles(&mut self) {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.profiles_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|s| s.to_str()) == Some("json") {
                    if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
        names.sort();
        self.profile_names = names;
    }

    pub fn request_refresh_all(&mut self) {
        self.refresh_profiles();
        self.worker.send(HidCmd::RefreshAll);
    }

    pub fn sync_selected_hex(&mut self) {
        if let Some((r, c)) = self.selected_key {
            let code = self.layers[self.current_layer][(r as usize) * 16 + (c as usize)];
            self.raw_hex_input = format!("0x{:04X}", code);
        }
    }

    pub fn restore_selected_key(&mut self) {
        let Some((r, c)) = self.selected_key else {
            return;
        };
        let def = factory_default_key_for(self.edition, self.current_layer, r, c);
        self.mark_ui_edit();
        self.worker.send(HidCmd::SetKey {
            layer: self.current_layer as u8,
            row: r,
            col: c,
            keycode: def,
        });
        self.worker.send(HidCmd::TriggerLedFeedback {
            count: 1,
            r: 56,
            g: 189,
            b: 248,
        });
        let s = tr(self.lang);
        let dec = decode_keycode_for(def, self.lang, self.edition);
        self.set_toast(format!("{} ({})", s.restored_key, dec.label), false);
    }

    pub fn restore_current_layer(&mut self) {
        let layer = self.current_layer;
        self.mark_ui_edit();
        let mut keys = Vec::new();
        for r in 0..8u8 {
            for c in 0..16u8 {
                let idx = (r as usize) * 16 + (c as usize);
                let def = factory_default_key_for(self.edition, layer, r, c);
                if self.layers[layer][idx] != def {
                    keys.push((r, c, def));
                }
            }
        }
        if keys.is_empty() {
            let s = tr(self.lang);
            self.set_toast(format!("L{} · {}", layer, s.restored_layer), false);
            return;
        }
        self.worker.send(HidCmd::RestoreLayer {
            layer: layer as u8,
            keys,
        });
    }

    pub fn load_ota_file(&mut self, path: PathBuf) {
        match std::fs::read(&path) {
            Ok(bytes) => {
                let res = validate_firmware_image(&bytes);
                self.ota_image_path = Some(path);
                self.ota_image_bytes = Some(bytes);
                self.ota_image_info = Some(res);
            }
            Err(e) => {
                self.ota_image_path = Some(path);
                self.ota_image_bytes = None;
                self.ota_image_info = Some(Err(e.to_string()));
            }
        }
    }

    pub fn reset_tester(&mut self) {
        self.tester_pressed.clear();
        self.tester_tested.clear();
        self.tester_chatter_keys.clear();
        self.tester_stats.clear();
        self.tester_prev_special = [false; win_sys::SPECIAL_KEY_COUNT];
        self.tester_total = 0;
        self.tester_chatter_count = 0;
        self.tester_min_dt_ms = None;
        self.tester_log.clear();
    }

    pub fn record_tester_press(&mut self, hid: u8, now: Instant) {
        if !self.tester_pressed.insert(hid) {
            return;
        }
        self.tester_tested.insert(hid);
        self.tester_total += 1;

        let entry = self.tester_stats.entry(hid).or_default();
        let press_dt = entry
            .last_press
            .map(|prev| now.saturating_duration_since(prev).as_millis() as u64);
        let bounce_dt = entry
            .last_release
            .map(|prev| now.saturating_duration_since(prev).as_millis() as u64);
        // Use the tighter of press-to-press or release-to-press bounce as effective gap
        let effective_dt = match (press_dt, bounce_dt) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        entry.count += 1;
        entry.down_at = Some(now);
        entry.last_press = Some(now);
        if let Some(d) = press_dt {
            entry.min_dt_ms = Some(entry.min_dt_ms.map_or(d, |cur| cur.min(d)));
        }
        if let Some(b) = bounce_dt {
            entry.min_bounce_ms = Some(entry.min_bounce_ms.map_or(b, |cur| cur.min(b)));
        }

        let is_chatter = effective_dt
            .map(|d| d < self.tester_threshold_ms)
            .unwrap_or(false);
        if is_chatter {
            entry.chatter_count += 1;
            self.tester_chatter_count += 1;
            self.tester_chatter_keys.insert(hid);
        }
        if let Some(d) = effective_dt {
            self.tester_min_dt_ms = Some(self.tester_min_dt_ms.map_or(d, |cur| cur.min(d)));
        }

        self.tester_log.push_front(TesterEvent {
            hid,
            name: hid_to_name(hid).to_string(),
            dt_ms: effective_dt,
            hold_ms: None,
            is_chatter,
        });
        while self.tester_log.len() > 60 {
            self.tester_log.pop_back();
        }
    }

    pub fn record_tester_release(&mut self, hid: u8, now: Instant) {
        self.tester_pressed.remove(&hid);
        if let Some(entry) = self.tester_stats.get_mut(&hid) {
            entry.last_release = Some(now);
            if let Some(down) = entry.down_at.take() {
                let hold = now.saturating_duration_since(down).as_millis() as u64;
                entry.last_hold_ms = Some(hold);
                if let Some(ev) = self
                    .tester_log
                    .iter_mut()
                    .find(|e| e.hid == hid && e.hold_ms.is_none())
                {
                    ev.hold_ms = Some(hold);
                }
            }
        }
    }

    pub fn push_recorded_macro_key(&mut self, hid: u8, pressed: bool, now: Instant) {
        let slot = &mut self.macro_slots[self.active_macro_slot];
        if self.macro_record_delays {
            if let Some(prev) = self.macro_last_event_time {
                let dt = now.saturating_duration_since(prev).as_millis().min(65535) as u16;
                if dt >= 15 {
                    slot.push(MacroStep::Delay(dt));
                }
            }
        }
        self.macro_last_event_time = Some(now);
        if pressed {
            slot.push(MacroStep::Down(hid));
        } else if matches!(slot.last(), Some(MacroStep::Down(prev_kc)) if *prev_kc == hid) {
            slot.pop();
            slot.push(MacroStep::Tap(hid));
        } else {
            slot.push(MacroStep::Up(hid));
        }
    }

    fn drain_worker_events(&mut self) {
        let s = tr(self.lang);
        let mut events = Vec::new();
        while let Ok(ev) = self.worker.rx.try_recv() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                HidEvent::FullRefresh {
                    mut status,
                    layers,
                    macros_raw,
                    rgb,
                    hw,
                    bt_info,
                    warnings,
                } => {
                    if status.connected && status.pid == PID_BLUETOOTH {
                        status.firmware_name = s.bt_readonly_hint.into();
                        if let Some(bt) = bt_info {
                            self.hw.battery = bt.battery;
                        }
                    } else if !status.connected {
                        self.hw.battery = None;
                    }
                    self.status = status;

                    if let Some(l) = layers {
                        self.layers = *l;
                        if let Some(detected) = detect_edition_from_keymap(&self.layers) {
                            if self.edition != detected {
                                self.edition = detected;
                                let _ = save_edition(&self.edition_path, detected);
                            }
                        }
                    }
                    if let Some(raw) = macros_raw {
                        self.macro_slots = decode_macro_buffer(&raw);
                    }
                    if let Some(r) = rgb {
                        self.rgb = r;
                    }
                    if let Some(h) = hw {
                        self.hw = h;
                    }
                    self.sync_selected_hex();

                    if !warnings.is_empty() {
                        self.set_toast(warnings.join(" | "), true);
                    } else if !self.status.connected {
                        self.set_toast(s.offline, true);
                    } else if self.status.pid == PID_BLUETOOTH {
                        self.set_toast(format!("{} · {}", s.online, s.bluetooth), false);
                    } else {
                        self.set_toast(
                            format!("{} · {}", s.online, self.status.firmware_name),
                            false,
                        );
                    }
                }
                HidEvent::LiveUpdate {
                    connected,
                    pid,
                    rgb,
                    hw,
                    bt_info,
                } => {
                    let was_connected = self.status.connected;
                    let old_pid = self.status.pid;
                    self.status.connected = connected;
                    self.status.pid = pid;
                    if connected {
                        if pid == PID_BLUETOOTH {
                            self.status.firmware_crc = None;
                            self.status.firmware_name = s.bt_readonly_hint.into();
                            if let Some(bt) = bt_info {
                                self.hw.battery = bt.battery;
                            } else if let Some(h) = hw {
                                self.hw.battery = h.battery;
                            }
                        } else {
                            if let Some(r) = rgb {
                                self.rgb = r;
                            }
                            if let Some(h) = hw {
                                self.hw = h;
                            }
                            if !was_connected || old_pid != pid {
                                self.status.firmware_crc = None;
                                self.request_refresh_all();
                                self.set_toast(s.reconnected, false);
                            }
                        }
                    } else {
                        self.status.firmware_crc = None;
                        self.hw.battery = None;
                    }
                }
                HidEvent::KeyUpdated {
                    layer,
                    row,
                    col,
                    keycode,
                } => {
                    let l = layer as usize;
                    if l < 4 {
                        self.layers[l][(row as usize) * 16 + (col as usize)] = keycode;
                        if l == self.current_layer && self.selected_key == Some((row, col)) {
                            self.raw_hex_input = format!("0x{:04X}", keycode);
                        }
                    }
                    let dec = decode_keycode_for(keycode, self.lang, self.edition);
                    self.set_toast(format!("L{} ({},{}) -> {}", layer, row, col, dec.name), false);
                }
                HidEvent::LayerRestored { layer, count } => {
                    for r in 0..8u8 {
                        for c in 0..16u8 {
                            let def = factory_default_key_for(self.edition, layer as usize, r, c);
                            self.layers[layer as usize][(r as usize) * 16 + (c as usize)] = def;
                        }
                    }
                    self.sync_selected_hex();
                    self.set_toast(
                        format!("L{} · {} ({})", layer, s.restored_layer, count),
                        false,
                    );
                }
                HidEvent::SnapshotExported { label } => {
                    self.refresh_profiles();
                    self.set_toast(label, false);
                }
                HidEvent::SnapshotApplied { label } => {
                    self.set_toast(format!("{} · {}", s.apply_prof, label), false);
                    self.request_refresh_all();
                }
                HidEvent::ToastOk(m) => self.set_toast(m, false),
                HidEvent::ToastErr(e) => self.set_toast(e, true),
            }
        }
    }
}

impl eframe::App for Crush80App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 1. Capture native HWND, handle --minimized startup, and handle System Tray restore
        self.tray.capture_main_hwnd();
        if self.start_minimized_pending {
            self.start_minimized_pending = false;
            tray::hide_main_window();
        }
        if self.tray.show_requested.swap(false, Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Optional built-in OpenGL framebuffer screenshot capture (--capture-dir <dir>)
        if let Some(dir) = self.capture_dir.clone() {
            let shots: &[(NavTab, &str)] = &[
                (NavTab::Keymap, "screenshot-keymap.raw"),
                (NavTab::Hardware, "screenshot-hardware.raw"),
                (NavTab::Tester, "screenshot-tester.raw"),
            ];
            let got_img = ctx.input(|i| {
                i.events.iter().find_map(|ev| match ev {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = got_img {
                if self.capture_step < shots.len() {
                    let _ = std::fs::create_dir_all(&dir);
                    let w = img.size[0] as u32;
                    let h = img.size[1] as u32;
                    let mut raw = Vec::with_capacity(8 + img.pixels.len() * 4);
                    raw.extend_from_slice(&w.to_le_bytes());
                    raw.extend_from_slice(&h.to_le_bytes());
                    for c in &img.pixels {
                        raw.extend_from_slice(&c.to_array());
                    }
                    let _ = std::fs::write(dir.join(shots[self.capture_step].1), raw);
                    self.capture_step += 1;
                    self.capture_inflight = false;
                    self.capture_wait_frames = 5;
                    if self.capture_step < shots.len() {
                        self.tab = shots[self.capture_step].0;
                    } else {
                        std::process::exit(0);
                    }
                }
            } else if self.capture_step < shots.len() {
                self.tab = shots[self.capture_step].0;
                if self.capture_wait_frames > 0 {
                    self.capture_wait_frames -= 1;
                } else if !self.capture_inflight {
                    self.capture_inflight = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
                }
            }
            ctx.request_repaint();
        }

        // 2. Track OTA completion even when user navigates away
        if self.ota_was_running {
            let (running, done, err) = self
                .ota_progress
                .lock()
                .map(|p| (p.running, p.done, p.error.clone()))
                .unwrap_or((false, false, None));
            if running {
                ctx.request_repaint_after(Duration::from_millis(60));
            } else if done || err.is_some() {
                self.ota_was_running = false;
                if let Some(e) = err {
                    self.set_toast(e, true);
                } else {
                    self.request_refresh_all();
                    let s = tr(self.lang);
                    self.set_toast(s.ota_success, false);
                }
            }
        }

        // 3. Drain non-blocking HidWorker events (zero UI-thread HID blocking!)
        self.drain_worker_events();

        // 4. Repaint cadence (throttle when unfocused or hidden to tray to save CPU)
        let is_focused = ctx.input(|i| i.focused);
        let is_hidden = self.tray.window_hidden.load(Ordering::Relaxed);
        if is_hidden {
            ctx.request_repaint_after(Duration::from_millis(500));
        } else if (self.tab == NavTab::Keymap || self.tab == NavTab::Rgb)
            && self.rgb.rainbow
            && self.rgb.effect != 0
        {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if is_focused && self.tab == NavTab::Tester {
            ctx.request_repaint_after(Duration::from_millis(8));
        } else if is_focused && self.macro_recording {
            ctx.request_repaint_after(Duration::from_millis(20));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        let s = tr(self.lang);

        // Paint opaque canvas background behind all panels so zero semi-transparent panel seams ever appear
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.screen_rect(),
            Rounding::ZERO,
            BG_CANVAS,
        );

        // --- Custom Frameless Dark Title Bar ---
        egui::TopBottomPanel::top("custom_titlebar")
            .exact_height(36.0)
            .frame(
                egui::Frame::none()
                    .fill(BG_RAIL)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(12.0, 0.0)),
            )
            .show(ctx, |ui| {
                let titlebar_rect = ui.max_rect();
                let drag_rect = Rect::from_min_max(
                    titlebar_rect.min,
                    Pos2::new(titlebar_rect.max.x - 132.0, titlebar_rect.max.y),
                );
                let drag_resp =
                    ui.interact(drag_rect, ui.id().with("tb_drag"), Sense::click_and_drag());
                if drag_resp.drag_started_by(egui::PointerButton::Primary) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                if drag_resp.double_clicked() {
                    let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                }

                ui.horizontal_centered(|ui| {
                    let (ic_r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(ic_r, Rounding::same(4.5), ACCENT);
                    p.rect_stroke(
                        ic_r.shrink(3.5),
                        Rounding::same(1.5),
                        Stroke::new(1.3_f32, BG_CANVAS),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("Crush 80 Studio")
                            .size(12.0)
                            .strong()
                            .color(TEXT_PRI),
                    );
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .size(10.5)
                            .color(TEXT_DIM),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if titlebar_win_btn(ui, 2) {
                            self.macro_recording = false;
                            self.tray.hide_main_window();
                            self.set_toast(s.minimized_tray, false);
                        }
                        if titlebar_win_btn(ui, 1) {
                            let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                        }
                        if titlebar_win_btn(ui, 0) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });

        // --- Sleek Left Navigation Rail ---
        egui::SidePanel::left("rail")
            .resizable(false)
            .exact_width(224.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::none()
                    .fill(BG_RAIL)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(16.0, 18.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, Rounding::same(9.0), ACCENT);
                    let inner = rect.shrink(7.0);
                    p.rect_stroke(inner, Rounding::same(3.0), Stroke::new(2.0_f32, BG_CANVAS));
                    p.text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "80",
                        egui::FontId::proportional(11.5),
                        BG_CANVAS,
                    );
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("CRUSH 80").size(15.5).strong().color(TEXT_PRI));
                        let sub = if self.edition == BoardEdition::LiteNoRgb {
                            s.subtitle_lite
                        } else {
                            s.subtitle_pro
                        };
                        ui.label(RichText::new(sub).size(9.5).color(TEXT_DIM));
                    });
                });

                ui.add_space(16.0);

                // Hardware Telemetry + Live Battery Gauge Card
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (dot_r, _) =
                                ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            let col = if !self.status.connected {
                                ROSE
                            } else if self.status.pid == PID_BLUETOOTH {
                                CYAN
                            } else {
                                MINT
                            };
                            ui.painter().circle_filled(dot_r.center(), 4.5, col);
                            if self.status.connected {
                                let halo = if self.status.pid == PID_BLUETOOTH {
                                    Color32::from_rgba_unmultiplied(56, 189, 248, 80)
                                } else {
                                    Color32::from_rgba_unmultiplied(52, 211, 153, 80)
                                };
                                ui.painter().circle_stroke(
                                    dot_r.center(),
                                    6.5,
                                    Stroke::new(1.0_f32, halo),
                                );
                            }
                            let conn = if self.status.pid == PID_USB {
                                s.usb_wired
                            } else if self.status.pid == PID_BLUETOOTH {
                                s.bluetooth
                            } else {
                                s.wireless_24g
                            };
                            let title = if self.status.connected {
                                format!("{} · {}", s.online, conn)
                            } else {
                                s.offline.into()
                            };
                            ui.label(RichText::new(title).size(12.0).strong().color(TEXT_PRI));
                        });
                        if self.status.connected {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(&self.status.firmware_name)
                                    .size(11.0)
                                    .color(ACCENT),
                            );

                            if let Some(pct) = self.hw.battery {
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(s.battery).size(10.5).color(TEXT_SEC));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let bcol = if pct >= 50 {
                                                MINT
                                            } else if pct >= 20 {
                                                ACCENT
                                            } else {
                                                ROSE
                                            };
                                            ui.label(
                                                RichText::new(format!("{}%", pct))
                                                    .size(11.0)
                                                    .strong()
                                                    .color(bcol),
                                            );
                                        },
                                    );
                                });
                                let (bar_r, _) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 5.0),
                                    Sense::hover(),
                                );
                                let p = ui.painter();
                                p.rect_filled(bar_r, Rounding::same(2.5), BG_SURFACE);
                                let fill_w = bar_r.width() * (pct.min(100) as f32 / 100.0);
                                let bcol = if pct >= 50 {
                                    MINT
                                } else if pct >= 20 {
                                    ACCENT
                                } else {
                                    ROSE
                                };
                                p.rect_filled(
                                    Rect::from_min_size(
                                        bar_r.min,
                                        Vec2::new(fill_w.max(4.0), bar_r.height()),
                                    ),
                                    Rounding::same(2.5),
                                    bcol,
                                );
                            }
                        }

                        // Compact Board Edition Toggle (Pro RGB / Lite Non-RGB)
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let w = (ui.available_width() - 6.0) / 2.0;
                            for (ed, lbl) in [
                                (BoardEdition::ProRgb, s.edition_pro),
                                (BoardEdition::LiteNoRgb, s.edition_lite),
                            ] {
                                let act = self.edition == ed;
                                let b = egui::Button::new(
                                    RichText::new(lbl)
                                        .size(10.5)
                                        .strong()
                                        .color(if act { BG_CANVAS } else { TEXT_SEC }),
                                )
                                .fill(if act {
                                    if ed == BoardEdition::ProRgb {
                                        ACCENT
                                    } else {
                                        CYAN
                                    }
                                } else {
                                    BG_SURFACE
                                })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if act {
                                        if ed == BoardEdition::ProRgb {
                                            ACCENT
                                        } else {
                                            CYAN
                                        }
                                    } else {
                                        HAIRLINE
                                    },
                                ))
                                .rounding(Rounding::same(5.0))
                                .min_size(Vec2::new(w, 22.0));
                                if ui.add(b).clicked() && self.edition != ed {
                                    self.edition = ed;
                                    if let Err(e) = save_edition(&self.edition_path, ed) {
                                        self.set_toast(e, true);
                                    }
                                }
                            }
                        });
                    });

                ui.add_space(14.0);
                ui.label(RichText::new(s.workspace).size(10.5).color(TEXT_DIM));
                ui.add_space(6.0);

                let rgb_nav_lbl = if self.edition == BoardEdition::LiteNoRgb {
                    s.nav_logo_light
                } else {
                    s.nav_rgb
                };
                let items = [
                    (NavTab::Keymap, s.nav_keymap, 0u8),
                    (NavTab::Rgb, rgb_nav_lbl, 1u8),
                    (NavTab::Hardware, s.nav_hardware, 5u8),
                    (NavTab::Macros, s.nav_macros, 2u8),
                    (NavTab::Profiles, s.nav_profiles, 3u8),
                    (NavTab::AutoSwitch, s.nav_autoswitch, 4u8),
                    (NavTab::Tester, s.nav_tester, 6u8),
                    (NavTab::Ota, s.nav_ota, 7u8),
                ];
                for (tab, label, icon_id) in items {
                    if nav_rail_button(ui, label, icon_id, self.tab == tab) {
                        if tab != NavTab::Macros {
                            self.macro_recording = false;
                        }
                        self.tab = tab;
                    }
                    ui.add_space(3.0);
                }

                // Bottom Footer: 3-Language Switcher + Rescan + Toast
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 16.0) / 3.0;
                        for (lg, lbl) in [(Lang::Zh, "中"), (Lang::En, "EN"), (Lang::Ja, "日")] {
                            let act = self.lang == lg;
                            let b = egui::Button::new(
                                RichText::new(lbl)
                                    .size(11.5)
                                    .strong()
                                    .color(if act { BG_CANVAS } else { TEXT_SEC }),
                            )
                            .fill(if act { ACCENT } else { BG_CARD })
                            .stroke(Stroke::new(1.0_f32, if act { ACCENT } else { HAIRLINE }))
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(w, 26.0));
                            if ui.add(b).clicked() {
                                self.set_lang(lg);
                            }
                        }
                    });

                    ui.add_space(8.0);
                    let w = ui.available_width();
                    ui.allocate_ui_with_layout(
                        Vec2::new(w, 32.0),
                        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                        |ui| {
                            let ref_btn = egui::Button::new(
                                RichText::new(s.rescan).size(12.0).color(TEXT_PRI),
                            )
                            .fill(BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, HAIRLINE))
                            .rounding(Rounding::same(8.0))
                            .min_size(Vec2::new(w, 32.0));
                            if ui.add(ref_btn).clicked() {
                                self.request_refresh_all();
                            }
                        },
                    );

                    if !self.toast.is_empty() {
                        ui.add_space(8.0);
                        egui::Frame::none()
                            .fill(BG_CARD)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if self.toast_is_err { ROSE } else { HAIRLINE },
                            ))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(&self.toast).size(10.5).color(
                                        if self.toast_is_err { ROSE } else { TEXT_SEC },
                                    ),
                                );
                            });
                    }
                });
            });

        // --- Main Workspace ---
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG_CANVAS)
                    .inner_margin(egui::Margin::symmetric(20.0, 14.0)),
            )
            .show(ctx, |ui| match self.tab {
                NavTab::Keymap => self.ui_keymap(ui),
                NavTab::Rgb => self.ui_rgb(ui),
                NavTab::Hardware => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_hardware(ui));
                }
                NavTab::Macros => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_macros(ui));
                }
                NavTab::Profiles => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_profiles(ui));
                }
                NavTab::AutoSwitch => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_autoswitch(ui));
                }
                NavTab::Tester => self.ui_tester(ui),
                NavTab::Ota => {
                    egui::ScrollArea::vertical().show(ui, |ui| self.ui_ota(ui));
                }
            });
    }
}

fn main() -> eframe::Result<()> {
    let _instance_guard = match win_sys::acquire_single_instance() {
        Some(g) => g,
        None => return Ok(()),
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Crush 80 Studio")
            .with_decorations(false)
            .with_icon(build_app_icon())
            .with_inner_size([1240.0, 810.0])
            .with_min_inner_size([1060.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Crush 80 Studio",
        options,
        Box::new(|cc| Ok(Box::new(Crush80App::new(cc)))),
    )
}
