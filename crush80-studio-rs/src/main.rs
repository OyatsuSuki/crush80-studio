#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autoswitch;
mod hid_win;
mod i18n;
mod keycodes;
mod macro_codec;
mod ota;
mod tray;
mod win_sys;

use autoswitch::{load_config, save_config, spawn_watcher, AutoSwitchShared, SwitchRule};
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, Pos2, Rect, RichText, Rounding, Sense,
    Stroke, Vec2,
};
use hid_win::{Crush80Hid, DeviceStatus, FullSnapshot, HardwareConfig, RgbState};
use i18n::{rgb_effect_label, tr, Lang};
use keycodes::{
    decode_keycode_lang, factory_default_key, KeyCategory, BASIC_KEYCODES, CRUSH80_LAYOUT,
    FACTORY_KEYMAP, SPECIAL_KEYCODES,
};
use macro_codec::{
    decode_macro_buffer, egui_key_to_hid, encode_macro_buffer, encoded_byte_len, format_step,
    hid_to_name, MacroStep, MACRO_BUFFER_SIZE, MACRO_SLOT_COUNT,
};
use ota::{flash_firmware_async, validate_firmware_image, FirmwareInfo, OtaProgress};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tray::{spawn_tray, TrayState};

// --- Wootility x Keychron Industrial Dark Palette ---
const BG_CANVAS: Color32 = Color32::from_rgb(10, 12, 16);
const BG_RAIL: Color32 = Color32::from_rgb(14, 17, 23);
const BG_CARD: Color32 = Color32::from_rgb(19, 23, 32);
const BG_SURFACE: Color32 = Color32::from_rgb(26, 31, 43);
const BG_SURFACE_HOV: Color32 = Color32::from_rgb(35, 42, 58);
const HAIRLINE: Color32 = Color32::from_rgb(36, 43, 58);
const TEXT_PRI: Color32 = Color32::from_rgb(241, 245, 249);
const TEXT_SEC: Color32 = Color32::from_rgb(148, 163, 184);
const TEXT_DIM: Color32 = Color32::from_rgb(92, 104, 124);

const ACCENT: Color32 = Color32::from_rgb(245, 158, 11);
const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(45, 30, 8, 255);
const CYAN: Color32 = Color32::from_rgb(56, 189, 248);
const MINT: Color32 = Color32::from_rgb(52, 211, 153);
const VIOLET: Color32 = Color32::from_rgb(167, 139, 250);
const ROSE: Color32 = Color32::from_rgb(251, 113, 133);

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavTab {
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
enum PickerTab {
    ProBuilder,
    Basic,
    Layer,
    Wob,
    Macro,
}

struct LiveSyncState {
    connected: bool,
    pid: u16,
    rgb: RgbState,
    hw: HardwareConfig,
    revision: u64,
    last_ui_edit: Instant,
    lang: Lang,
}

struct Crush80App {
    hid: Arc<Crush80Hid>,
    status: DeviceStatus,
    lang: Lang,
    settings_path: PathBuf,
    tab: NavTab,
    picker_tab: PickerTab,
    start_time: Instant,
    // Live bidirectional sync + system tray + Windows startup
    live_sync: Arc<Mutex<LiveSyncState>>,
    applied_rev: u64,
    tray: Arc<TrayState>,
    start_minimized_pending: bool,
    autostart_enabled: bool,
    // Keymap
    layers: [[u16; 128]; 4],
    current_layer: usize,
    selected_key: Option<(u8, u8)>,
    raw_hex_input: String,
    kc_search: String,
    // Tap-Hold builder
    mt_mod: u8,
    mt_key: u16,
    lt_layer: u8,
    lt_key: u16,
    mod_c: bool,
    mod_s: bool,
    mod_a: bool,
    mod_g: bool,
    mod_key: u16,
    // RGB & Hardware
    rgb: RgbState,
    hw: HardwareConfig,
    // Visual Macro Recorder
    macro_slots: Vec<Vec<MacroStep>>,
    active_macro_slot: usize,
    macro_recording: bool,
    macro_record_delays: bool,
    macro_last_event_time: Option<Instant>,
    macro_prev_mods: egui::Modifiers,
    macro_text_input: String,
    macro_delay_input: u16,
    // Profiles
    profiles_dir: PathBuf,
    profile_names: Vec<String>,
    new_profile_name: String,
    // AutoSwitch
    autoswitch_path: PathBuf,
    autoswitch: Arc<Mutex<AutoSwitchShared>>,
    // Key Tester & Chatter Detector
    tester_pressed: HashSet<u8>,
    tester_tested: HashSet<u8>,
    tester_chatter_keys: HashSet<u8>,
    tester_last_press: HashMap<u8, Instant>,
    tester_prev_mods: egui::Modifiers,
    tester_total: u64,
    tester_chatter_count: u64,
    tester_last_dt_ms: Option<u64>,
    tester_log: VecDeque<(String, Option<u64>, bool)>,
    // OTA Firmware Flasher
    ota_image_path: Option<PathBuf>,
    ota_image_bytes: Option<Vec<u8>>,
    ota_image_info: Option<Result<FirmwareInfo, String>>,
    ota_progress: Arc<Mutex<OtaProgress>>,
    ota_was_running: bool,
    // Toast
    toast: String,
    toast_is_err: bool,
    // Optional CLI screenshot capture (--capture-dir <dir>)
    capture_dir: Option<PathBuf>,
    capture_step: usize,
    capture_wait_frames: u8,
    capture_inflight: bool,
}

fn load_lang(path: &PathBuf) -> Lang {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Lang>(&s).ok())
        .unwrap_or(Lang::Zh)
}

fn save_lang(path: &PathBuf, lang: Lang) {
    if let Ok(s) = serde_json::to_string(&lang) {
        let _ = std::fs::write(path, s);
    }
}

const PID_USB: u16 = 0x5055;
const PID_BLUETOOTH: u16 = 0xB700;

fn format_tray_tooltip(lang: Lang, connected: bool, pid: u16, battery: Option<u8>) -> String {
    let s = tr(lang);
    if !connected {
        return format!("Crush 80 Studio · {}", s.offline);
    }
    let conn = if pid == PID_USB {
        s.usb_wired
    } else if pid == PID_BLUETOOTH {
        s.bluetooth
    } else {
        s.wireless_24g
    };
    match battery {
        Some(pct) => format!("Crush 80 Studio · {} · {} {}%", conn, s.battery, pct),
        None => format!("Crush 80 Studio · {}", conn),
    }
}

fn spawn_live_sync(
    hid: Arc<Crush80Hid>,
    live: Arc<Mutex<LiveSyncState>>,
    tray: Arc<TrayState>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(1000));
        if hid.is_ota_busy() {
            continue;
        }
        let (skip_edit, lang) = {
            let st = live.lock().unwrap();
            (st.last_ui_edit.elapsed() < Duration::from_millis(1200), st.lang)
        };
        if skip_edit {
            continue;
        }
        match hid.poll_live_state() {
            Ok((rgb, hw, pid)) => {
                let tip = format_tray_tooltip(lang, true, pid, hw.battery);
                tray.update_tooltip(tip);
                let changed = {
                    let mut st = live.lock().unwrap();
                    if !st.connected || st.pid != pid || st.rgb != rgb || st.hw != hw {
                        st.connected = true;
                        st.pid = pid;
                        st.rgb = rgb;
                        st.hw = hw;
                        st.revision = st.revision.wrapping_add(1);
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    ctx.request_repaint();
                }
            }
            Err(_) => {
                if hid.is_ota_busy() {
                    continue;
                }
                if let Some(bt) = win_sys::query_bluetooth_crush80() {
                    let tip = format_tray_tooltip(lang, true, PID_BLUETOOTH, bt.battery);
                    tray.update_tooltip(tip);
                    let changed = {
                        let mut st = live.lock().unwrap();
                        if !st.connected || st.pid != PID_BLUETOOTH || st.hw.battery != bt.battery {
                            st.connected = true;
                            st.pid = PID_BLUETOOTH;
                            st.hw.battery = bt.battery;
                            st.revision = st.revision.wrapping_add(1);
                            true
                        } else {
                            false
                        }
                    };
                    if changed {
                        ctx.request_repaint();
                    }
                    continue;
                }
                let tip = format_tray_tooltip(lang, false, 0, None);
                tray.update_tooltip(tip);
                let changed = {
                    let mut st = live.lock().unwrap();
                    if st.connected {
                        st.connected = false;
                        st.pid = 0;
                        st.hw.battery = None;
                        st.revision = st.revision.wrapping_add(1);
                        true
                    } else {
                        false
                    }
                };
                if changed {
                    ctx.request_repaint();
                }
            }
        }
    });
}

