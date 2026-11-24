//! Dedicated non-blocking HID worker & tray-aware live-sync background service.
//! Ensures `eframe::App::update` performs ZERO blocking HID I/O, coalesces high-frequency
//! RGB/slider bursts, slows polling when hidden to tray, and guards against TOCTOU UI bounce.

use crate::hid_win::{Crush80Hid, DeviceStatus, FullSnapshot, HardwareConfig, RgbState};
use crate::i18n::{tr, Lang};
use crate::keycodes::BoardEdition;
use crate::tray::TrayState;
use crate::win_sys::{query_bluetooth_crush80, BluetoothBatteryInfo};
use eframe::egui;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const PID_BLUETOOTH: u16 = 0xB700;
const UI_EDIT_GRACE: Duration = Duration::from_millis(1200);
const COALESCE_WINDOW: Duration = Duration::from_millis(30);

#[derive(Clone, Debug)]
pub enum HidCmd {
    RefreshAll,
    SetKey {
        layer: u8,
        row: u8,
        col: u8,
        keycode: u16,
    },
    RestoreLayer {
        layer: u8,
        keys: Vec<(u8, u8, u16)>,
    },
    SetRgb {
        brightness: Option<u8>,
        effect: Option<u8>,
        speed: Option<u8>,
        color: Option<(u8, u8)>,
        rainbow: Option<bool>,
        save: bool,
    },
    SetConfigByte {
        id: u8,
        val: u8,
        save: bool,
    },
    TriggerLedFeedback {
        count: u8,
        r: u8,
        g: u8,
        b: u8,
    },
    WriteMacroBuffer {
        raw: Vec<u8>,
    },
    ExportSnapshot {
        path: PathBuf,
        edition: BoardEdition,
        label: String,
    },
    ApplySnapshot {
        snap: FullSnapshot,
        edition: Option<BoardEdition>,
        save_to_flash: bool,
        label: String,
    },
}

#[derive(Clone, Debug)]
pub enum HidEvent {
    FullRefresh {
        status: DeviceStatus,
        layers: Option<Box<[[u16; 128]; 4]>>,
        macros_raw: Option<Vec<u8>>,
        rgb: Option<RgbState>,
        hw: Option<HardwareConfig>,
        bt_info: Option<BluetoothBatteryInfo>,
        warnings: Vec<String>,
    },
    LiveUpdate {
        connected: bool,
        pid: u16,
        rgb: Option<RgbState>,
        hw: Option<HardwareConfig>,
        bt_info: Option<BluetoothBatteryInfo>,
    },
    KeyUpdated {
        layer: u8,
        row: u8,
        col: u8,
        keycode: u16,
    },
    LayerRestored {
        layer: u8,
        count: usize,
    },
    SnapshotExported {
        label: String,
    },
    SnapshotApplied {
        label: String,
    },
    ToastOk(String),
    ToastErr(String),
}

/// Coalesces adjacent `SetRgb`, same-id `SetConfigByte`, or duplicate `RefreshAll` commands.
pub fn coalesce_next_cmd(first: HidCmd, queue: &mut VecDeque<HidCmd>) -> HidCmd {
    match first {
        HidCmd::SetRgb {
            mut brightness,
            mut effect,
            mut speed,
            mut color,
            mut rainbow,
            mut save,
        } => {
            while let Some(HidCmd::SetRgb { .. }) = queue.front() {
                if let Some(HidCmd::SetRgb {
                    brightness: b2,
                    effect: e2,
                    speed: sp2,
                    color: c2,
                    rainbow: rb2,
                    save: s2,
                }) = queue.pop_front()
                {
                    if b2.is_some() {
                        brightness = b2;
                    }
                    if e2.is_some() {
                        effect = e2;
                    }
                    if sp2.is_some() {
                        speed = sp2;
                    }
                    if c2.is_some() {
                        color = c2;
                    }
                    if rb2.is_some() {
                        rainbow = rb2;
                    }
                    save |= s2;
                }
            }
            HidCmd::SetRgb {
                brightness,
                effect,
                speed,
                color,
                rainbow,
                save,
            }
        }
        HidCmd::SetConfigByte {
            id,
            mut val,
            mut save,
        } => {
            while matches!(queue.front(), Some(HidCmd::SetConfigByte { id: id2, .. }) if *id2 == id)
            {
                if let Some(HidCmd::SetConfigByte {
                    val: v2,
                    save: s2,
                    ..
                }) = queue.pop_front()
                {
                    val = v2;
                    save |= s2;
                }
            }
            HidCmd::SetConfigByte { id, val, save }
        }
        HidCmd::RefreshAll => {
            while matches!(queue.front(), Some(HidCmd::RefreshAll)) {
                queue.pop_front();
            }
            HidCmd::RefreshAll
        }
        other => other,
    }
}

