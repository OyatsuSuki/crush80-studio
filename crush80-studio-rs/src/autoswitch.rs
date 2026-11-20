use crate::hid_win::{Crush80Hid, FullSnapshot, RgbState};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwitchRule {
    pub id: String,
    pub name: String,
    pub exe: String,
    pub action: String, // "rgb" or "profile"
    pub rgb: RgbState,
    pub profile: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct AutoSwitchConfig {
    pub enabled: bool,
    pub rules: Vec<SwitchRule>,
    pub default_rgb: Option<RgbState>,
}

pub struct AutoSwitchShared {
    pub cfg: AutoSwitchConfig,
    pub active_rule_id: Option<String>,
    pub last_exe: String,
}

pub fn get_foreground_exe() -> String {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return String::new();
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return String::new();
        }
        let hproc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if hproc.is_null() {
            return String::new();
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(hproc, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(hproc);
        if ok == 0 || len == 0 {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..len as usize]);
        Path::new(&full)
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }
}

pub fn load_config(path: &Path) -> AutoSwitchConfig {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(cfg) = serde_json::from_str::<AutoSwitchConfig>(&text) {
            return cfg;
        }
    }
    AutoSwitchConfig {
        enabled: false,
        default_rgb: None,
        rules: vec![
            SwitchRule {
                id: "rule-vscode".into(),
                name: "VS Code 编程冷白".into(),
                exe: "code.exe, cursor.exe".into(),
                action: "rgb".into(),
                rgb: RgbState { brightness: 4, effect: 6, speed: 2, hue: 140, saturation: 40, rainbow: false },
                profile: String::new(),
                enabled: true,
            },
            SwitchRule {
                id: "rule-game".into(),
                name: "FPS 竞技幻彩".into(),
                exe: "cs2.exe, valorant-win64-shipping.exe".into(),
                action: "rgb".into(),
                rgb: RgbState { brightness: 8, effect: 9, speed: 4, hue: 0, saturation: 255, rainbow: true },
                profile: String::new(),
                enabled: true,
            },
        ],
    }
}

pub fn save_config(path: &Path, cfg: &AutoSwitchConfig) {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    if let Ok(json) = serde_json::to_string_pretty(cfg) {
        let _ = std::fs::write(path, json);
    }
}

pub fn spawn_watcher(hid: Arc<Crush80Hid>, shared: Arc<Mutex<AutoSwitchShared>>, profiles_dir: PathBuf) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(800));
        let exe = get_foreground_exe();
        let (apply_rule, revert_rgb) = {
            let mut st = shared.lock().unwrap();
            st.last_exe = exe.clone();
            if !st.cfg.enabled || exe.is_empty() {
                (None, None)
            } else {
                let matched = st
                    .cfg
                    .rules
                    .iter()
                    .find(|r| {
                        r.enabled
                            && r.exe
                                .split(',')
                                .map(|s| s.trim().to_lowercase())
                                .any(|t| !t.is_empty() && t == exe)
                    })
                    .cloned();

                match matched {
                    Some(rule) if st.active_rule_id.as_deref() != Some(rule.id.as_str()) => {
                        st.active_rule_id = Some(rule.id.clone());
                        (Some(rule), None)
                    }
                    None if st.active_rule_id.is_some() => {
                        st.active_rule_id = None;
                        (None, st.cfg.default_rgb.clone())
                    }
                    _ => (None, None),
                }
            }
        };

        if let Some(rule) = apply_rule {
            if rule.action == "rgb" {
                let c = if rule.rgb.rainbow { None } else { Some((rule.rgb.hue, rule.rgb.saturation)) };
                let _ = hid.set_rgb_partial(
                    Some(rule.rgb.brightness),
                    Some(rule.rgb.effect),
                    Some(rule.rgb.speed),
                    c,
                    Some(rule.rgb.rainbow),
                    false, // RAM-only, zero Flash wear!
                );
            } else if rule.action == "profile" && !rule.profile.is_empty() {
                let p = profiles_dir.join(format!("{}.json", rule.profile));
                if let Ok(txt) = std::fs::read_to_string(p) {
                    if let Ok(snap) = serde_json::from_str::<FullSnapshot>(&txt) {
                        let _ = hid.apply_snapshot(&snap);
                    }
                }
            }
        } else if let Some(rgb) = revert_rgb {
            let c = if rgb.rainbow { None } else { Some((rgb.hue, rgb.saturation)) };
            let _ = hid.set_rgb_partial(
                Some(rgb.brightness),
                Some(rgb.effect),
                Some(rgb.speed),
                c,
                Some(rgb.rainbow),
                false,
            );
        }
    });
}