impl Crush80App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        setup_theme(&cc.egui_ctx);

        let data_dir = resolve_data_dir();
        let profiles_dir = data_dir.join("profiles");
        let _ = std::fs::create_dir_all(&profiles_dir);
        let autoswitch_path = data_dir.join("autoswitch.json");
        let settings_path = data_dir.join("lang.json");
        let lang = load_lang(&settings_path);

        let hid = Arc::new(Crush80Hid::new());
        let cfg = load_config(&autoswitch_path);
        let autoswitch = Arc::new(Mutex::new(AutoSwitchShared {
            cfg,
            active_rule_id: None,
            last_exe: String::new(),
        }));
        spawn_watcher(hid.clone(), autoswitch.clone(), profiles_dir.clone());

        let tray = TrayState::new();
        let s = tr(lang);
        tray.update_menu_labels(s.tray_show, s.tray_quit);
        spawn_tray(tray.clone());

        let live_sync = Arc::new(Mutex::new(LiveSyncState {
            connected: false,
            pid: 0,
            rgb: RgbState::default(),
            hw: HardwareConfig::default(),
            revision: 0,
            last_ui_edit: Instant::now() - Duration::from_secs(5),
            lang,
        }));
        spawn_live_sync(hid.clone(), live_sync.clone(), tray.clone(), cc.egui_ctx.clone());

        let mut app = Self {
            hid,
            status: DeviceStatus::default(),
            lang,
            settings_path,
            tab: NavTab::Keymap,
            picker_tab: PickerTab::ProBuilder,
            start_time: Instant::now(),
            live_sync,
            applied_rev: 0,
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
            hw: HardwareConfig { tapping_term_ms: 200, logo_on: true, side_on: true, sys_rgb_on: true, ..Default::default() },
            macro_slots: vec![Vec::new(); MACRO_SLOT_COUNT],
            active_macro_slot: 0,
            macro_recording: false,
            macro_record_delays: false,
            macro_last_event_time: None,
            macro_prev_mods: egui::Modifiers::NONE,
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
            tester_last_press: HashMap::new(),
            tester_prev_mods: egui::Modifiers::NONE,
            tester_total: 0,
            tester_chatter_count: 0,
            tester_last_dt_ms: None,
            tester_log: VecDeque::new(),
            ota_image_path: None,
            ota_image_bytes: None,
            ota_image_info: None,
            ota_progress: Arc::new(Mutex::new(OtaProgress::default())),
            ota_was_running: false,
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
        app.refresh_all();
        app
    }

    fn mark_ui_edit(&self) {
        self.live_sync.lock().unwrap().last_ui_edit = Instant::now();
    }

    fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
        save_lang(&self.settings_path, lang);
        let s = tr(lang);
        if self.status.connected && self.status.pid == PID_BLUETOOTH {
            self.status.firmware_name = s.bt_readonly_hint.into();
        }
        self.tray.update_menu_labels(s.tray_show, s.tray_quit);
        {
            let mut st = self.live_sync.lock().unwrap();
            st.lang = lang;
        }
        let tip = format_tray_tooltip(lang, self.status.connected, self.status.pid, self.hw.battery);
        self.tray.update_tooltip(tip);
    }

    fn set_toast(&mut self, msg: impl Into<String>, is_err: bool) {
        self.toast = msg.into();
        self.toast_is_err = is_err;
    }

    fn refresh_profiles(&mut self) {
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

    fn refresh_all(&mut self) {
        self.refresh_profiles();
        self.status = self.hid.get_status();
        let s = tr(self.lang);
        if !self.status.connected {
            if let Some(bt) = win_sys::query_bluetooth_crush80() {
                self.status.connected = true;
                self.status.pid = PID_BLUETOOTH;
                self.status.firmware_name = s.bt_readonly_hint.into();
                self.hw.battery = bt.battery;
                self.tray.update_tooltip(format_tray_tooltip(self.lang, true, PID_BLUETOOTH, bt.battery));
                {
                    let mut st = self.live_sync.lock().unwrap();
                    st.connected = true;
                    st.pid = PID_BLUETOOTH;
                    st.hw.battery = bt.battery;
                }
                self.set_toast(format!("{} · {}", s.online, s.bluetooth), false);
                return;
            }
            self.hw.battery = None;
            self.set_toast(s.offline, true);
            self.tray.update_tooltip(format_tray_tooltip(self.lang, false, 0, None));
            return;
        }
        if let Ok(l) = self.hid.read_keymap() {
            self.layers = l;
        }
        if let Ok((rgb, hw, pid)) = self.hid.poll_live_state() {
            self.rgb = rgb.clone();
            self.hw = hw.clone();
            self.status.pid = pid;
            self.tray.update_tooltip(format_tray_tooltip(self.lang, true, pid, hw.battery));
            let mut st = self.live_sync.lock().unwrap();
            st.connected = true;
            st.pid = pid;
            st.rgb = rgb;
            st.hw = hw;
        }
        if let Ok((_, raw)) = self.hid.read_macros() {
            self.macro_slots = decode_macro_buffer(&raw);
        }
        self.sync_selected_hex();
        self.set_toast(format!("{} · {}", s.online, self.status.firmware_name), false);
    }

    fn sync_selected_hex(&mut self) {
        if let Some((r, c)) = self.selected_key {
            let code = self.layers[self.current_layer][(r as usize) * 16 + (c as usize)];
            self.raw_hex_input = format!("0x{:04X}", code);
        }
    }

    fn apply_keycode(&mut self, code: u16) {
        let Some((r, c)) = self.selected_key else {
            return;
        };
        self.mark_ui_edit();
        match self.hid.set_key(self.current_layer as u8, r, c, code) {
            Ok(()) => {
                self.layers[self.current_layer][(r as usize) * 16 + (c as usize)] = code;
                self.raw_hex_input = format!("0x{:04X}", code);
                let dec = decode_keycode_lang(code, self.lang);
                self.set_toast(format!("L{} ({},{}) -> {}", self.current_layer, r, c, dec.name), false);
            }
            Err(e) => self.set_toast(e, true),
        }
    }

    fn restore_selected_key(&mut self) {
        let Some((r, c)) = self.selected_key else {
            return;
        };
        let def = factory_default_key(self.current_layer, r, c);
        self.mark_ui_edit();
        match self.hid.set_key(self.current_layer as u8, r, c, def) {
            Ok(()) => {
                self.layers[self.current_layer][(r as usize) * 16 + (c as usize)] = def;
                self.sync_selected_hex();
                let s = tr(self.lang);
                let dec = decode_keycode_lang(def, self.lang);
                self.set_toast(format!("{} ({})", s.restored_key, dec.label), false);
            }
            Err(e) => self.set_toast(e, true),
        }
    }

    fn restore_current_layer(&mut self) {
        let layer = self.current_layer;
        self.mark_ui_edit();
        let mut first_err: Option<String> = None;
        for r in 0..8u8 {
            for c in 0..16u8 {
                let idx = (r as usize) * 16 + (c as usize);
                let def = FACTORY_KEYMAP[layer][idx];
                if self.layers[layer][idx] != def {
                    match self.hid.set_key(layer as u8, r, c, def) {
                        Ok(()) => {
                            self.layers[layer][idx] = def;
                        }
                        Err(e) => {
                            if first_err.is_none() {
                                first_err = Some(e);
                            }
                            break;
                        }
                    }
                }
            }
            if first_err.is_some() {
                break;
            }
        }
        self.sync_selected_hex();
        if let Some(e) = first_err {
            self.set_toast(e, true);
        } else {
            let s = tr(self.lang);
            self.set_toast(format!("L{} · {}", layer, s.restored_layer), false);
        }
    }

    fn load_ota_file(&mut self, path: PathBuf) {
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

    fn record_tester_press(&mut self, hid: u8, now: Instant) {
        self.tester_pressed.insert(hid);
        self.tester_tested.insert(hid);
        self.tester_total += 1;
        let dt_ms = self
            .tester_last_press
            .get(&hid)
            .map(|prev| now.saturating_duration_since(*prev).as_millis() as u64);
        self.tester_last_press.insert(hid, now);
        let is_chatter = dt_ms.map(|d| d < 50).unwrap_or(false);
        if is_chatter {
            self.tester_chatter_count += 1;
            self.tester_chatter_keys.insert(hid);
        }
        if let Some(d) = dt_ms {
            self.tester_last_dt_ms = Some(d);
        }
        self.tester_log
            .push_front((hid_to_name(hid).to_string(), dt_ms, is_chatter));
        while self.tester_log.len() > 24 {
            self.tester_log.pop_back();
        }
    }

    fn push_recorded_macro_key(&mut self, hid: u8, pressed: bool, now: Instant) {
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
            let (running, done, err) = {
                let p = self.ota_progress.lock().unwrap();
                (p.running, p.done, p.error.clone())
            };
            if running {
                ctx.request_repaint_after(Duration::from_millis(60));
            } else if done || err.is_some() {
                self.ota_was_running = false;
                if let Some(e) = err {
                    self.set_toast(e, true);
                } else {
                    self.refresh_all();
                    let s = tr(self.lang);
                    self.set_toast(s.ota_success, false);
                }
            }
        }

        // 3. Pull live hardware updates from background poller (bi-directional sync + hotplug auto-reconnect)
        if !self.hid.is_ota_busy() {
            let mut need_resync = false;
            {
                let st = self.live_sync.lock().unwrap();
                if st.revision != self.applied_rev {
                    self.applied_rev = st.revision;
                    let was_connected = self.status.connected;
                    let old_pid = self.status.pid;
                    self.status.connected = st.connected;
                    if st.connected {
                        let new_pid = st.pid;
                        self.status.pid = new_pid;
                        if new_pid == PID_BLUETOOTH {
                            self.status.firmware_name = tr(self.lang).bt_readonly_hint.into();
                            self.hw.battery = st.hw.battery;
                        } else {
                            self.rgb = st.rgb.clone();
                            self.hw = st.hw.clone();
                            if !was_connected || old_pid != new_pid {
                                need_resync = true;
                            }
                        }
                    } else {
                        self.status.pid = 0;
                        self.hw.battery = None;
                    }
                }
            }
            if need_resync {
                self.refresh_all();
                if self.status.connected {
                    let s = tr(self.lang);
                    self.set_toast(format!("{} · {}", s.reconnected, self.status.firmware_name), false);
                }
            }
        }

        // 4. Repaint cadence: smooth rainbow animation / Tester / OTA OR tray event polling (250ms)
        if (self.tab == NavTab::Keymap || self.tab == NavTab::Rgb) && self.rgb.rainbow && self.rgb.effect != 0 {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else if self.tab == NavTab::Tester || self.macro_recording {
            ctx.request_repaint_after(Duration::from_millis(33));
        } else {
            ctx.request_repaint_after(Duration::from_millis(250));
        }

        let s = tr(self.lang);

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
                let drag_resp = ui.interact(drag_rect, ui.id().with("tb_drag"), Sense::click_and_drag());
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
                    p.rect_stroke(ic_r.shrink(3.5), Rounding::same(1.5), Stroke::new(1.3_f32, BG_CANVAS));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Crush 80 Studio").size(12.0).strong().color(TEXT_PRI));

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Close button -> Minimize directly to System Tray via Win32 ShowWindow(SW_HIDE)!
                        if titlebar_win_btn(ui, 2) {
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
            .frame(
                egui::Frame::none()
                    .fill(BG_RAIL)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .inner_margin(egui::Margin::symmetric(16.0, 18.0)),
            )
            .show(ctx, |ui| {
                // Brand Header
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, Rounding::same(9.0), ACCENT);
                    let inner = rect.shrink(7.0);
                    p.rect_stroke(inner, Rounding::same(3.0), Stroke::new(2.0_f32, BG_CANVAS));
                    p.text(rect.center(), egui::Align2::CENTER_CENTER, "80", egui::FontId::proportional(11.5), BG_CANVAS);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("CRUSH 80").size(15.5).strong().color(TEXT_PRI));
                        ui.label(RichText::new("PRO REBOOT").size(9.5).color(TEXT_DIM));
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
                            let (dot_r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
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
                            ui.label(RichText::new(&self.status.firmware_name).size(11.0).color(ACCENT));

                            // Real-time Battery Gauge!
                            if let Some(pct) = self.hw.battery {
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(s.battery).size(10.5).color(TEXT_SEC));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        let bcol = if pct >= 50 { MINT } else if pct >= 20 { ACCENT } else { ROSE };
                                        ui.label(RichText::new(format!("{}%", pct)).size(11.0).strong().color(bcol));
                                    });
                                });
                                let (bar_r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 5.0), Sense::hover());
                                let p = ui.painter();
                                p.rect_filled(bar_r, Rounding::same(2.5), BG_SURFACE);
                                let fill_w = bar_r.width() * (pct.min(100) as f32 / 100.0);
                                let bcol = if pct >= 50 { MINT } else if pct >= 20 { ACCENT } else { ROSE };
                                p.rect_filled(
                                    Rect::from_min_size(bar_r.min, Vec2::new(fill_w.max(4.0), bar_r.height())),
                                    Rounding::same(2.5),
                                    bcol,
                                );
                            }
                        }
                    });

                ui.add_space(18.0);
                ui.label(RichText::new(s.workspace).size(10.5).color(TEXT_DIM));
                ui.add_space(6.0);

                let items = [
                    (NavTab::Keymap, s.nav_keymap, 0u8),
                    (NavTab::Rgb, s.nav_rgb, 1u8),
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
                    // Clean 3-way language pill (中 / EN / 日) — fixed height horizontal row (never ui.columns inside bottom_up!)
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 16.0) / 3.0;
                        for (lg, lbl) in [(Lang::Zh, "中"), (Lang::En, "EN"), (Lang::Ja, "日")] {
                            let act = self.lang == lg;
                            let b = egui::Button::new(
                                RichText::new(lbl).size(11.5).strong().color(if act { BG_CANVAS } else { TEXT_SEC }),
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
                    let ref_btn = egui::Button::new(RichText::new(s.rescan).size(12.0).color(TEXT_PRI))
                        .fill(BG_SURFACE)
                        .stroke(Stroke::new(1.0_f32, HAIRLINE))
                        .rounding(Rounding::same(8.0))
                        .min_size(Vec2::new(ui.available_width(), 32.0));
                    if ui.add(ref_btn).clicked() {
                        self.refresh_all();
                    }

                    if !self.toast.is_empty() {
                        ui.add_space(8.0);
                        egui::Frame::none()
                            .fill(BG_CARD)
                            .stroke(Stroke::new(1.0_f32, if self.toast_is_err { ROSE } else { HAIRLINE }))
                            .rounding(Rounding::same(8.0))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(&self.toast)
                                        .size(10.5)
                                        .color(if self.toast_is_err { ROSE } else { TEXT_SEC }),
                                );
                            });
                    }
                });
            });

        // --- Main Workspace ---
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(BG_CANVAS).inner_margin(egui::Margin::symmetric(20.0, 14.0)))
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