pub fn should_publish_live_update(last_ui_edit: Instant, now: Instant) -> bool {
    now.saturating_duration_since(last_ui_edit) >= UI_EDIT_GRACE
}

pub fn live_poll_interval(window_hidden: bool) -> Duration {
    if window_hidden {
        Duration::from_secs(8)
    } else {
        Duration::from_secs(1)
    }
}

pub fn ble_poll_interval() -> Duration {
    Duration::from_secs(12)
}

fn update_tray_tip(tray: &TrayState, lang: Lang, connected: bool, pid: u16, battery: Option<u8>) {
    let s = tr(lang);
    let tip = if !connected {
        format!("Crush 80 Studio · {}", s.offline)
    } else {
        let mode = if pid == 0x5055 {
            s.usb_wired
        } else if pid == PID_BLUETOOTH {
            s.bluetooth
        } else {
            s.wireless_24g
        };
        if let Some(bat) = battery {
            format!("Crush 80 Studio · {} · {}%", mode, bat)
        } else {
            format!("Crush 80 Studio · {}", mode)
        }
    };
    tray.update_tooltip(tip);
}

fn now_unix_ts() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}", secs)
}

pub struct HidWorkerHandle {
    pub tx: mpsc::Sender<HidCmd>,
    pub rx: mpsc::Receiver<HidEvent>,
    last_ui_edit: Arc<Mutex<Instant>>,
    lang: Arc<Mutex<Lang>>,
}

