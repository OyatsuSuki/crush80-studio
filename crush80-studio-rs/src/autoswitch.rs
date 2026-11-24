use crate::hid_win::{Crush80Hid, FullSnapshot, RgbState};
use crate::storage::resolve_profile_path;
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

#[derive(Clone, Debug)]
pub enum SwitchTransition {
    ApplyRule(SwitchRule),
    Revert(Option<RgbState>),
    Noop,
}

/// Pure rule-matching state machine for AutoSwitch (tested independently of Win32/HID).
pub fn select_transition(
    cfg: &AutoSwitchConfig,
    active_rule_id: Option<&str>,
    exe: &str,
) -> SwitchTransition {
    if !cfg.enabled || exe.is_empty() {
        return SwitchTransition::Noop;
    }
    let exe_lower = exe.to_ascii_lowercase();
    let matched = cfg
        .rules
        .iter()
        .find(|r| {
            r.enabled
                && r.exe
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .any(|t| !t.is_empty() && t == exe_lower)
        })
        .cloned();

    match matched {
        Some(rule) if active_rule_id != Some(rule.id.as_str()) => {
            SwitchTransition::ApplyRule(rule)
        }
        None if active_rule_id.is_some() => SwitchTransition::Revert(cfg.default_rgb.clone()),
        _ => SwitchTransition::Noop,
    }
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
                rgb: RgbState {
                    brightness: 4,
                    effect: 6,
                    speed: 2,
                    hue: 140,
                    saturation: 40,
                    rainbow: false,
                },
                profile: String::new(),
                enabled: true,
            },
            SwitchRule {
                id: "rule-game".into(),
                name: "FPS 竞技幻彩".into(),
                exe: "cs2.exe, valorant-win64-shipping.exe".into(),
                action: "rgb".into(),
                rgb: RgbState {
                    brightness: 8,
                    effect: 9,
                    speed: 4,
                    hue: 0,
                    saturation: 255,
                    rainbow: true,
                },
                profile: String::new(),
                enabled: true,
            },
        ],
    }
}

pub fn save_config(path: &Path, cfg: &AutoSwitchConfig) -> Result<(), String> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).map_err(|e| format!("创建配置目录失败: {}", e))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("序列化联动规则失败: {}", e))?;
    std::fs::write(path, json).map_err(|e| format!("写入联动配置失败: {}", e))
}

pub fn spawn_watcher(
    hid: Arc<Crush80Hid>,
    shared: Arc<Mutex<AutoSwitchShared>>,
    profiles_dir: PathBuf,
) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_millis(800));
        if hid.is_ota_busy() {
            continue;
        }
        let exe = get_foreground_exe();
        let transition = {
            let Ok(mut st) = shared.lock() else {
                continue;
            };
            st.last_exe = exe.clone();
            select_transition(&st.cfg, st.active_rule_id.as_deref(), &exe)
        };

        match transition {
            SwitchTransition::ApplyRule(rule) => {
                let res = if rule.action == "rgb" {
                    let c = if rule.rgb.rainbow {
                        None
                    } else {
                        Some((rule.rgb.hue, rule.rgb.saturation))
                    };
                    hid.set_rgb_partial(
                        Some(rule.rgb.brightness),
                        Some(rule.rgb.effect),
                        Some(rule.rgb.speed),
                        c,
                        Some(rule.rgb.rainbow),
                        false, // RAM-only, zero Flash wear!
                    )
                } else if rule.action == "profile" && !rule.profile.is_empty() {
                    resolve_profile_path(&profiles_dir, &rule.profile)
                        .and_then(|p| {
                            std::fs::read_to_string(p).map_err(|e| format!("读取方案失败: {}", e))
                        })
                        .and_then(|txt| {
                            serde_json::from_str::<FullSnapshot>(&txt)
                                .map_err(|e| format!("解析方案失败: {}", e))
                        })
                        .and_then(|snap| hid.apply_snapshot_checked(&snap, None, false))
                } else {
                    Ok(())
                };

                // Only commit `active_rule_id` on success so disconnected/sleeping boards retry.
                if res.is_ok() {
                    if let Ok(mut st) = shared.lock() {
                        st.active_rule_id = Some(rule.id);
                    }
                }
            }
            SwitchTransition::Revert(maybe_rgb) => {
                let res = if let Some(rgb) = maybe_rgb {
                    let c = if rgb.rainbow {
                        None
                    } else {
                        Some((rgb.hue, rgb.saturation))
                    };
                    hid.set_rgb_partial(
                        Some(rgb.brightness),
                        Some(rgb.effect),
                        Some(rgb.speed),
                        c,
                        Some(rgb.rainbow),
                        false,
                    )
                } else {
                    Ok(())
                };
                if res.is_ok() {
                    if let Ok(mut st) = shared.lock() {
                        st.active_rule_id = None;
                    }
                }
            }
            SwitchTransition::Noop => {}
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_transition_matches_case_insensitive_csv_and_reverts() {
        let cfg = AutoSwitchConfig {
            enabled: true,
            default_rgb: Some(RgbState {
                brightness: 5,
                effect: 6,
                speed: 2,
                hue: 10,
                saturation: 255,
                rainbow: false,
            }),
            rules: vec![SwitchRule {
                id: "r1".into(),
                name: "Code".into(),
                exe: "Code.EXE, cursor.exe".into(),
                action: "rgb".into(),
                rgb: RgbState {
                    brightness: 8,
                    effect: 1,
                    speed: 3,
                    hue: 0,
                    saturation: 255,
                    rainbow: true,
                },
                profile: String::new(),
                enabled: true,
            }],
        };

        match select_transition(&cfg, None, "cursor.exe") {
            SwitchTransition::ApplyRule(r) => assert_eq!(r.id, "r1"),
            other => panic!("expected ApplyRule, got {:?}", other),
        }
        assert!(matches!(
            select_transition(&cfg, Some("r1"), "code.exe"),
            SwitchTransition::Noop
        ));
        assert!(matches!(
            select_transition(&cfg, Some("r1"), "notepad.exe"),
            SwitchTransition::Revert(Some(_))
        ));
    }
}