impl Crush80App {
    fn ui_keymap(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.keymap_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.keymap_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::same(4.0))
                    .show(ui, |ui| {
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let names = ["L0 Win", "L1 Win Fn", "L2 Mac", "L3 Mac Fn"];
                            for (i, n) in names.iter().enumerate() {
                                let active = self.current_layer == i;
                                let b = egui::Button::new(
                                    RichText::new(*n).size(12.0).strong().color(if active { BG_CANVAS } else { TEXT_SEC }),
                                )
                                .fill(if active { ACCENT } else { Color32::TRANSPARENT })
                                .stroke(Stroke::NONE)
                                .rounding(Rounding::same(6.0))
                                .min_size(Vec2::new(72.0, 26.0));
                                if ui.add(b).clicked() {
                                    self.current_layer = i;
                                    self.sync_selected_hex();
                                }
                            }
                        });
                    });
            });
        });

        ui.add_space(10.0);
        self.draw_keyboard_twin(ui, true);
        ui.add_space(10.0);

        ui.columns(2, |cols| {
            // Left: 4-Layer Inspector
            card_frame().show(&mut cols[0], |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.inspector).size(14.5).strong().color(TEXT_PRI));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((r, c)) = self.selected_key {
                            pill_badge(ui, &format!("R{} · C{}", r, c), CYAN);
                        }
                        if chip_button(ui, s.restore_layer, false) {
                            self.restore_current_layer();
                        }
                        if self.selected_key.is_some() && chip_button(ui, s.restore_key, false) {
                            self.restore_selected_key();
                        }
                    });
                });
                ui.add_space(10.0);

                if let Some((r, c)) = self.selected_key {
                    let idx = (r as usize) * 16 + (c as usize);
                    ui.columns(4, |lcols| {
                        for l in 0..4 {
                            let dec = decode_keycode_lang(self.layers[l][idx], self.lang);
                            let is_cur = l == self.current_layer;
                            egui::Frame::none()
                                .fill(if is_cur { ACCENT_SOFT } else { BG_SURFACE })
                                .stroke(Stroke::new(1.0_f32, if is_cur { ACCENT } else { HAIRLINE }))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 7.0))
                                .show(&mut lcols[l], |ui| {
                                    ui.label(RichText::new(format!("L{}", l)).size(10.0).color(if is_cur { ACCENT } else { TEXT_DIM }));
                                    ui.label(RichText::new(&dec.label).size(13.0).strong().color(TEXT_PRI));
                                    ui.label(RichText::new(&dec.name).size(9.5).color(TEXT_SEC));
                                });
                        }
                    });
                }

                ui.add_space(12.0);
                ui.label(RichText::new(s.presets).size(12.0).strong().color(TEXT_SEC));
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    for (label, code) in [
                        ("Caps -> Ctrl/Esc", 0x2129),
                        ("Space -> L1/Space", 0x412C),
                        ("CapsWord", 0x7C73),
                        ("Esc / ~", 0x7C16),
                        ("OSM Shift", 0x52A2),
                        ("OSL Layer 1", 0x5281),
                    ] {
                        if chip_button(ui, label, false) {
                            self.apply_keycode(code);
                        }
                    }
                });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.hex_write).size(12.0).color(TEXT_SEC));
                    ui.add(egui::TextEdit::singleline(&mut self.raw_hex_input).desired_width(100.0));
                    if primary_button(ui, s.apply, 76.0) {
                        let clean = self.raw_hex_input.trim().trim_start_matches("0x").trim_start_matches("0X");
                        if let Ok(v) = u16::from_str_radix(clean, 16) {
                            self.apply_keycode(v);
                        }
                    }
                });
            });

            // Right: Modular Key Drawer
            card_frame().show(&mut cols[1], |ui| {
                ui.horizontal(|ui| {
                    for (t, name) in [
                        (PickerTab::ProBuilder, s.tab_builder),
                        (PickerTab::Basic, s.tab_basic),
                        (PickerTab::Layer, s.tab_layer),
                        (PickerTab::Wob, s.tab_wob),
                        (PickerTab::Macro, s.tab_macro),
                    ] {
                        if chip_button(ui, name, self.picker_tab == t) {
                            self.picker_tab = t;
                        }
                    }
                });
                ui.add_space(12.0);

                match self.picker_tab {
                    PickerTab::ProBuilder => {
                        ui.label(RichText::new(s.mt_title).size(12.0).color(VIOLET).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("mt_m")
                                .selected_text(mod_label(self.mt_mod))
                                .width(115.0)
                                .show_ui(ui, |ui| {
                                    for (v, n) in [(1, "LCtrl"), (2, "LShift"), (4, "LAlt"), (8, "LWin"), (3, "Ctrl+Shift"), (15, "Hyper")] {
                                        ui.selectable_value(&mut self.mt_mod, v, n);
                                    }
                                });
                            egui::ComboBox::from_id_salt("mt_k")
                                .selected_text(decode_keycode_lang(self.mt_key, self.lang).label)
                                .width(95.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, l) in BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65) {
                                        ui.selectable_value(&mut self.mt_key, c, l);
                                    }
                                });
                            if primary_button(ui, s.apply, 72.0) {
                                self.apply_keycode(0x2000 | (((self.mt_mod as u16) & 0x1F) << 8) | (self.mt_key & 0xFF));
                            }
                        });

                        ui.add_space(10.0);
                        ui.label(RichText::new(s.lt_title).size(12.0).color(VIOLET).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("lt_l")
                                .selected_text(format!("Layer {}", self.lt_layer))
                                .width(115.0)
                                .show_ui(ui, |ui| {
                                    for l in 0..4u8 {
                                        ui.selectable_value(&mut self.lt_layer, l, format!("Layer {}", l));
                                    }
                                });
                            egui::ComboBox::from_id_salt("lt_k")
                                .selected_text(decode_keycode_lang(self.lt_key, self.lang).label)
                                .width(95.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, l) in BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65) {
                                        ui.selectable_value(&mut self.lt_key, c, l);
                                    }
                                });
                            if primary_button(ui, s.apply, 72.0) {
                                self.apply_keycode(0x4000 | (((self.lt_layer as u16) & 0x0F) << 8) | (self.lt_key & 0xFF));
                            }
                        });

                        ui.add_space(10.0);
                        ui.label(RichText::new(s.mods_title).size(12.0).color(MINT).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if chip_button(ui, "Ctrl", self.mod_c) { self.mod_c = !self.mod_c; }
                            if chip_button(ui, "Shift", self.mod_s) { self.mod_s = !self.mod_s; }
                            if chip_button(ui, "Alt", self.mod_a) { self.mod_a = !self.mod_a; }
                            if chip_button(ui, "Win", self.mod_g) { self.mod_g = !self.mod_g; }
                            egui::ComboBox::from_id_salt("mk")
                                .selected_text(decode_keycode_lang(self.mod_key, self.lang).label)
                                .width(72.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, l) in BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65) {
                                        ui.selectable_value(&mut self.mod_key, c, l);
                                    }
                                });
                            if primary_button(ui, s.apply, 58.0) {
                                let mut m = 0u16;
                                if self.mod_c { m |= 1; }
                                if self.mod_s { m |= 2; }
                                if self.mod_a { m |= 4; }
                                if self.mod_g { m |= 8; }
                                if m > 0 {
                                    self.apply_keycode((m << 8) | (self.mod_key & 0xFF));
                                }
                            }
                        });
                    }
                    PickerTab::Basic => {
                        ui.add(egui::TextEdit::singleline(&mut self.kc_search).hint_text(s.search_hint).desired_width(ui.available_width()));
                        ui.add_space(8.0);
                        let q = self.kc_search.to_lowercase();
                        egui::ScrollArea::vertical().max_height(185.0).show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for &(code, name, label) in BASIC_KEYCODES {
                                    if !q.is_empty() && !name.to_lowercase().contains(&q) && !label.to_lowercase().contains(&q) {
                                        continue;
                                    }
                                    if chip_button(ui, label, false) {
                                        self.apply_keycode(code);
                                    }
                                }
                            });
                        });
                    }
                    PickerTab::Layer => {
                        let ops = [
                            (0x0001, "▽ TRNS"), (0x0000, "NO"),
                            (0x5221, "MO(1)"), (0x5222, "MO(2)"), (0x5223, "MO(3)"),
                            (0x5261, "TG(1)"), (0x5262, "TG(2)"), (0x5263, "TG(3)"),
                            (0x5200, "TO(0)"), (0x5201, "TO(1)"), (0x5202, "TO(2)"),
                            (0x5240, "DF(0)"), (0x5242, "DF(2)"),
                            (0x5281, "OSL(1)"), (0x52C1, "TT(1)"),
                        ];
                        ui.horizontal_wrapped(|ui| {
                            for (c, l) in ops {
                                if chip_button(ui, l, false) {
                                    self.apply_keycode(c);
                                }
                            }
                        });
                    }
                    PickerTab::Wob => {
                        ui.horizontal_wrapped(|ui| {
                            for &(code, _, _) in SPECIAL_KEYCODES {
                                let lbl = decode_keycode_lang(code, self.lang).label;
                                if chip_button(ui, &lbl, false) {
                                    self.apply_keycode(code);
                                }
                            }
                        });
                    }
                    PickerTab::Macro => {
                        ui.horizontal_wrapped(|ui| {
                            for i in 0..16u16 {
                                if chip_button(ui, &format!("M{}", i), false) {
                                    self.apply_keycode(0x7700 + i);
                                }
                            }
                        });
                    }
                }
            });
        });
    }

    fn draw_keyboard_twin(&mut self, ui: &mut egui::Ui, interactive: bool) {
        let avail_w = ui.available_width().max(640.0);
        let pad_x = 18.0;
        let pad_y = 14.0;
        let unit = ((avail_w - pad_x * 2.0) / 18.25).clamp(32.0, 47.0);
        let board_w = unit * 18.25 + pad_x * 2.0;
        let board_h = unit * 6.25 + pad_y * 2.0;

        let (chassis, _) = ui.allocate_exact_size(Vec2::new(board_w, board_h), Sense::hover());
        let p = ui.painter();

        p.rect_filled(chassis, Rounding::same(16.0), Color32::from_rgb(20, 24, 33));
        p.rect_stroke(chassis, Rounding::same(16.0), Stroke::new(1.5_f32, Color32::from_rgb(50, 58, 76)));

        let plate = chassis.shrink2(Vec2::new(pad_x - 8.0, pad_y - 8.0));
        p.rect_filled(plate, Rounding::same(10.0), Color32::from_rgb(10, 12, 17));

        let origin = chassis.min + Vec2::new(pad_x, pad_y);
        let t = self.start_time.elapsed().as_secs_f32();
        let mut clicked = None;

        for k in CRUSH80_LAYOUT {
            let rect = Rect::from_min_size(
                Pos2::new(origin.x + k.x * unit + 2.5, origin.y + k.y * unit + 2.5),
                Vec2::new(k.w * unit - 5.0, unit - 5.0),
            );
            let resp = if interactive {
                Some(ui.interact(rect, ui.id().with(("kcap", k.r, k.c)), Sense::click()))
            } else {
                None
            };
            if let Some(ref r) = resp {
                if r.clicked() {
                    clicked = Some((k.r, k.c));
                }
            }

            let selected = interactive && self.selected_key == Some((k.r, k.c));
            let hovered = resp.as_ref().map(|r| r.hovered()).unwrap_or(false);
            let code = self.layers[self.current_layer][(k.r as usize) * 16 + (k.c as usize)];
            let dec = decode_keycode_lang(code, self.lang);

            let glow_col = if !self.hw.sys_rgb_on || self.rgb.effect == 0 || self.rgb.brightness == 0 {
                None
            } else if self.rgb.rainbow {
                let hue = (((k.x / 18.25) * 255.0 + t * (24.0 + self.rgb.speed as f32 * 14.0)) as u32 % 256) as u8;
                Some(hsv_to_color32(hue, 230))
            } else {
                Some(hsv_to_color32(self.rgb.hue, self.rgb.saturation))
            };

            if let Some(gc) = glow_col {
                let alpha = (18 + self.rgb.brightness * 6).min(75);
                let bleed = Color32::from_rgba_unmultiplied(gc.r(), gc.g(), gc.b(), alpha);
                p.rect_filled(rect.expand(1.5), Rounding::same(7.5), bleed);
            }

            let skirt_col = match k.style {
                2 => Color32::from_rgb(38, 28, 16),
                1 => Color32::from_rgb(20, 24, 33),
                _ => Color32::from_rgb(26, 31, 42),
            };
            p.rect_filled(rect, Rounding::same(6.5), skirt_col);

            let dish = Rect::from_min_max(rect.min + Vec2::new(2.0, 1.5), rect.max - Vec2::new(2.0, 4.5));
            let dish_col = if selected {
                Color32::from_rgb(48, 36, 18)
            } else if hovered {
                Color32::from_rgb(40, 48, 64)
            } else {
                match k.style {
                    2 => Color32::from_rgb(56, 40, 20),
                    1 => Color32::from_rgb(27, 32, 44),
                    _ => Color32::from_rgb(35, 41, 55),
                }
            };
            p.rect_filled(dish, Rounding::same(5.0), dish_col);

            let cat_col = match dec.category {
                KeyCategory::TapHold => Some(VIOLET),
                KeyCategory::Layer => Some(ACCENT),
                KeyCategory::Mods => Some(MINT),
                KeyCategory::Macro => Some(ROSE),
                _ => None,
            };
            if let Some(cc) = cat_col {
                p.circle_filled(Pos2::new(dish.max.x - 5.0, dish.min.y + 5.0), 2.3, cc);
            }

            if selected {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(2.0_f32, ACCENT));
            } else if hovered {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(1.2_f32, TEXT_SEC));
            }

            let legend_col = if dec.code == 0 {
                TEXT_DIM
            } else if selected {
                ACCENT
            } else if let Some(gc) = glow_col {
                blend_color(TEXT_PRI, gc, 0.25)
            } else {
                TEXT_PRI
            };
            if k.r == 4 && k.c == 12 {
                p.text(Pos2::new(dish.center().x, dish.center().y - 5.0), egui::Align2::CENTER_CENTER, "F13", egui::FontId::proportional(10.5), legend_col);
                p.text(Pos2::new(dish.center().x, dish.center().y + 7.5), egui::Align2::CENTER_CENTER, &dec.label, egui::FontId::proportional(8.0), TEXT_SEC);
            } else {
                let fs = if dec.label.chars().count() > 5 { 9.0 } else { 10.8 };
                p.text(dish.center(), egui::Align2::CENTER_CENTER, &dec.label, egui::FontId::proportional(fs), legend_col);
            }
        }

        if let Some(sel) = clicked {
            self.selected_key = Some(sel);
            self.sync_selected_hex();
        }
    }

    fn ui_rgb(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.rgb_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.rgb_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, s.save_flash, 118.0) {
                    self.mark_ui_edit();
                    match self.hid.set_rgb_partial(None, None, None, None, None, true) {
                        Ok(()) => self.set_toast(s.saved_flash, false),
                        Err(e) => self.set_toast(e, true),
                    }
                }
            });
        });

        ui.add_space(12.0);
        self.draw_keyboard_twin(ui, false);
        ui.add_space(12.0);

        ui.columns(2, |cols| {
            card_frame().show(&mut cols[0], |ui| {
                ui.label(RichText::new(s.color_mode).size(14.5).strong().color(TEXT_PRI));
                ui.add_space(10.0);

                egui::Frame::none()
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::same(4.0))
                    .show(ui, |ui| {
                        ui.columns(2, |scols| {
                            let rb = self.rgb.rainbow;
                            let b1 = egui::Button::new(
                                RichText::new(s.rainbow_mode).size(12.5).strong().color(if rb { BG_CANVAS } else { TEXT_SEC }),
                            )
                            .fill(if rb { ACCENT } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(scols[0].available_width(), 30.0));
                            if scols[0].add(b1).clicked() {
                                self.mark_ui_edit();
                                self.rgb.rainbow = true;
                                let _ = self.hid.set_rgb_partial(None, None, None, None, Some(true), false);
                            }

                            let b2 = egui::Button::new(
                                RichText::new(s.single_mode).size(12.5).strong().color(if !rb { BG_CANVAS } else { TEXT_SEC }),
                            )
                            .fill(if !rb { ACCENT } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(scols[1].available_width(), 30.0));
                            if scols[1].add(b2).clicked() {
                                self.mark_ui_edit();
                                self.rgb.rainbow = false;
                                let _ = self.hid.set_rgb_partial(None, None, None, Some((self.rgb.hue, self.rgb.saturation)), Some(false), false);
                            }
                        });
                    });

                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.effect).size(12.5).color(TEXT_SEC));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let cur = rgb_effect_label(self.lang, self.rgb.effect);
                        egui::ComboBox::from_id_salt("rgb_eff")
                            .selected_text(cur)
                            .width(210.0)
                            .show_ui(ui, |ui| {
                                for id in 0..=18u8 {
                                    if ui.selectable_value(&mut self.rgb.effect, id, rgb_effect_label(self.lang, id)).changed() {
                                        self.mark_ui_edit();
                                        let _ = self.hid.set_rgb_partial(None, Some(id), None, None, None, false);
                                    }
                                }
                            });
                    });
                });

                ui.add_space(10.0);
                ui.label(RichText::new(format!("{}  {} / 9", s.brightness, self.rgb.brightness)).size(12.0).color(TEXT_SEC));
                if ui.add(egui::Slider::new(&mut self.rgb.brightness, 0..=9).show_value(false)).changed() {
                    self.mark_ui_edit();
                    let _ = self.hid.set_rgb_partial(Some(self.rgb.brightness), None, None, None, None, false);
                }

                ui.add_space(8.0);
                ui.label(RichText::new(format!("{}  {} / 4", s.speed, self.rgb.speed)).size(12.0).color(TEXT_SEC));
                if ui.add(egui::Slider::new(&mut self.rgb.speed, 0..=4).show_value(false)).changed() {
                    self.mark_ui_edit();
                    let _ = self.hid.set_rgb_partial(None, None, Some(self.rgb.speed), None, None, false);
                }
            });

            card_frame().show(&mut cols[1], |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.color_pad).size(14.5).strong().color(TEXT_PRI));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        pill_badge(ui, &format!("H:{}  S:{}", self.rgb.hue, self.rgb.saturation), ACCENT);
                    });
                });
                ui.add_space(10.0);

                let pad_w = ui.available_width();
                let pad_h = 112.0;
                let (pad_rect, resp) = ui.allocate_exact_size(Vec2::new(pad_w, pad_h), Sense::click_and_drag());
                let p = ui.painter();
                let cols_n = 36;
                let rows_n = 8;
                let cw = pad_rect.width() / cols_n as f32;
                let ch = pad_rect.height() / rows_n as f32;
                for ix in 0..cols_n {
                    let h = ((ix as f32 / (cols_n - 1) as f32) * 255.0) as u8;
                    for iy in 0..rows_n {
                        let sat = ((1.0 - iy as f32 / (rows_n - 1) as f32) * 255.0) as u8;
                        let r = Rect::from_min_size(
                            Pos2::new(pad_rect.min.x + ix as f32 * cw, pad_rect.min.y + iy as f32 * ch),
                            Vec2::new(cw + 0.8, ch + 0.8),
                        );
                        p.rect_filled(r, Rounding::ZERO, hsv_to_color32(h, sat));
                    }
                }
                p.rect_stroke(pad_rect, Rounding::same(8.0), Stroke::new(1.5_f32, HAIRLINE));

                let rx = pad_rect.min.x + (self.rgb.hue as f32 / 255.0) * pad_rect.width();
                let ry = pad_rect.min.y + (1.0 - self.rgb.saturation as f32 / 255.0) * pad_rect.height();
                p.circle_stroke(Pos2::new(rx, ry), 7.0, Stroke::new(2.0_f32, Color32::WHITE));
                p.circle_stroke(Pos2::new(rx, ry), 8.2, Stroke::new(1.0_f32, Color32::BLACK));

                if (resp.clicked() || resp.dragged()) && resp.interact_pointer_pos().is_some() {
                    let pos = resp.interact_pointer_pos().unwrap();
                    let nx = ((pos.x - pad_rect.min.x) / pad_rect.width()).clamp(0.0, 1.0);
                    let ny = ((pos.y - pad_rect.min.y) / pad_rect.height()).clamp(0.0, 1.0);
                    let nh = (nx * 255.0) as u8;
                    let ns = ((1.0 - ny) * 255.0) as u8;
                    if nh != self.rgb.hue || ns != self.rgb.saturation || self.rgb.rainbow {
                        self.mark_ui_edit();
                        self.rgb.hue = nh;
                        self.rgb.saturation = ns;
                        self.rgb.rainbow = false;
                        let _ = self.hid.set_rgb_partial(None, None, None, Some((nh, ns)), None, false);
                    }
                }

                ui.add_space(12.0);
                ui.label(RichText::new(s.quick_colors).size(11.5).color(TEXT_SEC));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let swatches = [
                        (140u8, 18u8, Color32::from_rgb(241, 245, 249)),
                        (135, 255, CYAN),
                        (26, 255, ACCENT),
                        (195, 255, VIOLET),
                        (0, 255, ROSE),
                        (96, 255, MINT),
                    ];
                    for (h, sat, col) in swatches {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 11.0, col);
                        if resp.hovered() {
                            ui.painter().circle_stroke(r.center(), 12.5, Stroke::new(1.5_f32, TEXT_PRI));
                        }
                        if resp.clicked() {
                            self.mark_ui_edit();
                            self.rgb.hue = h;
                            self.rgb.saturation = sat;
                            self.rgb.rainbow = false;
                            let _ = self.hid.set_rgb_partial(None, None, None, Some((h, sat)), None, false);
                        }
                        ui.add_space(4.0);
                    }
                });
            });
        });
    }

    fn ui_hardware(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.hw_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.hw_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if primary_button(ui, s.save_flash, 118.0) {
                    self.mark_ui_edit();
                    match self.hid.set_rgb_partial(None, None, None, None, None, true) {
                        Ok(()) => self.set_toast(s.saved_flash, false),
                        Err(e) => self.set_toast(e, true),
                    }
                }
            });
        });

        ui.add_space(14.0);

        ui.columns(2, |cols| {
            card_frame().show(&mut cols[0], |ui| {
                ui.label(RichText::new(s.perf_card).size(14.5).strong().color(TEXT_PRI));
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(s.low_latency).size(13.0).strong().color(TEXT_PRI));
                        ui.label(RichText::new(s.low_latency_sub).size(11.0).color(TEXT_SEC));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if toggle_switch(ui, &mut self.hw.low_latency) {
                            self.mark_ui_edit();
                            let _ = self.hid.set_config_byte(29, if self.hw.low_latency { 0xFF } else { 0x00 }, false);
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(s.win_lock).size(13.0).strong().color(TEXT_PRI));
                        ui.label(RichText::new(s.win_lock_sub).size(11.0).color(TEXT_SEC));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if toggle_switch(ui, &mut self.hw.win_lock) {
                            self.mark_ui_edit();
                            let _ = self.hid.set_config_byte(21, if self.hw.win_lock { 0xFF } else { 0x00 }, false);
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(s.os_mode).size(13.0).strong().color(TEXT_PRI));
                        ui.label(RichText::new(s.os_mode_sub).size(11.0).color(TEXT_SEC));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if chip_button(ui, "macOS", self.hw.mac_mode) {
                            self.mark_ui_edit();
                            self.hw.mac_mode = true;
                            let _ = self.hid.set_config_byte(26, 1, false);
                        }
                        if chip_button(ui, "Windows", !self.hw.mac_mode) {
                            self.mark_ui_edit();
                            self.hw.mac_mode = false;
                            let _ = self.hid.set_config_byte(26, 0, false);
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.tap_term).size(13.0).strong().color(TEXT_PRI));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        pill_badge(ui, &format!("{} ms", self.hw.tapping_term_ms), ACCENT);
                    });
                });
                ui.label(RichText::new(s.tap_term_sub).size(11.0).color(TEXT_SEC));
                ui.add_space(6.0);
                let mut units = (self.hw.tapping_term_ms / 10).clamp(10, 30) as u8;
                if ui.add(egui::Slider::new(&mut units, 10..=30).show_value(false)).changed() {
                    self.mark_ui_edit();
                    self.hw.tapping_term_ms = (units as u16) * 10;
                    let _ = self.hid.set_config_byte(60, units, false);
                }
            });

            card_frame().show(&mut cols[1], |ui| {
                ui.label(RichText::new(s.ambient_card).size(14.5).strong().color(TEXT_PRI));
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(s.master_rgb).size(13.0).strong().color(TEXT_PRI));
                        ui.label(RichText::new(s.master_rgb_sub).size(11.0).color(TEXT_SEC));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if toggle_switch(ui, &mut self.hw.sys_rgb_on) {
                            self.mark_ui_edit();
                            let _ = self.hid.set_config_byte(40, if self.hw.sys_rgb_on { 0 } else { 1 }, false);
                        }
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                // Logo Light
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.logo_light).size(13.5).strong().color(ACCENT));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if toggle_switch(ui, &mut self.hw.logo_on) {
                            self.mark_ui_edit();
                            let _ = self.hid.set_config_byte(35, if self.hw.logo_on { 0 } else { 1 }, false);
                        }
                        ui.label(RichText::new(if self.hw.logo_on { s.on } else { s.off }).size(11.5).color(TEXT_SEC));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.mode_lbl).size(12.0).color(TEXT_SEC));
                    for (m, lbl) in [(0u8, "01"), (1, "02"), (2, "03"), (3, "04")] {
                        if chip_button(ui, lbl, self.hw.logo_mode == m) {
                            self.mark_ui_edit();
                            self.hw.logo_mode = m;
                            let _ = self.hid.set_config_byte(32, m, false);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.color_lbl).size(12.0).color(TEXT_SEC));
                    if chip_button(ui, s.rainbow, self.hw.logo_color == 0xFF) {
                        self.mark_ui_edit();
                        self.hw.logo_color = 0xFF;
                        let _ = self.hid.set_config_byte(33, 0xFF, false);
                    }
                    let dots = [
                        (0u8, Color32::from_rgb(244, 63, 94)),
                        (1, Color32::from_rgb(251, 146, 60)),
                        (2, Color32::from_rgb(250, 204, 21)),
                        (3, Color32::from_rgb(52, 211, 153)),
                        (4, CYAN),
                        (5, Color32::from_rgb(96, 165, 250)),
                        (6, VIOLET),
                        (7, TEXT_PRI),
                    ];
                    for (c, col) in dots {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.5, col);
                        if self.hw.logo_color == c {
                            ui.painter().circle_stroke(r.center(), 9.2, Stroke::new(1.6_f32, ACCENT));
                        }
                        if resp.clicked() {
                            self.mark_ui_edit();
                            self.hw.logo_color = c;
                            let _ = self.hid.set_config_byte(33, c, false);
                        }
                    }
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);

                // Side Light
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.side_light).size(13.5).strong().color(CYAN));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if toggle_switch(ui, &mut self.hw.side_on) {
                            self.mark_ui_edit();
                            let _ = self.hid.set_config_byte(39, if self.hw.side_on { 0 } else { 1 }, false);
                        }
                        ui.label(RichText::new(if self.hw.side_on { s.on } else { s.off }).size(11.5).color(TEXT_SEC));
                    });
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.mode_lbl).size(12.0).color(TEXT_SEC));
                    for (m, lbl) in [(0u8, "01"), (1, "02"), (2, "03"), (3, "04")] {
                        if chip_button(ui, lbl, self.hw.side_mode == m) {
                            self.mark_ui_edit();
                            self.hw.side_mode = m;
                            let _ = self.hid.set_config_byte(36, m, false);
                        }
                    }
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.color_lbl).size(12.0).color(TEXT_SEC));
                    if chip_button(ui, s.rainbow, self.hw.side_color == 0xFF) {
                        self.mark_ui_edit();
                        self.hw.side_color = 0xFF;
                        let _ = self.hid.set_config_byte(37, 0xFF, false);
                    }
                    let dots = [
                        (0u8, Color32::from_rgb(244, 63, 94)),
                        (1, Color32::from_rgb(251, 146, 60)),
                        (2, Color32::from_rgb(250, 204, 21)),
                        (3, Color32::from_rgb(52, 211, 153)),
                        (4, CYAN),
                        (5, Color32::from_rgb(96, 165, 250)),
                        (6, VIOLET),
                        (7, TEXT_PRI),
                    ];
                    for (c, col) in dots {
                        let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                        ui.painter().circle_filled(r.center(), 7.5, col);
                        if self.hw.side_color == c {
                            ui.painter().circle_stroke(r.center(), 9.2, Stroke::new(1.6_f32, ACCENT));
                        }
                        if resp.clicked() {
                            self.mark_ui_edit();
                            self.hw.side_color = c;
                            let _ = self.hid.set_config_byte(37, c, false);
                        }
                    }
                });
            });
        });

        ui.add_space(12.0);
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.autostart).size(13.5).strong().color(TEXT_PRI));
                        pill_badge(ui, s.start_minimized, CYAN);
                    });
                    ui.label(RichText::new(s.autostart_sub).size(11.5).color(TEXT_SEC));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle_switch(ui, &mut self.autostart_enabled) {
                        match win_sys::set_autostart_enabled(self.autostart_enabled) {
                            Ok(()) => {
                                self.set_toast(
                                    format!(
                                        "{} · {}",
                                        s.autostart,
                                        if self.autostart_enabled { s.on } else { s.off }
                                    ),
                                    false,
                                );
                            }
                            Err(e) => {
                                self.autostart_enabled = win_sys::is_autostart_enabled();
                                self.set_toast(e, true);
                            }
                        }
                    }
                    ui.label(
                        RichText::new(if self.autostart_enabled { s.on } else { s.off })
                            .size(11.5)
                            .color(if self.autostart_enabled { MINT } else { TEXT_SEC }),
                    );
                });
            });
        });
    }

    fn ui_macros(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);

        // Live keyboard recording capture when active
        if self.macro_recording {
            let now = Instant::now();
            let (events, cur_mods) = ui.ctx().input(|i| {
                let evs: Vec<(egui::Key, bool)> = i
                    .events
                    .iter()
                    .filter_map(|ev| match ev {
                        egui::Event::Key {
                            key,
                            pressed,
                            repeat: false,
                            ..
                        } => Some((*key, *pressed)),
                        _ => None,
                    })
                    .collect();
                (evs, i.modifiers)
            });

            let prev = self.macro_prev_mods;
            let mod_pairs = [
                (prev.ctrl, cur_mods.ctrl, 0xE0u8),
                (prev.shift, cur_mods.shift, 0xE1u8),
                (prev.alt, cur_mods.alt, 0xE2u8),
                (prev.command, cur_mods.command, 0xE3u8),
            ];
            for (was, is, hid) in mod_pairs {
                if !was && is {
                    self.push_recorded_macro_key(hid, true, now);
                }
            }
            for (key, pressed) in events {
                if let Some(hid) = egui_key_to_hid(key) {
                    self.push_recorded_macro_key(hid, pressed, now);
                }
            }
            for (was, is, hid) in mod_pairs {
                if was && !is {
                    self.push_recorded_macro_key(hid, false, now);
                }
            }
            self.macro_prev_mods = cur_mods;
        }

        let used = encoded_byte_len(&self.macro_slots);
        let over_limit = used > MACRO_BUFFER_SIZE;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.macro_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.macro_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let write_btn = egui::Button::new(
                    RichText::new(s.write_macros).size(12.0).strong().color(BG_CANVAS),
                )
                .fill(if over_limit { TEXT_DIM } else { ACCENT })
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(7.0))
                .min_size(Vec2::new(118.0, 28.0));
                if ui.add_enabled(!over_limit, write_btn).clicked() {
                    self.macro_recording = false;
                    self.mark_ui_edit();
                    match encode_macro_buffer(&self.macro_slots) {
                        Ok(raw) => match self.hid.write_macro_buffer(&raw) {
                            Ok(()) => self.set_toast(s.saved_flash, false),
                            Err(e) => self.set_toast(e, true),
                        },
                        Err(e) => self.set_toast(e, true),
                    }
                }
                pill_badge(
                    ui,
                    &format!("{} {} / {} B", s.macro_bytes_used, used, MACRO_BUFFER_SIZE),
                    if over_limit { ROSE } else { CYAN },
                );
            });
        });

        ui.add_space(12.0);

        // 16-slot selector bar (M0..M15)
        card_frame().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for i in 0..MACRO_SLOT_COUNT {
                    let count = self.macro_slots[i].len();
                    let label = if count > 0 {
                        format!("M{} ({})", i, count)
                    } else {
                        format!("M{}", i)
                    };
                    if chip_button(ui, &label, self.active_macro_slot == i) {
                        self.macro_recording = false;
                        self.active_macro_slot = i;
                    }
                }
            });
        });

        ui.add_space(12.0);

        // Active slot visual step editor + recorder
        card_frame().show(ui, |ui| {
            let slot_idx = self.active_macro_slot;
            ui.horizontal(|ui| {
                pill_badge(ui, &format!("M{:02}", slot_idx), ACCENT);
                ui.add_space(4.0);

                if self.macro_recording {
                    let stop_btn = egui::Button::new(
                        RichText::new(s.macro_stop).size(12.0).strong().color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(220, 38, 38))
                    .stroke(Stroke::NONE)
                    .rounding(Rounding::same(7.0))
                    .min_size(Vec2::new(96.0, 28.0));
                    if ui.add(stop_btn).clicked() {
                        self.macro_recording = false;
                    }
                } else if primary_button(ui, s.macro_record, 96.0) {
                    self.macro_recording = true;
                    self.macro_last_event_time = None;
                    self.macro_prev_mods = ui.ctx().input(|i| i.modifiers);
                }

                if chip_button(ui, "⏱ Delay", self.macro_record_delays) {
                    self.macro_record_delays = !self.macro_record_delays;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if danger_button(ui, s.macro_clear) {
                        self.macro_slots[slot_idx].clear();
                    }
                });
            });

            if self.macro_recording {
                ui.add_space(8.0);
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 22))
                    .stroke(Stroke::new(1.0_f32, ROSE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(s.macro_recording_hint).size(12.0).strong().color(ROSE));
                    });
            }

            ui.add_space(10.0);

            // Step chips
            egui::Frame::none()
                .fill(BG_CANVAS)
                .stroke(Stroke::new(1.0_f32, HAIRLINE))
                .rounding(Rounding::same(9.0))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.set_min_height(96.0);
                    ui.set_min_width(ui.available_width());
                    if self.macro_slots[slot_idx].is_empty() {
                        ui.label(RichText::new(s.macro_hint).size(12.0).color(TEXT_DIM));
                    } else {
                        let mut remove_idx: Option<usize> = None;
                        ui.horizontal_wrapped(|ui| {
                            for (idx, step) in self.macro_slots[slot_idx].iter().enumerate() {
                                let col = match step {
                                    MacroStep::Text(_) => TEXT_PRI,
                                    MacroStep::Tap(_) => CYAN,
                                    MacroStep::Down(_) => MINT,
                                    MacroStep::Up(_) => VIOLET,
                                    MacroStep::Delay(_) => ACCENT,
                                };
                                let label = format!("{}  ×", format_step(step, self.lang));
                                let b = egui::Button::new(RichText::new(label).size(11.5).color(col))
                                    .fill(BG_SURFACE)
                                    .stroke(Stroke::new(
                                        1.0_f32,
                                        Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 95),
                                    ))
                                    .rounding(Rounding::same(6.5))
                                    .min_size(Vec2::new(0.0, 26.0));
                                if ui.add(b).clicked() {
                                    remove_idx = Some(idx);
                                }
                            }
                        });
                        if let Some(i) = remove_idx {
                            self.macro_slots[slot_idx].remove(i);
                        }
                    }
                });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            // Manual Add Text & Add Delay controls
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.macro_text_input)
                        .hint_text(s.macro_hint)
                        .desired_width(240.0),
                );
                if chip_button(ui, s.macro_add_text, false) {
                    if !self.macro_text_input.is_empty() {
                        let txt = std::mem::take(&mut self.macro_text_input);
                        self.macro_slots[slot_idx].push(MacroStep::Text(txt));
                    }
                }

                ui.add_space(14.0);
                ui.add(
                    egui::DragValue::new(&mut self.macro_delay_input)
                        .range(1..=65535)
                        .suffix(" ms"),
                );
                if chip_button(ui, s.macro_add_delay, false) {
                    self.macro_slots[slot_idx].push(MacroStep::Delay(self.macro_delay_input));
                }
            });
        });
    }

    fn ui_profiles(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.prof_title).size(19.0).strong().color(TEXT_PRI));
                    pill_badge(ui, s.prof_includes_hw, MINT);
                });
                ui.label(RichText::new(s.prof_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chip_button(ui, s.prof_export, false) {
                    if let Some(path) = win_sys::save_file_dialog(
                        s.prof_export,
                        "crush80-profile.json",
                        "JSON Profile (*.json)",
                        "*.json",
                    ) {
                        let ts = format!(
                            "{}",
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                        );
                        match self.hid.export_snapshot(ts) {
                            Ok(snap) => match serde_json::to_string_pretty(&snap) {
                                Ok(txt) => match std::fs::write(&path, txt) {
                                    Ok(()) => self.set_toast(
                                        path.file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("Exported")
                                            .to_string(),
                                        false,
                                    ),
                                    Err(e) => self.set_toast(e.to_string(), true),
                                },
                                Err(e) => self.set_toast(e.to_string(), true),
                            },
                            Err(e) => self.set_toast(e, true),
                        }
                    }
                }
                if chip_button(ui, s.prof_import, false) {
                    if let Some(path) =
                        win_sys::open_file_dialog(s.prof_import, "JSON Profile (*.json)", "*.json")
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(txt) => match serde_json::from_str::<FullSnapshot>(&txt) {
                                Ok(snap) => {
                                    let stem = path
                                        .file_stem()
                                        .and_then(|n| n.to_str())
                                        .unwrap_or("imported")
                                        .to_string();
                                    if let Ok(pretty) = serde_json::to_string_pretty(&snap) {
                                        let dest = self.profiles_dir.join(format!("{}.json", stem));
                                        let _ = std::fs::write(dest, pretty);
                                        self.refresh_profiles();
                                        self.set_toast(format!("{} · {}", s.prof_import, stem), false);
                                    }
                                }
                                Err(e) => self.set_toast(format!("Invalid JSON: {}", e), true),
                            },
                            Err(e) => self.set_toast(e.to_string(), true),
                        }
                    }
                }
            });
        });
        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_profile_name).hint_text(s.prof_hint).desired_width(340.0));
                if primary_button(ui, s.save_prof, 130.0) {
                    let name = self.new_profile_name.trim().to_string();
                    if !name.is_empty() {
                        let ts = format!("{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs());
                        match self.hid.export_snapshot(ts) {
                            Ok(snap) => {
                                if let Ok(txt) = serde_json::to_string_pretty(&snap) {
                                    let _ = std::fs::write(self.profiles_dir.join(format!("{}.json", name)), txt);
                                    self.new_profile_name.clear();
                                    self.refresh_profiles();
                                    self.set_toast(name, false);
                                }
                            }
                            Err(e) => self.set_toast(e, true),
                        }
                    }
                }
            });

            ui.add_space(14.0);
            let names = self.profile_names.clone();
            for name in names {
                egui::Frame::none()
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&name).size(13.5).strong().color(TEXT_PRI));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if danger_button(ui, s.delete) {
                                    let _ = std::fs::remove_file(self.profiles_dir.join(format!("{}.json", name)));
                                    self.refresh_profiles();
                                }
                                if chip_button(ui, s.prof_export, false) {
                                    let default_name = format!("{}.json", name);
                                    if let Some(dest) = win_sys::save_file_dialog(
                                        s.prof_export,
                                        &default_name,
                                        "JSON Profile (*.json)",
                                        "*.json",
                                    ) {
                                        let src = self.profiles_dir.join(&default_name);
                                        match std::fs::copy(&src, &dest) {
                                            Ok(_) => self.set_toast(default_name, false),
                                            Err(e) => self.set_toast(e.to_string(), true),
                                        }
                                    }
                                }
                                if primary_button(ui, s.apply_prof, 76.0) {
                                    self.mark_ui_edit();
                                    let p = self.profiles_dir.join(format!("{}.json", name));
                                    if let Ok(t) = std::fs::read_to_string(p) {
                                        if let Ok(snap) = serde_json::from_str::<FullSnapshot>(&t) {
                                            match self.hid.apply_snapshot(&snap) {
                                                Ok(()) => self.refresh_all(),
                                                Err(e) => self.set_toast(e, true),
                                            }
                                        }
                                    }
                                }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
        });
    }

    fn ui_autoswitch(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let mut save_needed = false;
        let (last_exe, active_name, mut enabled, mut rules) = {
            let st = self.autoswitch.lock().unwrap();
            let active = st.active_rule_id.as_ref().and_then(|id| {
                st.cfg.rules.iter().find(|r| &r.id == id).map(|r| r.name.clone())
            });
            (st.last_exe.clone(), active, st.cfg.enabled, st.cfg.rules.clone())
        };

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.auto_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.auto_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if toggle_switch(ui, &mut enabled) {
                    save_needed = true;
                }
                ui.label(RichText::new(if enabled { s.running } else { s.stopped }).size(12.5).color(if enabled { MINT } else { TEXT_SEC }));
            });
        });

        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(s.fg_app).size(12.0).color(TEXT_DIM));
                pill_badge(ui, if last_exe.is_empty() { "-" } else { &last_exe }, CYAN);
                ui.add_space(16.0);
                ui.label(RichText::new(s.matched_rule).size(12.0).color(TEXT_DIM));
                pill_badge(ui, active_name.as_deref().unwrap_or(s.default_state), MINT);
            });

            ui.add_space(12.0);
            let mut del_idx = None;
            for (idx, r) in rules.iter_mut().enumerate() {
                egui::Frame::none()
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if toggle_switch(ui, &mut r.enabled) { save_needed = true; }
                            if ui.add(egui::TextEdit::singleline(&mut r.name).desired_width(140.0)).changed() { save_needed = true; }
                            if ui.add(egui::TextEdit::singleline(&mut r.exe).desired_width(190.0)).changed() { save_needed = true; }
                            if chip_button(ui, if r.rgb.rainbow { s.rainbow } else { s.single_mode }, r.rgb.rainbow) {
                                r.rgb.rainbow = !r.rgb.rainbow;
                                save_needed = true;
                            }
                            if ui.add(egui::DragValue::new(&mut r.rgb.effect).range(0..=18)).changed() { save_needed = true; }
                            if ui.add(egui::DragValue::new(&mut r.rgb.brightness).range(0..=9)).changed() { save_needed = true; }
                            if !r.rgb.rainbow {
                                if ui.add(egui::DragValue::new(&mut r.rgb.hue).range(0..=255)).changed() { save_needed = true; }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if danger_button(ui, s.delete) { del_idx = Some(idx); }
                            });
                        });
                    });
                ui.add_space(6.0);
            }
            if let Some(i) = del_idx {
                rules.remove(i);
                save_needed = true;
            }

            ui.add_space(8.0);
            if chip_button(ui, s.add_rule, false) {
                rules.push(SwitchRule {
                    id: format!("rule-{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()),
                    name: "Rule".into(),
                    exe: "notepad.exe".into(),
                    action: "rgb".into(),
                    rgb: RgbState { brightness: 6, effect: 6, speed: 2, hue: 135, saturation: 255, rainbow: false },
                    profile: String::new(),
                    enabled: true,
                });
                save_needed = true;
            }
        });

        if save_needed {
            let mut st = self.autoswitch.lock().unwrap();
            if enabled && !st.cfg.enabled {
                st.cfg.default_rgb = Some(self.rgb.clone());
            }
            st.cfg.enabled = enabled;
            st.cfg.rules = rules;
            st.active_rule_id = None;
            save_config(&self.autoswitch_path, &st.cfg);
        }
    }

    fn ui_tester(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);

        // Capture live keyboard events + modifier transitions
        let now = Instant::now();
        let (events, cur_mods) = ui.ctx().input(|i| {
            let evs: Vec<(egui::Key, bool)> = i
                .events
                .iter()
                .filter_map(|ev| match ev {
                    egui::Event::Key {
                        key,
                        pressed,
                        repeat: false,
                        ..
                    } => Some((*key, *pressed)),
                    _ => None,
                })
                .collect();
            (evs, i.modifiers)
        });

        let prev = self.tester_prev_mods;
        for (was, is, hid) in [
            (prev.ctrl, cur_mods.ctrl, 0xE0u8),
            (prev.shift, cur_mods.shift, 0xE1u8),
            (prev.alt, cur_mods.alt, 0xE2u8),
            (prev.command, cur_mods.command, 0xE3u8),
        ] {
            if !was && is {
                self.record_tester_press(hid, now);
            } else if was && !is {
                self.tester_pressed.remove(&hid);
            }
        }
        self.tester_prev_mods = cur_mods;

        for (key, pressed) in events {
            if let Some(hid) = egui_key_to_hid(key) {
                if pressed {
                    self.record_tester_press(hid, now);
                } else {
                    self.tester_pressed.remove(&hid);
                }
            }
        }

        // Header
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.tester_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.tester_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chip_button(ui, s.tester_reset, false) {
                    self.tester_pressed.clear();
                    self.tester_tested.clear();
                    self.tester_chatter_keys.clear();
                    self.tester_last_press.clear();
                    self.tester_total = 0;
                    self.tester_chatter_count = 0;
                    self.tester_last_dt_ms = None;
                    self.tester_log.clear();
                }
            });
        });

        ui.add_space(10.0);
        self.draw_keyboard_twin_tester(ui);
        ui.add_space(12.0);

        // Stats bar + recent event log
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                pill_badge(
                    ui,
                    &format!("{}: {}", s.tester_total, self.tester_total),
                    CYAN,
                );
                let chatter_col = if self.tester_chatter_count > 0 { ROSE } else { MINT };
                let chatter_txt = if self.tester_chatter_count > 0 {
                    format!(
                        "{}: {} · {}",
                        s.tester_chatter, self.tester_chatter_count, s.tester_chatter_warn
                    )
                } else {
                    format!("{}: 0 · {}", s.tester_chatter, s.tester_clean)
                };
                pill_badge(ui, &chatter_txt, chatter_col);

                let dt_str = match self.tester_last_dt_ms {
                    Some(ms) => format!("{}: {} ms", s.tester_last_dt, ms),
                    None => format!("{}: -", s.tester_last_dt),
                };
                pill_badge(
                    ui,
                    &dt_str,
                    if self.tester_last_dt_ms.map(|d| d < 50).unwrap_or(false) {
                        ROSE
                    } else {
                        ACCENT
                    },
                );
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(8.0);

            egui::ScrollArea::vertical().max_height(155.0).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (name, dt_opt, is_chatter) in &self.tester_log {
                        let txt = match dt_opt {
                            Some(d) => format!("{} ({}ms)", name, d),
                            None => name.clone(),
                        };
                        pill_badge(ui, &txt, if *is_chatter { ROSE } else { TEXT_SEC });
                    }
                });
            });
        });
    }

    fn draw_keyboard_twin_tester(&self, ui: &mut egui::Ui) {
        let avail_w = ui.available_width().max(640.0);
        let pad_x = 18.0;
        let pad_y = 14.0;
        let unit = ((avail_w - pad_x * 2.0) / 18.25).clamp(32.0, 47.0);
        let board_w = unit * 18.25 + pad_x * 2.0;
        let board_h = unit * 6.25 + pad_y * 2.0;

        let (chassis, _) = ui.allocate_exact_size(Vec2::new(board_w, board_h), Sense::hover());
        let p = ui.painter();

        p.rect_filled(chassis, Rounding::same(16.0), Color32::from_rgb(20, 24, 33));
        p.rect_stroke(chassis, Rounding::same(16.0), Stroke::new(1.5_f32, Color32::from_rgb(50, 58, 76)));

        let plate = chassis.shrink2(Vec2::new(pad_x - 8.0, pad_y - 8.0));
        p.rect_filled(plate, Rounding::same(10.0), Color32::from_rgb(10, 12, 17));

        let origin = chassis.min + Vec2::new(pad_x, pad_y);

        for k in CRUSH80_LAYOUT {
            let rect = Rect::from_min_size(
                Pos2::new(origin.x + k.x * unit + 2.5, origin.y + k.y * unit + 2.5),
                Vec2::new(k.w * unit - 5.0, unit - 5.0),
            );
            let idx = (k.r as usize) * 16 + (k.c as usize);
            let factory_code = FACTORY_KEYMAP[0][idx];
            let hid = if factory_code <= 0xFF { factory_code as u8 } else { 0 };
            // Normalize R-modifiers to match egui's unified modifier state if pressed
            let norm_hid = match hid {
                0xE4 => 0xE0,
                0xE5 => 0xE1,
                0xE6 => 0xE2,
                0xE7 => 0xE3,
                other => other,
            };

            let is_pressed = hid != 0 && (self.tester_pressed.contains(&hid) || self.tester_pressed.contains(&norm_hid));
            let is_chatter = hid != 0 && (self.tester_chatter_keys.contains(&hid) || self.tester_chatter_keys.contains(&norm_hid));
            let is_tested = hid != 0 && (self.tester_tested.contains(&hid) || self.tester_tested.contains(&norm_hid));

            if is_pressed {
                let bleed = Color32::from_rgba_unmultiplied(CYAN.r(), CYAN.g(), CYAN.b(), 110);
                p.rect_filled(rect.expand(2.0), Rounding::same(8.0), bleed);
            }

            let skirt_col = match k.style {
                2 => Color32::from_rgb(38, 28, 16),
                1 => Color32::from_rgb(20, 24, 33),
                _ => Color32::from_rgb(26, 31, 42),
            };
            p.rect_filled(rect, Rounding::same(6.5), skirt_col);

            let dish = Rect::from_min_max(rect.min + Vec2::new(2.0, 1.5), rect.max - Vec2::new(2.0, 4.5));
            let dish_col = if is_pressed {
                Color32::from_rgb(16, 78, 96)
            } else if is_chatter {
                Color32::from_rgb(76, 22, 34)
            } else if is_tested {
                Color32::from_rgb(18, 52, 46)
            } else {
                match k.style {
                    2 => Color32::from_rgb(56, 40, 20),
                    1 => Color32::from_rgb(27, 32, 44),
                    _ => Color32::from_rgb(35, 41, 55),
                }
            };
            p.rect_filled(dish, Rounding::same(5.0), dish_col);

            if is_pressed {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(2.0_f32, CYAN));
            } else if is_chatter {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(1.8_f32, ROSE));
            } else if is_tested {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(1.2_f32, MINT));
            }

            let dec = decode_keycode_lang(self.layers[0][idx], self.lang);
            let legend_col = if is_pressed {
                Color32::WHITE
            } else if is_chatter {
                ROSE
            } else if is_tested {
                MINT
            } else {
                TEXT_PRI
            };
            let fs = if dec.label.chars().count() > 5 { 9.0 } else { 10.8 };
            p.text(dish.center(), egui::Align2::CENTER_CENTER, &dec.label, egui::FontId::proportional(fs), legend_col);
        }
    }

    fn ui_ota(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let prog = self.ota_progress.lock().unwrap().clone();
        let is_usb_wired = self.status.connected && self.status.pid == 0x5055;

        ui.label(RichText::new(s.ota_title).size(19.0).strong().color(TEXT_PRI));
        ui.label(RichText::new(s.ota_sub).size(12.0).color(TEXT_SEC));
        ui.add_space(12.0);

        // Current device & USB-only safety card
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                let conn_lbl = if !self.status.connected {
                    s.offline
                } else if is_usb_wired {
                    s.usb_wired
                } else {
                    s.wireless_24g
                };
                pill_badge(ui, conn_lbl, if is_usb_wired { MINT } else { ROSE });
                if self.status.connected {
                    pill_badge(ui, &self.status.firmware_name, ACCENT);
                    if let Some(crc) = self.status.firmware_crc {
                        pill_badge(ui, &format!("CRC 0x{:08X}", crc), CYAN);
                    }
                }
            });

            if !is_usb_wired {
                ui.add_space(10.0);
                egui::Frame::none()
                    .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 22))
                    .stroke(Stroke::new(1.0_f32, ROSE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(s.ota_usb_only).size(12.5).strong().color(ROSE));
                    });
            }

            ui.add_space(8.0);
            ui.label(RichText::new(s.ota_warning).size(11.5).color(ACCENT));
        });

        ui.add_space(12.0);

        // Firmware image selection & validation card
        card_frame().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if primary_button(ui, s.ota_select_bin, 130.0) {
                    if let Some(path) =
                        win_sys::open_file_dialog(s.ota_select_bin, "Firmware BIN (*.bin)", "*.bin")
                    {
                        self.load_ota_file(path);
                    }
                }

                if let Some(prosuite_path) = find_bundled_firmware(&[
                    "Crush80-RGB-0108-NativeHeld-v130-ProSuite.bin",
                    r"Crush80Driver\Crush80-RGB-0108-NativeHeld-v130-ProSuite.bin",
                ]) {
                    if chip_button(ui, s.ota_prosuite_btn, false) {
                        self.load_ota_file(prosuite_path);
                    }
                }

                if let Some(official_path) = find_bundled_firmware(&[
                    r"wobkey-crush80-reboot-pro-firmware\firmware\official\Crush80-RGB-20260618-official.bin",
                    r"firmware\official\Crush80-RGB-20260618-official.bin",
                ]) {
                    if chip_button(ui, s.ota_rollback_official, false) {
                        self.load_ota_file(official_path);
                    }
                }
            });

            if let Some(ref path) = self.ota_image_path {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(11.5)
                        .color(TEXT_SEC),
                );
            }

            if let Some(ref res) = self.ota_image_info {
                ui.add_space(10.0);
                match res {
                    Ok(info) => {
                        ui.horizontal_wrapped(|ui| {
                            pill_badge(ui, s.ota_valid, MINT);
                            pill_badge(ui, &info.name, ACCENT);
                            pill_badge(ui, &format!("CRC 0x{:08X}", info.ota_crc), CYAN);
                            pill_badge(ui, &format!("{} B", info.size), TEXT_SEC);
                        });
                    }
                    Err(err) => {
                        ui.horizontal_wrapped(|ui| {
                            pill_badge(ui, s.ota_invalid, ROSE);
                            ui.label(RichText::new(err).size(12.0).color(ROSE));
                        });
                    }
                }
            }
        });

        ui.add_space(12.0);

        // Flash trigger + live progress card
        card_frame().show(ui, |ui| {
            let valid_img = matches!(self.ota_image_info, Some(Ok(_))) && self.ota_image_bytes.is_some();
            let can_flash = is_usb_wired && valid_img && !prog.running;

            ui.horizontal(|ui| {
                let btn_lbl = if prog.running {
                    s.ota_flashing
                } else {
                    s.ota_flash_btn
                };
                let flash_btn = egui::Button::new(
                    RichText::new(btn_lbl)
                        .size(13.0)
                        .strong()
                        .color(if can_flash { BG_CANVAS } else { TEXT_DIM }),
                )
                .fill(if can_flash { ACCENT } else { BG_SURFACE })
                .stroke(Stroke::new(1.0_f32, if can_flash { ACCENT } else { HAIRLINE }))
                .rounding(Rounding::same(8.0))
                .min_size(Vec2::new(160.0, 34.0));

                if ui.add_enabled(can_flash, flash_btn).clicked() {
                    if let Some(bytes) = self.ota_image_bytes.clone() {
                        {
                            let mut p = self.ota_progress.lock().unwrap();
                            *p = OtaProgress::default();
                            p.running = true;
                            p.stage = s.ota_flashing.to_string();
                        }
                        self.ota_was_running = true;
                        flash_firmware_async(self.hid.clone(), bytes, self.ota_progress.clone());
                    }
                }

                if prog.done && prog.error.is_none() {
                    pill_badge(ui, s.ota_success, MINT);
                    if let Some(crc) = prog.new_crc {
                        pill_badge(ui, &format!("0x{:08X}", crc), CYAN);
                    }
                }
            });

            if prog.running || prog.done || prog.error.is_some() {
                ui.add_space(12.0);
                ui.add(
                    egui::ProgressBar::new(prog.fraction.clamp(0.0, 1.0))
                        .show_percentage()
                        .animate(prog.running),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(&prog.stage)
                        .size(12.0)
                        .color(if prog.error.is_some() { ROSE } else { TEXT_SEC }),
                );
                if let Some(ref err) = prog.error {
                    ui.add_space(4.0);
                    ui.label(RichText::new(err).size(12.0).strong().color(ROSE));
                }
            }
        });
    }
}