impl HidWorkerHandle {
    pub fn spawn(
        hid: Arc<Crush80Hid>,
        tray: Arc<TrayState>,
        ctx: egui::Context,
        initial_lang: Lang,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<HidCmd>();
        let (ev_tx, ev_rx) = mpsc::channel::<HidEvent>();
        let last_ui_edit = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(5)));
        let lang = Arc::new(Mutex::new(initial_lang));

        // 1. Command Worker Thread (handles all UI-initiated HID I/O off the UI thread)
        {
            let hid = hid.clone();
            let tray = tray.clone();
            let ctx = ctx.clone();
            let ev_tx = ev_tx.clone();
            let lang = lang.clone();
            std::thread::spawn(move || {
                let mut pending: VecDeque<HidCmd> = VecDeque::new();
                loop {
                    let first = if let Some(cmd) = pending.pop_front() {
                        cmd
                    } else {
                        match cmd_rx.recv() {
                            Ok(c) => c,
                            Err(_) => break,
                        }
                    };

                    // Brief coalescing window for rapid RGB/slider bursts
                    if matches!(first, HidCmd::SetRgb { .. } | HidCmd::SetConfigByte { .. }) {
                        std::thread::sleep(COALESCE_WINDOW);
                    }
                    while let Ok(more) = cmd_rx.try_recv() {
                        pending.push_back(more);
                    }
                    let cmd = coalesce_next_cmd(first, &mut pending);

                    let cur_lang = lang.lock().map(|l| *l).unwrap_or(Lang::Zh);
                    let emit = |ev: HidEvent| {
                        let _ = ev_tx.send(ev);
                        ctx.request_repaint();
                    };

                    match cmd {
                        HidCmd::RefreshAll => {
                            let mut status = hid.get_status();
                            let mut warnings = Vec::new();
                            let mut layers = None;
                            let mut macros_raw = None;
                            let mut rgb = None;
                            let mut hw = None;
                            let mut bt_info = None;

                            if status.connected {
                                match hid.read_keymap() {
                                    Ok(km) => layers = Some(Box::new(km)),
                                    Err(e) => warnings.push(format!("键位读取失败: {}", e)),
                                }
                                match hid.read_macros() {
                                    Ok((_, raw)) => macros_raw = Some(raw),
                                    Err(e) => warnings.push(format!("宏读取失败: {}", e)),
                                }
                                match hid.read_rgb() {
                                    Ok(r) => rgb = Some(r),
                                    Err(e) => warnings.push(format!("灯效读取失败: {}", e)),
                                }
                                match hid.read_hw_config() {
                                    Ok(h) => {
                                        update_tray_tip(&tray, cur_lang, true, status.pid, h.battery);
                                        hw = Some(h);
                                    }
                                    Err(e) => warnings.push(format!("硬件参数读取失败: {}", e)),
                                }
                            } else if let Some(bt) = query_bluetooth_crush80() {
                                status.connected = true;
                                status.pid = PID_BLUETOOTH;
                                status.firmware_crc = None;
                                status.firmware_name = bt.name.clone();
                                update_tray_tip(&tray, cur_lang, true, PID_BLUETOOTH, bt.battery);
                                bt_info = Some(bt);
                            } else {
                                update_tray_tip(&tray, cur_lang, false, 0, None);
                            }

                            emit(HidEvent::FullRefresh {
                                status,
                                layers,
                                macros_raw,
                                rgb,
                                hw,
                                bt_info,
                                warnings,
                            });
                        }
                        HidCmd::SetKey {
                            layer,
                            row,
                            col,
                            keycode,
                        } => match hid.set_key(layer, row, col, keycode) {
                            Ok(()) => emit(HidEvent::KeyUpdated {
                                layer,
                                row,
                                col,
                                keycode,
                            }),
                            Err(e) => emit(HidEvent::ToastErr(e)),
                        },
                        HidCmd::RestoreLayer { layer, keys } => {
                            let mut written = 0usize;
                            let mut first_err: Option<String> = None;
                            for (r, c, kc) in keys {
                                match hid.set_key(layer, r, c, kc) {
                                    Ok(()) => written += 1,
                                    Err(e) => {
                                        first_err = Some(e);
                                        break;
                                    }
                                }
                            }
                            if let Some(err) = first_err {
                                emit(HidEvent::ToastErr(format!(
                                    "恢复层 L{} 中断（已写入 {} 键）: {}",
                                    layer, written, err
                                )));
                            } else {
                                emit(HidEvent::LayerRestored {
                                    layer,
                                    count: written,
                                });
                            }
                        }
                        HidCmd::SetRgb {
                            brightness,
                            effect,
                            speed,
                            color,
                            rainbow,
                            save,
                        } => {
                            if let Err(e) =
                                hid.set_rgb_partial(brightness, effect, speed, color, rainbow, save)
                            {
                                emit(HidEvent::ToastErr(e));
                            } else if save {
                                emit(HidEvent::ToastOk(tr(cur_lang).saved_flash.to_string()));
                            }
                        }
                        HidCmd::SetConfigByte { id, val, save } => {
                            if let Err(e) = hid.set_config_byte(id, val, save) {
                                emit(HidEvent::ToastErr(e));
                            } else if save {
                                emit(HidEvent::ToastOk(tr(cur_lang).saved_flash.to_string()));
                            }
                        }
                        HidCmd::TriggerLedFeedback { count, r, g, b } => {
                            hid.trigger_led_feedback(count, r, g, b);
                        }
                        HidCmd::WriteMacroBuffer { raw } => match hid.write_macro_buffer(&raw) {
                            Ok(()) => emit(HidEvent::ToastOk(tr(cur_lang).saved_flash.to_string())),
                            Err(e) => emit(HidEvent::ToastErr(e)),
                        },
                        HidCmd::ExportSnapshot {
                            path,
                            edition,
                            label,
                        } => match hid.export_snapshot_for(now_unix_ts(), edition) {
                            Ok(snap) => {
                                let res = path
                                    .parent()
                                    .map(std::fs::create_dir_all)
                                    .transpose()
                                    .map_err(|e| format!("创建目录失败: {}", e))
                                    .and_then(|_| {
                                        serde_json::to_string_pretty(&snap)
                                            .map_err(|e| format!("序列化失败: {}", e))
                                    })
                                    .and_then(|json| {
                                        std::fs::write(&path, json)
                                            .map_err(|e| format!("写入文件失败: {}", e))
                                    });
                                match res {
                                    Ok(()) => emit(HidEvent::SnapshotExported { label }),
                                    Err(e) => emit(HidEvent::ToastErr(e)),
                                }
                            }
                            Err(e) => emit(HidEvent::ToastErr(e)),
                        },
                        HidCmd::ApplySnapshot {
                            snap,
                            edition,
                            save_to_flash,
                            label,
                        } => match hid.apply_snapshot_checked(&snap, edition, save_to_flash) {
                            Ok(()) => emit(HidEvent::SnapshotApplied { label }),
                            Err(e) => emit(HidEvent::ToastErr(e)),
                        },
                    }
                }
            });
        }

        // 2. Tray-Aware Live-Sync Thread (with locked TOCTOU check & BLE throttling)
        {
            let hid = hid.clone();
            let tray = tray.clone();
            let ctx = ctx.clone();
            let ev_tx = ev_tx.clone();
            let last_ui_edit = last_ui_edit.clone();
            let lang = lang.clone();
            std::thread::spawn(move || {
                let mut last_ble_probe = Instant::now() - ble_poll_interval();
                let mut cached_bt: Option<BluetoothBatteryInfo> = None;
                let mut last_connected = false;
                let mut last_pid = 0u16;
                let mut last_rgb: Option<RgbState> = None;
                let mut last_hw: Option<HardwareConfig> = None;

                loop {
                    let hidden = tray.window_hidden.load(Ordering::Relaxed);
                    std::thread::sleep(live_poll_interval(hidden));
                    if hid.is_ota_busy() {
                        continue;
                    }
                    let recent_edit = last_ui_edit
                        .lock()
                        .map(|t| !should_publish_live_update(*t, Instant::now()))
                        .unwrap_or(false);
                    if recent_edit {
                        continue;
                    }

                    let cur_lang = lang.lock().map(|l| *l).unwrap_or(Lang::Zh);

                    match hid.poll_live_state() {
                        Ok((rgb, hw, pid)) => {
                            // Re-check `last_ui_edit` AFTER polling to eliminate TOCTOU UI bounce!
                            let still_safe = last_ui_edit
                                .lock()
                                .map(|t| should_publish_live_update(*t, Instant::now()))
                                .unwrap_or(true);
                            if !still_safe {
                                continue;
                            }
                            cached_bt = None;
                            update_tray_tip(&tray, cur_lang, true, pid, hw.battery);

                            let changed = !last_connected
                                || last_pid != pid
                                || last_rgb.as_ref() != Some(&rgb)
                                || last_hw.as_ref() != Some(&hw);
                            if changed {
                                last_connected = true;
                                last_pid = pid;
                                last_rgb = Some(rgb.clone());
                                last_hw = Some(hw.clone());
                                let _ = ev_tx.send(HidEvent::LiveUpdate {
                                    connected: true,
                                    pid,
                                    rgb: Some(rgb),
                                    hw: Some(hw),
                                    bt_info: None,
                                });
                                ctx.request_repaint();
                            }
                        }
                        Err(_) => {
                            // Only scan Windows PnP Bluetooth devices every 12s when USB/2.4G is offline
                            if last_ble_probe.elapsed() >= ble_poll_interval() {
                                cached_bt = query_bluetooth_crush80();
                                last_ble_probe = Instant::now();
                            }
                            if let Some(bt) = cached_bt.clone() {
                                update_tray_tip(&tray, cur_lang, true, PID_BLUETOOTH, bt.battery);
                                let changed = !last_connected
                                    || last_pid != PID_BLUETOOTH
                                    || last_hw.as_ref().and_then(|h| h.battery) != bt.battery;
                                if changed {
                                    last_connected = true;
                                    last_pid = PID_BLUETOOTH;
                                    let hw = HardwareConfig {
                                        battery: bt.battery,
                                        ..Default::default()
                                    };
                                    last_hw = Some(hw.clone());
                                    let _ = ev_tx.send(HidEvent::LiveUpdate {
                                        connected: true,
                                        pid: PID_BLUETOOTH,
                                        rgb: None,
                                        hw: Some(hw),
                                        bt_info: Some(bt),
                                    });
                                    ctx.request_repaint();
                                }
                            } else if last_connected {
                                last_connected = false;
                                last_pid = 0;
                                last_rgb = None;
                                last_hw = None;
                                update_tray_tip(&tray, cur_lang, false, 0, None);
                                let _ = ev_tx.send(HidEvent::LiveUpdate {
                                    connected: false,
                                    pid: 0,
                                    rgb: None,
                                    hw: None,
                                    bt_info: None,
                                });
                                ctx.request_repaint();
                            }
                        }
                    }
                }
            });
        }

        Self {
            tx: cmd_tx,
            rx: ev_rx,
            last_ui_edit,
            lang,
        }
    }

    pub fn send(&self, cmd: HidCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn mark_ui_edit(&self) {
        if let Ok(mut t) = self.last_ui_edit.lock() {
            *t = Instant::now();
        }
    }

    pub fn set_lang(&self, lang: Lang) {
        if let Ok(mut l) = self.lang.lock() {
            *l = lang;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coalesce_rgb_commands_merges_pending_slider_burst() {
        let mut q = VecDeque::new();
        for b in 1..=9 {
            q.push_back(HidCmd::SetRgb {
                brightness: Some(b),
                effect: None,
                speed: None,
                color: None,
                rainbow: None,
                save: b == 5,
            });
        }
        q.push_back(HidCmd::SetRgb {
            brightness: None,
            effect: Some(6),
            speed: Some(3),
            color: Some((120, 200)),
            rainbow: Some(false),
            save: false,
        });
        let first = q.pop_front().unwrap();
        let merged = coalesce_next_cmd(first, &mut q);
        assert!(q.is_empty());
        match merged {
            HidCmd::SetRgb {
                brightness,
                effect,
                speed,
                color,
                rainbow,
                save,
            } => {
                assert_eq!(brightness, Some(9));
                assert_eq!(effect, Some(6));
                assert_eq!(speed, Some(3));
                assert_eq!(color, Some((120, 200)));
                assert_eq!(rainbow, Some(false));
                assert!(save, "save=true in burst must be preserved");
            }
            other => panic!("expected SetRgb, got {:?}", other),
        }
    }

    #[test]
    fn test_live_sync_skips_stale_publish_when_ui_edited_mid_poll() {
        let now = Instant::now();
        let recent = now - Duration::from_millis(400);
        let old = now - Duration::from_millis(1500);
        assert!(!should_publish_live_update(recent, now));
        assert!(should_publish_live_update(old, now));
    }

    #[test]
    fn test_poll_interval_slows_when_hidden_to_tray() {
        assert_eq!(live_poll_interval(false), Duration::from_secs(1));
        assert_eq!(live_poll_interval(true), Duration::from_secs(8));
        assert_eq!(ble_poll_interval(), Duration::from_secs(12));
    }
}