// --- Custom Widgets ---

fn nav_rail_button(ui: &mut egui::Ui, label: &str, icon_id: u8, active: bool) -> bool {
    let size = Vec2::new(ui.available_width(), 33.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();

    let bg = if active { ACCENT_SOFT } else if resp.hovered() { BG_SURFACE } else { Color32::TRANSPARENT };
    p.rect_filled(rect, Rounding::same(8.0), bg);
    if active {
        let bar = Rect::from_min_size(rect.min + Vec2::new(0.0, 7.0), Vec2::new(3.0, rect.height() - 14.0));
        p.rect_filled(bar, Rounding::same(2.0), ACCENT);
    }

    let col = if active { ACCENT } else if resp.hovered() { TEXT_PRI } else { TEXT_SEC };
    let ic = Pos2::new(rect.min.x + 20.0, rect.center().y);
    match icon_id {
        0 => {
            let r = Rect::from_center_size(ic, Vec2::new(14.0, 9.5));
            p.rect_stroke(r, Rounding::same(2.0), Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x - 4.0, ic.y + 1.5), Pos2::new(ic.x + 4.0, ic.y + 1.5)], Stroke::new(1.4_f32, col));
        }
        1 => {
            p.circle_stroke(ic, 4.2, Stroke::new(1.5_f32, col));
            p.circle_filled(ic, 1.8, col);
        }
        2 => {
            p.line_segment([Pos2::new(ic.x + 2.0, ic.y - 6.0), Pos2::new(ic.x - 3.0, ic.y + 0.5)], Stroke::new(1.6_f32, col));
            p.line_segment([Pos2::new(ic.x - 3.0, ic.y + 0.5), Pos2::new(ic.x + 2.5, ic.y + 0.5)], Stroke::new(1.6_f32, col));
            p.line_segment([Pos2::new(ic.x + 2.5, ic.y + 0.5), Pos2::new(ic.x - 2.0, ic.y + 6.0)], Stroke::new(1.6_f32, col));
        }
        3 => {
            let r1 = Rect::from_center_size(Pos2::new(ic.x, ic.y - 2.5), Vec2::new(12.0, 5.0));
            p.rect_stroke(r1, Rounding::same(1.5), Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x - 5.5, ic.y + 3.5), Pos2::new(ic.x + 5.5, ic.y + 3.5)], Stroke::new(1.4_f32, col));
        }
        5 => {
            p.line_segment([Pos2::new(ic.x - 6.0, ic.y - 3.0), Pos2::new(ic.x + 6.0, ic.y - 3.0)], Stroke::new(1.4_f32, col));
            p.circle_filled(Pos2::new(ic.x - 2.0, ic.y - 3.0), 2.3, col);
            p.line_segment([Pos2::new(ic.x - 6.0, ic.y + 3.0), Pos2::new(ic.x + 6.0, ic.y + 3.0)], Stroke::new(1.4_f32, col));
            p.circle_filled(Pos2::new(ic.x + 2.5, ic.y + 3.0), 2.3, col);
        }
        6 => {
            // Pulse / key tester icon
            p.line_segment([Pos2::new(ic.x - 6.5, ic.y + 0.5), Pos2::new(ic.x - 2.5, ic.y + 0.5)], Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x - 2.5, ic.y + 0.5), Pos2::new(ic.x - 0.5, ic.y - 5.0)], Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x - 0.5, ic.y - 5.0), Pos2::new(ic.x + 2.0, ic.y + 4.5)], Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x + 2.0, ic.y + 4.5), Pos2::new(ic.x + 4.0, ic.y + 0.5)], Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x + 4.0, ic.y + 0.5), Pos2::new(ic.x + 6.5, ic.y + 0.5)], Stroke::new(1.4_f32, col));
        }
        7 => {
            // OTA firmware upload/flash arrow icon
            p.line_segment([Pos2::new(ic.x, ic.y + 4.0), Pos2::new(ic.x, ic.y - 4.5)], Stroke::new(1.5_f32, col));
            p.line_segment([Pos2::new(ic.x - 3.5, ic.y - 1.5), Pos2::new(ic.x, ic.y - 5.0)], Stroke::new(1.5_f32, col));
            p.line_segment([Pos2::new(ic.x + 3.5, ic.y - 1.5), Pos2::new(ic.x, ic.y - 5.0)], Stroke::new(1.5_f32, col));
            p.line_segment([Pos2::new(ic.x - 5.5, ic.y + 5.5), Pos2::new(ic.x + 5.5, ic.y + 5.5)], Stroke::new(1.4_f32, col));
        }
        _ => {
            p.circle_stroke(ic, 5.5, Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x - 2.5, ic.y), Pos2::new(ic.x + 2.5, ic.y)], Stroke::new(1.4_f32, col));
            p.line_segment([Pos2::new(ic.x, ic.y - 2.5), Pos2::new(ic.x, ic.y + 2.5)], Stroke::new(1.4_f32, col));
        }
    }

    p.text(
        Pos2::new(rect.min.x + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if active { TEXT_PRI } else { col },
    );
    resp.clicked()
}

fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 20.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let bg = if *on { ACCENT } else { Color32::from_rgb(44, 52, 68) };
    p.rect_filled(rect, Rounding::same(10.0), bg);
    let cx = if *on { rect.max.x - 10.0 } else { rect.min.x + 10.0 };
    p.circle_filled(Pos2::new(cx, rect.center().y), 7.2, Color32::WHITE);
    resp.changed()
}

fn primary_button(ui: &mut egui::Ui, label: &str, min_w: f32) -> bool {
    let b = egui::Button::new(RichText::new(label).size(12.0).strong().color(BG_CANVAS))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(min_w, 28.0));
    ui.add(b).clicked()
}

fn chip_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let b = egui::Button::new(RichText::new(label).size(11.5).color(if active { ACCENT } else { TEXT_PRI }))
        .fill(if active { ACCENT_SOFT } else { BG_SURFACE })
        .stroke(Stroke::new(1.0_f32, if active { ACCENT } else { HAIRLINE }))
        .rounding(Rounding::same(6.5))
        .min_size(Vec2::new(0.0, 26.0));
    ui.add(b).clicked()
}

fn danger_button(ui: &mut egui::Ui, label: &str) -> bool {
    let b = egui::Button::new(RichText::new(label).size(11.5).color(ROSE))
        .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 20))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(251, 113, 133, 80)))
        .rounding(Rounding::same(6.5))
        .min_size(Vec2::new(0.0, 26.0));
    ui.add(b).clicked()
}

fn pill_badge(ui: &mut egui::Ui, text: &str, col: Color32) {
    let bg = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 26);
    egui::Frame::none()
        .fill(bg)
        .rounding(Rounding::same(99.0))
        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(col));
        });
}

fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(13.0))
}

fn titlebar_win_btn(ui: &mut egui::Ui, kind: u8) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(38.0, 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        let bg = if kind == 2 { Color32::from_rgb(220, 38, 38) } else { BG_SURFACE_HOV };
        p.rect_filled(rect, Rounding::same(6.0), bg);
    }
    let col = if resp.hovered() && kind == 2 { Color32::WHITE } else if resp.hovered() { TEXT_PRI } else { TEXT_SEC };
    let c = rect.center();
    match kind {
        0 => { p.line_segment([Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)], Stroke::new(1.4_f32, col)); }
        1 => { p.rect_stroke(Rect::from_center_size(c, Vec2::splat(9.0)), Rounding::same(1.5), Stroke::new(1.3_f32, col)); }
        _ => {
            p.line_segment([Pos2::new(c.x - 4.5, c.y - 4.5), Pos2::new(c.x + 4.5, c.y + 4.5)], Stroke::new(1.5_f32, col));
            p.line_segment([Pos2::new(c.x + 4.5, c.y - 4.5), Pos2::new(c.x - 4.5, c.y + 4.5)], Stroke::new(1.5_f32, col));
        }
    }
    resp.clicked()
}

fn mod_label(m: u8) -> &'static str {
    match m {
        1 => "LCtrl", 2 => "LShift", 4 => "LAlt", 8 => "LWin", 3 => "Ctrl+Shift", 15 => "Hyper", _ => "Mod",
    }
}

fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let inv = 1.0 - t;
    Color32::from_rgb(
        (a.r() as f32 * inv + b.r() as f32 * t) as u8,
        (a.g() as f32 * inv + b.g() as f32 * t) as u8,
        (a.b() as f32 * inv + b.b() as f32 * t) as u8,
    )
}

fn hsv_to_color32(hue: u8, sat: u8) -> Color32 {
    let h = (hue as f32 / 255.0) * 6.0;
    let s = sat as f32 / 255.0;
    let v = 0.94f32;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0), 1 => (x, c, 0.0), 2 => (0.0, c, x),
        3 => (0.0, x, c), 4 => (x, 0.0, c), _ => (c, 0.0, x),
    };
    Color32::from_rgb(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

fn find_bundled_firmware(rel_candidates: &[&str]) -> Option<PathBuf> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bases.push(dir.to_path_buf());
            if let Some(p1) = dir.parent() {
                bases.push(p1.to_path_buf());
                if let Some(p2) = p1.parent() {
                    bases.push(p2.to_path_buf());
                    if let Some(p3) = p2.parent() {
                        bases.push(p3.to_path_buf());
                    }
                }
            }
        }
    }
    bases.push(PathBuf::from(r"d:\Workspace\Antigravity\Crush80Driver"));
    bases.push(PathBuf::from(r"d:\Workspace\Antigravity"));

    for base in bases {
        for rel in rel_candidates {
            let p = base.join(rel);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn resolve_data_dir() -> PathBuf {
    let shared = PathBuf::from(r"d:\Workspace\Antigravity\Crush80Driver\studio");
    if shared.exists() {
        return shared;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("data")))
        .unwrap_or_else(|| PathBuf::from("data"))
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\msyh.ttc") {
        fonts.font_data.insert("zh".into(), FontData::from_owned(bytes));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "zh".into());
        fonts.families.entry(FontFamily::Monospace).or_default().push("zh".into());
    }
    // Add Japanese font fallback (Yu Gothic / Meiryo) for crisp kana & Japanese kanji
    for jp in [r"C:\Windows\Fonts\YuGothM.ttc", r"C:\Windows\Fonts\meiryo.ttc"] {
        if let Ok(bytes) = std::fs::read(jp) {
            fonts.font_data.insert("ja".into(), FontData::from_owned(bytes));
            fonts.families.entry(FontFamily::Proportional).or_default().push("ja".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn build_app_icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("../assets/icon_64.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG_CANVAS;
    v.window_fill = BG_CARD;
    v.faint_bg_color = BG_SURFACE;
    v.extreme_bg_color = Color32::from_rgb(13, 16, 23);
    v.override_text_color = Some(TEXT_PRI);
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.menu_rounding = Rounding::same(8.0);
    v.widgets.noninteractive.bg_fill = BG_CARD;
    v.widgets.noninteractive.weak_bg_fill = BG_CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_SEC);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.widgets.inactive.bg_fill = BG_SURFACE;
    v.widgets.inactive.weak_bg_fill = BG_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRI);
    v.widgets.inactive.rounding = Rounding::same(7.0);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.widgets.hovered.bg_fill = BG_SURFACE_HOV;
    v.widgets.hovered.weak_bg_fill = BG_SURFACE_HOV;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRI);
    v.widgets.hovered.rounding = Rounding::same(7.0);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.rounding = Rounding::same(7.0);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.bg_fill = BG_SURFACE_HOV;
    v.widgets.open.weak_bg_fill = BG_SURFACE_HOV;
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.rounding = Rounding::same(7.0);
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    style.visuals = v;
    style.spacing.item_spacing = Vec2::new(10.0, 7.0);
    style.spacing.button_padding = Vec2::new(12.0, 5.5);
    style.spacing.slider_width = 240.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    ctx.set_theme(egui::Theme::Dark);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style(style);
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
