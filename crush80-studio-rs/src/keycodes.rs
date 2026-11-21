use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoardEdition {
    ProRgb,
    LiteNoRgb,
}

impl Default for BoardEdition {
    fn default() -> Self {
        BoardEdition::ProRgb
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCategory {
    Basic,
    Special,
    Layer,
    TapHold,
    Mods,
    Macro,
    Raw,
}

#[derive(Clone, Debug)]
pub struct DecodedKey {
    pub code: u16,
    pub name: String,
    pub label: String,
    pub category: KeyCategory,
}

#[derive(Clone, Copy)]
pub struct PhysicalKey {
    pub r: u8,
    pub c: u8,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub style: u8, // 0=alpha, 1=mod, 2=accent, 3=space
}

pub const BASIC_KEYCODES: &[(u16, &str, &str)] = &[
    (0x0000, "KC_NO", "空"),
    (0x0001, "KC_TRNS", "▽"),
    (0x0004, "KC_A", "A"), (0x0005, "KC_B", "B"), (0x0006, "KC_C", "C"), (0x0007, "KC_D", "D"),
    (0x0008, "KC_E", "E"), (0x0009, "KC_F", "F"), (0x000A, "KC_G", "G"), (0x000B, "KC_H", "H"),
    (0x000C, "KC_I", "I"), (0x000D, "KC_J", "J"), (0x000E, "KC_K", "K"), (0x000F, "KC_L", "L"),
    (0x0010, "KC_M", "M"), (0x0011, "KC_N", "N"), (0x0012, "KC_O", "O"), (0x0013, "KC_P", "P"),
    (0x0014, "KC_Q", "Q"), (0x0015, "KC_R", "R"), (0x0016, "KC_S", "S"), (0x0017, "KC_T", "T"),
    (0x0018, "KC_U", "U"), (0x0019, "KC_V", "V"), (0x001A, "KC_W", "W"), (0x001B, "KC_X", "X"),
    (0x001C, "KC_Y", "Y"), (0x001D, "KC_Z", "Z"),
    (0x001E, "KC_1", "1"), (0x001F, "KC_2", "2"), (0x0020, "KC_3", "3"), (0x0021, "KC_4", "4"),
    (0x0022, "KC_5", "5"), (0x0023, "KC_6", "6"), (0x0024, "KC_7", "7"), (0x0025, "KC_8", "8"),
    (0x0026, "KC_9", "9"), (0x0027, "KC_0", "0"),
    (0x0028, "KC_ENT", "Enter"), (0x0029, "KC_ESC", "Esc"), (0x002A, "KC_BSPC", "Bksp"),
    (0x002B, "KC_TAB", "Tab"), (0x002C, "KC_SPC", "Space"),
    (0x002D, "KC_MINS", "-"), (0x002E, "KC_EQL", "="), (0x002F, "KC_LBRC", "["),
    (0x0030, "KC_RBRC", "]"), (0x0031, "KC_BSLS", "\\"), (0x0032, "KC_NUHS", "#"), (0x0033, "KC_SCLN", ";"),
    (0x0034, "KC_QUOT", "'"), (0x0035, "KC_GRV", "`"), (0x0036, "KC_COMM", ","),
    (0x0037, "KC_DOT", "."), (0x0038, "KC_SLSH", "/"), (0x0039, "KC_CAPS", "Caps"),
    (0x003A, "KC_F1", "F1"), (0x003B, "KC_F2", "F2"), (0x003C, "KC_F3", "F3"), (0x003D, "KC_F4", "F4"),
    (0x003E, "KC_F5", "F5"), (0x003F, "KC_F6", "F6"), (0x0040, "KC_F7", "F7"), (0x0041, "KC_F8", "F8"),
    (0x0042, "KC_F9", "F9"), (0x0043, "KC_F10", "F10"), (0x0044, "KC_F11", "F11"), (0x0045, "KC_F12", "F12"),
    (0x0046, "KC_PSCR", "PrtSc"), (0x0047, "KC_SCRL", "ScrLk"), (0x0048, "KC_PAUS", "Pause"),
    (0x0049, "KC_INS", "Ins"), (0x004A, "KC_HOME", "Home"), (0x004B, "KC_PGUP", "PgUp"),
    (0x004C, "KC_DEL", "Del"), (0x004D, "KC_END", "End"), (0x004E, "KC_PGDN", "PgDn"),
    (0x004F, "KC_RGHT", "→"), (0x0050, "KC_LEFT", "←"), (0x0051, "KC_DOWN", "↓"), (0x0052, "KC_UP", "↑"),
    (0x0064, "KC_NUBS", "\\"), (0x0065, "KC_APP", "Menu"),
    (0x0068, "KC_F13", "F13"), (0x0069, "KC_F14", "F14"), (0x006A, "KC_F15", "F15"), (0x006B, "KC_F16", "F16"),
    (0x006C, "KC_F17", "F17"), (0x006D, "KC_F18", "F18"), (0x006E, "KC_F19", "F19"), (0x006F, "KC_F20", "F20"),
    (0x0070, "KC_F21", "F21"), (0x0071, "KC_F22", "F22"), (0x0072, "KC_F23", "F23"), (0x0073, "KC_F24", "F24"),
    (0x00A5, "KC_PWR", "电源"), (0x00A6, "KC_SLEP", "休眠"), (0x00A7, "KC_WAKE", "唤醒"),
    (0x00A8, "KC_MUTE", "静音"), (0x00A9, "KC_VOLU", "音量+"), (0x00AA, "KC_VOLD", "音量-"),
    (0x00AB, "KC_MNXT", "下一曲"), (0x00AC, "KC_MPRV", "上一曲"), (0x00AD, "KC_MSTP", "停止"),
    (0x00AE, "KC_MPLY", "播放"), (0x00AF, "KC_MSEL", "媒体"),
    (0x00B1, "KC_MAIL", "邮件"), (0x00B2, "KC_CALC", "计算器"), (0x00B3, "KC_MYCM", "电脑"),
    (0x00B4, "KC_WSCH", "搜索"), (0x00B5, "KC_WHOM", "主页"), (0x00B6, "KC_WBAK", "后退"),
    (0x00B7, "KC_WFWD", "前进"), (0x00BD, "KC_BRIU", "屏亮+"), (0x00BE, "KC_BRID", "屏亮-"),
    (0x00E0, "KC_LCTL", "LCtrl"), (0x00E1, "KC_LSFT", "LShift"),
    (0x00E2, "KC_LALT", "LAlt"), (0x00E3, "KC_LGUI", "LWin"),
    (0x00E4, "KC_RCTL", "RCtrl"), (0x00E5, "KC_RSFT", "RShift"),
    (0x00E6, "KC_RALT", "RAlt"), (0x00E7, "KC_RGUI", "RWin"),
];

pub const SPECIAL_KEYCODES: &[(u16, &str, &str)] = &[
    (0x7820, "RGB_TOG", "灯开关"), (0x7821, "RGB_MOD", "灯效+"), (0x7822, "RGB_RMOD", "灯效-"),
    (0x7823, "RGB_HUI", "色相+"), (0x7824, "RGB_HUD", "色相-"),
    (0x7825, "RGB_SAI", "饱和+"), (0x7826, "RGB_SAD", "饱和-"),
    (0x7827, "RGB_VAI", "亮度+"), (0x7828, "RGB_VAD", "亮度-"),
    (0x7829, "RGB_SPI", "速度+"), (0x782A, "RGB_SPD", "速度-"),
    (0x7C00, "QK_BOOT", "重置"), (0x7C01, "DB_TOGG", "调试"),
    (0x7C02, "EE_CLR", "清配置"), (0x7C03, "QK_RBT", "重启"),
    (0xFFFF, "KC_TRNS", "▽"),
    (0x7C16, "KC_GESC", "Esc/~"), (0x5C16, "KC_GESC", "Esc/~"), (0x7C73, "CW_TOGG", "CapsW"),
    (0x7F00, "WOB_24G", "2.4G"), (0x7F01, "WOB_BT1", "BT 1"),
    (0x7F02, "WOB_BT2", "BT 2"), (0x7F03, "WOB_BT3", "BT 3"),
    (0x7F04, "WOB_MODE", "三模切"), (0x7F05, "WOB_WINL", "锁Win"),
    (0x7F06, "WOB_OS", "Win/Mac"), (0x7F07, "WOB_LMOD", "Logo效"),
    (0x7F08, "WOB_LCOL", "Logo色"), (0x7F09, "WOB_LPWR", "Logo灯"),
    (0x7F0A, "WOB_SMOD", "侧灯效"), (0x7F0B, "WOB_SCOL", "侧灯色"),
    (0x7F0C, "WOB_SPWR", "侧灯开"), (0x7F0D, "WOB_SCAN", "低延迟"),
    (0x7F0E, "WOB_RGBP", "总灯开"), (0x7F0F, "WOB_BATT", "电量"),
];

/// Exact custom keycode table from official Crush 80 Non-RGB (Lite) VIA JSON.
/// Note: 0x7F07..0x7F0C differ from Pro RGB because side-light codes are absent.
pub const SPECIAL_KEYCODES_NORGB: &[(u16, &str, &str)] = &[
    (0x7820, "RGB_TOG", "灯开关"), (0x7821, "RGB_MOD", "灯效+"), (0x7822, "RGB_RMOD", "灯效-"),
    (0x7823, "RGB_HUI", "色相+"), (0x7824, "RGB_HUD", "色相-"),
    (0x7825, "RGB_SAI", "饱和+"), (0x7826, "RGB_SAD", "饱和-"),
    (0x7827, "RGB_VAI", "亮度+"), (0x7828, "RGB_VAD", "亮度-"),
    (0x7829, "RGB_SPI", "速度+"), (0x782A, "RGB_SPD", "速度-"),
    (0x7C00, "QK_BOOT", "重置"), (0x7C01, "DB_TOGG", "调试"),
    (0x7C02, "EE_CLR", "清配置"), (0x7C03, "QK_RBT", "重启"),
    (0xFFFF, "KC_TRNS", "▽"),
    (0x7C16, "KC_GESC", "Esc/~"), (0x5C16, "KC_GESC", "Esc/~"), (0x7C73, "CW_TOGG", "CapsW"),
    (0x7F00, "WOB_24G", "2.4G"), (0x7F01, "WOB_BT1", "BT 1"),
    (0x7F02, "WOB_BT2", "BT 2"), (0x7F03, "WOB_BT3", "BT 3"),
    (0x7F04, "WOB_MODE", "三模切"), (0x7F05, "WOB_WINL", "锁Win"),
    (0x7F06, "WOB_OS", "Win/Mac"), (0x7F07, "WOB_SCAN", "低延迟"),
    (0x7F08, "WOB_LMOD", "Logo效"), (0x7F09, "WOB_LCOL", "Logo色"),
    (0x7F0A, "WOB_LPWR", "Logo灯"), (0x7F0B, "WOB_RGBP", "总灯开"),
    (0x7F0C, "WOB_BATT", "电量"),
];

pub fn special_keycodes_for(edition: BoardEdition) -> &'static [(u16, &'static str, &'static str)] {
    match edition {
        BoardEdition::ProRgb => SPECIAL_KEYCODES,
        BoardEdition::LiteNoRgb => SPECIAL_KEYCODES_NORGB,
    }
}

pub const RGB_EFFECTS: &[(u8, &str)] = &[
    (0, "00 · 关闭背光"),
    (1, "01 · 彩虹波浪"),
    (2, "02 · 七彩云朵"),
    (3, "03 · 旋涡流光"),
    (4, "04 · 混彩渐变"),
    (5, "05 · 呼吸模式"),
    (6, "06 · 纯色常亮"),
    (7, "07 · 单点亮起"),
    (8, "08 · 涟漪扩散"),
    (9, "09 · 激光横扫"),
    (10, "10 · 繁星点点"),
    (11, "11 · 百花齐放 (重启易变02)"),
    (12, "12 · 穿梭流光"),
    (13, "13 · 律动波纹"),
    (14, "14 · 流星雨"),
    (15, "15 · 矩阵雨滴"),
    (16, "16 · 往复扫描"),
    (17, "17 · 触发变色"),
    (18, "18 · 中心扩散"),
];

fn localized_label(code: u16, lang: crate::i18n::Lang) -> Option<&'static str> {
    use crate::i18n::Lang;
    match lang {
        Lang::Zh => None, // BASIC_KEYCODES and SPECIAL_KEYCODES already hold Zh labels
        Lang::En => match code {
            0x0000 => Some("None"),
            0x00A5 => Some("Pwr"),
            0x00A6 => Some("Sleep"),
            0x00A7 => Some("Wake"),
            0x00A8 => Some("Mute"),
            0x00A9 => Some("Vol+"),
            0x00AA => Some("Vol-"),
            0x00AB => Some("Next"),
            0x00AC => Some("Prev"),
            0x00AD => Some("Stop"),
            0x00AE => Some("Play"),
            0x00AF => Some("Media"),
            0x00B1 => Some("Mail"),
            0x00B2 => Some("Calc"),
            0x00B3 => Some("MyPC"),
            0x00B4 => Some("Search"),
            0x00B5 => Some("Home"),
            0x00B6 => Some("Back"),
            0x00B7 => Some("Fwd"),
            0x00BD => Some("Bri+"),
            0x00BE => Some("Bri-"),
            0x7820 => Some("RGB"),
            0x7821 => Some("Mode+"),
            0x7822 => Some("Mode-"),
            0x7823 => Some("Hue+"),
            0x7824 => Some("Hue-"),
            0x7825 => Some("Sat+"),
            0x7826 => Some("Sat-"),
            0x7827 => Some("Val+"),
            0x7828 => Some("Val-"),
            0x7829 => Some("Spd+"),
            0x782A => Some("Spd-"),
            0x7C00 => Some("Boot"),
            0x7C01 => Some("Debug"),
            0x7C02 => Some("Reset"),
            0x7C03 => Some("Reboot"),
            0x7F04 => Some("Mode"),
            0x7F05 => Some("WinLk"),
            0x7F07 => Some("LogoM"),
            0x7F08 => Some("LogoC"),
            0x7F09 => Some("Logo"),
            0x7F0A => Some("SideM"),
            0x7F0B => Some("SideC"),
            0x7F0C => Some("Side"),
            0x7F0D => Some("LoLat"),
            0x7F0E => Some("AllLED"),
            0x7F0F => Some("Batt"),
            _ => None,
        },
        Lang::Ja => match code {
            0x0000 => Some("空"),
            0x00A5 => Some("電源"),
            0x00A6 => Some("休止"),
            0x00A7 => Some("復帰"),
            0x00A8 => Some("消音"),
            0x00A9 => Some("音量+"),
            0x00AA => Some("音量-"),
            0x00AB => Some("次曲"),
            0x00AC => Some("前曲"),
            0x00AD => Some("停止"),
            0x00AE => Some("再生"),
            0x00AF => Some("メディア"),
            0x00B1 => Some("メール"),
            0x00B2 => Some("電卓"),
            0x00B3 => Some("PC"),
            0x00B4 => Some("検索"),
            0x00B5 => Some("ホーム"),
            0x00B6 => Some("戻る"),
            0x00B7 => Some("進む"),
            0x00BD => Some("輝度+"),
            0x00BE => Some("輝度-"),
            0x7820 => Some("RGB切"),
            0x7821 => Some("効果+"),
            0x7822 => Some("効果-"),
            0x7823 => Some("色相+"),
            0x7824 => Some("色相-"),
            0x7825 => Some("彩度+"),
            0x7826 => Some("彩度-"),
            0x7827 => Some("明度+"),
            0x7828 => Some("明度-"),
            0x7829 => Some("速度+"),
            0x782A => Some("速度-"),
            0x7C00 => Some("書込"),
            0x7C01 => Some("デバッグ"),
            0x7C02 => Some("初期化"),
            0x7C03 => Some("再起動"),
            0x7F04 => Some("接続切"),
            0x7F05 => Some("Win錠"),
            0x7F07 => Some("ロゴ効"),
            0x7F08 => Some("ロゴ色"),
            0x7F09 => Some("ロゴ灯"),
            0x7F0A => Some("側効果"),
            0x7F0B => Some("側面色"),
            0x7F0C => Some("側面灯"),
            0x7F0D => Some("低遅延"),
            0x7F0E => Some("全灯切"),
            0x7F0F => Some("電池"),
            _ => None,
        },
    }
}

fn basic_label_lang(kc: u16, lang: crate::i18n::Lang) -> String {
    if let Some(l) = localized_label(kc, lang) {
        return l.to_string();
    }
    for &(c, _, l) in BASIC_KEYCODES {
        if c == kc {
            return l.to_string();
        }
    }
    format!("0x{:02X}", kc)
}

fn format_mods(mods5: u8) -> String {
    let mut parts = Vec::new();
    if mods5 & 0x01 != 0 { parts.push("C"); }
    if mods5 & 0x02 != 0 { parts.push("S"); }
    if mods5 & 0x04 != 0 { parts.push("A"); }
    if mods5 & 0x08 != 0 { parts.push("G"); }
    let joined = if parts.is_empty() { "M".to_string() } else { parts.join("+") };
    if mods5 & 0x10 != 0 { format!("R{}", joined) } else { joined }
}

fn localized_norgb_wob_label(code: u16, lang: crate::i18n::Lang) -> Option<&'static str> {
    use crate::i18n::Lang;
    match lang {
        Lang::Zh => None,
        Lang::En => match code {
            0x7F07 => Some("LoLat"),
            0x7F08 => Some("LogoM"),
            0x7F09 => Some("LogoC"),
            0x7F0A => Some("Logo"),
            0x7F0B => Some("AllLED"),
            0x7F0C => Some("Batt"),
            _ => None,
        },
        Lang::Ja => match code {
            0x7F07 => Some("低遅延"),
            0x7F08 => Some("ロゴ効"),
            0x7F09 => Some("ロゴ色"),
            0x7F0A => Some("ロゴ灯"),
            0x7F0B => Some("全灯切"),
            0x7F0C => Some("電池"),
            _ => None,
        },
    }
}

pub fn decode_keycode(code: u16) -> DecodedKey {
    decode_keycode_for(code, crate::i18n::Lang::Zh, BoardEdition::ProRgb)
}

pub fn decode_keycode_lang(code: u16, lang: crate::i18n::Lang) -> DecodedKey {
    decode_keycode_for(code, lang, BoardEdition::ProRgb)
}

pub fn decode_keycode_for(code: u16, lang: crate::i18n::Lang, edition: BoardEdition) -> DecodedKey {
    for &(c, n, l) in BASIC_KEYCODES {
        if c == code {
            let lbl = localized_label(code, lang).unwrap_or(l);
            return DecodedKey { code, name: n.into(), label: lbl.into(), category: KeyCategory::Basic };
        }
    }
    for &(c, n, l) in special_keycodes_for(edition) {
        if c == code {
            let lbl = if edition == BoardEdition::LiteNoRgb {
                localized_norgb_wob_label(code, lang)
                    .or_else(|| if (0x7F07..=0x7F0F).contains(&code) { None } else { localized_label(code, lang) })
                    .unwrap_or(l)
            } else {
                localized_label(code, lang).unwrap_or(l)
            };
            return DecodedKey { code, name: n.into(), label: lbl.into(), category: KeyCategory::Special };
        }
    }
    if (0x7700..=0x770F).contains(&code) {
        let i = code - 0x7700;
        return DecodedKey { code, name: format!("MACRO({})", i), label: format!("M{}", i), category: KeyCategory::Macro };
    }
    if (0x0100..=0x1FFF).contains(&code) {
        let m = format_mods(((code >> 8) & 0x1F) as u8);
        let k = basic_label_lang(code & 0xFF, lang);
        return DecodedKey { code, name: format!("MOD({},{})", m, k), label: format!("{}+{}", m, k), category: KeyCategory::Mods };
    }
    if (0x2000..=0x3FFF).contains(&code) {
        let m = format_mods(((code >> 8) & 0x1F) as u8);
        let k = basic_label_lang(code & 0xFF, lang);
        return DecodedKey { code, name: format!("MT({},{})", m, k), label: format!("{}/{}", m, k), category: KeyCategory::TapHold };
    }
    if (0x4000..=0x4FFF).contains(&code) {
        let l = (code >> 8) & 0x0F;
        let k = basic_label_lang(code & 0xFF, lang);
        return DecodedKey { code, name: format!("LT({},{})", l, k), label: format!("L{}/{}", l, k), category: KeyCategory::TapHold };
    }
    if (0x5200..=0x521F).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("TO({})", l), label: format!("TO({})", l), category: KeyCategory::Layer };
    }
    if (0x5220..=0x523F).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("MO({})", l), label: format!("Fn{}", l), category: KeyCategory::Layer };
    }
    if (0x5240..=0x525F).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("DF({})", l), label: format!("DF({})", l), category: KeyCategory::Layer };
    }
    if (0x5260..=0x527F).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("TG({})", l), label: format!("TG({})", l), category: KeyCategory::Layer };
    }
    if (0x5280..=0x529F).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("OSL({})", l), label: format!("OSL{}", l), category: KeyCategory::Layer };
    }
    if (0x52A0..=0x52BF).contains(&code) {
        let m = format_mods((code & 0x1F) as u8);
        return DecodedKey { code, name: format!("OSM({})", m), label: format!("OS:{}", m), category: KeyCategory::TapHold };
    }
    if (0x52C0..=0x52DF).contains(&code) {
        let l = code & 0x1F;
        return DecodedKey { code, name: format!("TT({})", l), label: format!("TT({})", l), category: KeyCategory::Layer };
    }
    DecodedKey { code, name: format!("0x{:04X}", code), label: format!("0x{:04X}", code), category: KeyCategory::Raw }
}

/// Exact 4-layer factory default keymap (from backup_stock.json `keymap_hex`).
pub const FACTORY_KEYMAP: [[u16; 128]; 4] = [
    // Layer 0
    [
        0x0029, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048,
        0x0035, 0x001E, 0x001F, 0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027, 0x002D, 0x002E, 0x002A, 0x0049, 0x004A,
        0x002B, 0x0014, 0x001A, 0x0008, 0x0015, 0x0017, 0x001C, 0x0018, 0x000C, 0x0012, 0x0013, 0x002F, 0x0030, 0x0031, 0x004C, 0x004D,
        0x0039, 0x0004, 0x0016, 0x0007, 0x0009, 0x000A, 0x000B, 0x000D, 0x000E, 0x000F, 0x0033, 0x0034, 0x0032, 0x0028, 0x004B, 0x004E,
        0x00E1, 0x0064, 0x001D, 0x001B, 0x0006, 0x0019, 0x0005, 0x0011, 0x0010, 0x0036, 0x0037, 0x0038, 0x00A8, 0x00E5, 0x0052, 0x0035,
        0x00E0, 0x00E3, 0x00E2, 0x0000, 0x002C, 0x002C, 0x0000, 0x0000, 0x002C, 0x00E6, 0x5221, 0x0065, 0x00E4, 0x0050, 0x0051, 0x004F,
        0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x00AA, 0x00A9, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    ],
    // Layer 1
    [
        0x0029, 0x00BE, 0x00BD, 0x00B5, 0x00B1, 0x00B2, 0x00AF, 0x00AC, 0x00AE, 0x00AB, 0x00A8, 0x00AA, 0x00A9, 0x0046, 0x0047, 0x0048,
        0x0035, 0x7F01, 0x7F02, 0x7F03, 0x7F00, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027, 0x002D, 0x002E, 0x7820, 0x0049, 0x004A,
        0x7F04, 0x0014, 0x001A, 0x0008, 0x0015, 0x0017, 0x001C, 0x0018, 0x000C, 0x0012, 0x7F09, 0x7F07, 0x7F08, 0x7821, 0x004C, 0x004D,
        0x0039, 0x0004, 0x0016, 0x0007, 0x0009, 0x000A, 0x7F0D, 0x000D, 0x000E, 0x7F0E, 0x7F0A, 0x7F0B, 0x0032, 0x7823, 0x004B, 0x004E,
        0x00E1, 0x0064, 0x001D, 0x001B, 0x0006, 0x0019, 0x0005, 0x0011, 0x7F06, 0x7826, 0x7825, 0x7F0C, 0x00A8, 0x00E5, 0x7827, 0x0035,
        0x00E0, 0x7F05, 0x00E2, 0x0000, 0x7F0F, 0x7F0F, 0x0000, 0x0000, 0x7F0F, 0x00E6, 0x5221, 0x0065, 0x00E4, 0x782A, 0x7828, 0x7829,
        0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x00AA, 0x00A9, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    ],
    // Layer 2
    [
        0x0029, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048,
        0x0035, 0x001E, 0x001F, 0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027, 0x002D, 0x002E, 0x002A, 0x0049, 0x004A,
        0x002B, 0x0014, 0x001A, 0x0008, 0x0015, 0x0017, 0x001C, 0x0018, 0x000C, 0x0012, 0x0013, 0x002F, 0x0030, 0x0031, 0x004C, 0x004D,
        0x0039, 0x0004, 0x0016, 0x0007, 0x0009, 0x000A, 0x000B, 0x000D, 0x000E, 0x000F, 0x0033, 0x0034, 0x0032, 0x0028, 0x004B, 0x004E,
        0x00E1, 0x0064, 0x001D, 0x001B, 0x0006, 0x0019, 0x0005, 0x0011, 0x0010, 0x0036, 0x0037, 0x0038, 0x00A8, 0x00E5, 0x0052, 0x0035,
        0x00E0, 0x00E3, 0x00E2, 0x0000, 0x002C, 0x002C, 0x0000, 0x0000, 0x002C, 0x00E6, 0x5221, 0x0065, 0x00E4, 0x0050, 0x0051, 0x004F,
        0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x00AA, 0x00A9, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    ],
    // Layer 3
    [
        0x0029, 0x003A, 0x003B, 0x003C, 0x003D, 0x003E, 0x003F, 0x0040, 0x0041, 0x0042, 0x0043, 0x0044, 0x0045, 0x0046, 0x0047, 0x0048,
        0x0035, 0x001E, 0x001F, 0x0020, 0x0021, 0x0022, 0x0023, 0x0024, 0x0025, 0x0026, 0x0027, 0x002D, 0x002E, 0x002A, 0x0049, 0x004A,
        0x002B, 0x0014, 0x001A, 0x0008, 0x0015, 0x0017, 0x001C, 0x0018, 0x000C, 0x0012, 0x0013, 0x002F, 0x0030, 0x0031, 0x004C, 0x004D,
        0x0039, 0x0004, 0x0016, 0x0007, 0x0009, 0x000A, 0x000B, 0x000D, 0x000E, 0x000F, 0x0033, 0x0034, 0x0032, 0x0028, 0x004B, 0x004E,
        0x00E1, 0x0064, 0x001D, 0x001B, 0x0006, 0x0019, 0x0005, 0x0011, 0x0010, 0x0036, 0x0037, 0x0038, 0x00A8, 0x00E5, 0x0052, 0x0035,
        0x00E0, 0x00E3, 0x00E2, 0x0000, 0x002C, 0x002C, 0x0000, 0x0000, 0x002C, 0x00E6, 0x5221, 0x0065, 0x00E4, 0x0050, 0x0051, 0x004F,
        0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
        0x00AA, 0x00A9, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000, 0x0000,
    ],
];

pub fn factory_default_key(layer: usize, row: u8, col: u8) -> u16 {
    factory_default_key_for(BoardEdition::ProRgb, layer, row, col)
}

pub fn factory_default_key_for(edition: BoardEdition, layer: usize, row: u8, col: u8) -> u16 {
    if layer >= 4 || row >= 8 || col >= 16 {
        return 0;
    }
    if edition == BoardEdition::LiteNoRgb {
        // Account for the 4 swapped matrix coordinates in official Crush 80 Non-RGB PCB:
        // (4,12)=RShift, (4,13)=F13(Mute), (5,6)=Space/Batt, (5,8)=Menu(RWin), (5,11)=unused
        let (src_r, src_c) = match (row, col) {
            (4, 12) => (4, 13),
            (4, 13) => (4, 12),
            (5, 6) => (5, 5),
            (5, 8) => (5, 11),
            (5, 11) => return 0x0000,
            other => other,
        };
        let kc = FACTORY_KEYMAP[layer][(src_r as usize) * 16 + (src_c as usize)];
        // Map Pro RGB custom codes (0x7F07..0x7F0F) to official Non-RGB codes (0x7F07..0x7F0C)
        return match kc {
            0x7F07 => 0x7F08, // LOMODE
            0x7F08 => 0x7F09, // LOCOLOUR
            0x7F09 => 0x7F0A, // LOPOWER
            0x7F0A | 0x7F0B | 0x7F0C => 0x0001, // Side RGB absent on Non-RGB -> KC_TRNS
            0x7F0D => 0x7F07, // KSDELAY
            0x7F0E => 0x7F0B, // SYSPOWER
            0x7F0F => 0x7F0C, // BATTST
            other => other,
        };
    }
    let idx = (row as usize) * 16 + (col as usize);
    FACTORY_KEYMAP[layer][idx]
}

/// Auto-detects Crush 80 Pro (RGB) vs Lite (Non-RGB) from the live Layer 0 keymap matrix.
/// On Pro RGB: (4,13)=KC_RSFT(0x00E5), (5,5)=KC_SPC(0x002C), (5,6)=0x0000.
/// On Lite Non-RGB: (4,12)=KC_RSFT(0x00E5), (5,6)=KC_SPC(0x002C).
pub fn detect_edition_from_keymap(layers: &[[u16; 128]; 4]) -> Option<BoardEdition> {
    let l0 = &layers[0];
    let r4c12 = l0[4 * 16 + 12];
    let r4c13 = l0[4 * 16 + 13];
    let r5c5 = l0[5 * 16 + 5];
    let r5c6 = l0[5 * 16 + 6];
    let l1 = &layers[1];

    let mut norgb_score = 0i32;
    let mut rgb_score = 0i32;

    if r4c12 == 0x00E5 { norgb_score += 3; }
    if r4c13 == 0x00E5 { rgb_score += 3; }
    if r5c6 == 0x002C { norgb_score += 3; }
    if r5c5 == 0x002C && r5c6 == 0x0000 { rgb_score += 3; }
    // On Pro RGB L1, 0x7F0D/0x7F0E/0x7F0F exist; on Non-RGB, max custom code is 0x7F0C
    if l1.iter().any(|&c| (0x7F0D..=0x7F0F).contains(&c)) {
        rgb_score += 2;
    }

    if norgb_score > rgb_score && norgb_score >= 3 {
        Some(BoardEdition::LiteNoRgb)
    } else if rgb_score > norgb_score && rgb_score >= 3 {
        Some(BoardEdition::ProRgb)
    } else {
        None
    }
}

pub fn f13_matrix(edition: BoardEdition) -> (u8, u8) {
    match edition {
        BoardEdition::ProRgb => (4, 12),
        BoardEdition::LiteNoRgb => (4, 13),
    }
}

pub fn layout_for(edition: BoardEdition) -> &'static [PhysicalKey] {
    match edition {
        BoardEdition::ProRgb => CRUSH80_LAYOUT,
        BoardEdition::LiteNoRgb => CRUSH80_LAYOUT_NORGB,
    }
}

pub const CRUSH80_LAYOUT: &[PhysicalKey] = &[
    // Row 0 (F13 88-key TKL: 0.25u gaps, F13 AnyKey @ Matrix 4,12 = x:14.0)
    PhysicalKey { r: 0, c: 0, x: 0.0, y: 0.0, w: 1.0, style: 2 },
    PhysicalKey { r: 0, c: 1, x: 1.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 2, x: 2.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 3, x: 3.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 4, x: 4.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 5, x: 5.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 6, x: 6.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 7, x: 7.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 8, x: 8.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 9, x: 9.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 10, x: 10.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 11, x: 11.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 12, x: 12.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 4, c: 12, x: 14.0, y: 0.0, w: 1.0, style: 2 }, // F13 AnyKey (default Mute 0x00A8)
    PhysicalKey { r: 0, c: 13, x: 15.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 14, x: 16.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 15, x: 17.25, y: 0.0, w: 1.0, style: 1 },
    // Row 1
    PhysicalKey { r: 1, c: 0, x: 0.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 1, x: 1.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 2, x: 2.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 3, x: 3.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 4, x: 4.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 5, x: 5.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 6, x: 6.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 7, x: 7.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 8, x: 8.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 9, x: 9.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 10, x: 10.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 11, x: 11.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 12, x: 12.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 13, x: 13.0, y: 1.25, w: 2.0, style: 1 },
    PhysicalKey { r: 1, c: 14, x: 15.25, y: 1.25, w: 1.0, style: 1 },
    PhysicalKey { r: 1, c: 15, x: 16.25, y: 1.25, w: 1.0, style: 1 },
    PhysicalKey { r: 3, c: 14, x: 17.25, y: 1.25, w: 1.0, style: 1 },
    // Row 2
    PhysicalKey { r: 2, c: 0, x: 0.0, y: 2.25, w: 1.5, style: 1 },
    PhysicalKey { r: 2, c: 1, x: 1.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 2, x: 2.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 3, x: 3.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 4, x: 4.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 5, x: 5.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 6, x: 6.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 7, x: 7.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 8, x: 8.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 9, x: 9.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 10, x: 10.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 11, x: 11.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 12, x: 12.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 13, x: 13.5, y: 2.25, w: 1.5, style: 1 },
    PhysicalKey { r: 2, c: 14, x: 15.25, y: 2.25, w: 1.0, style: 1 },
    PhysicalKey { r: 2, c: 15, x: 16.25, y: 2.25, w: 1.0, style: 1 },
    PhysicalKey { r: 3, c: 15, x: 17.25, y: 2.25, w: 1.0, style: 1 },
    // Row 3
    PhysicalKey { r: 3, c: 0, x: 0.0, y: 3.25, w: 1.75, style: 1 },
    PhysicalKey { r: 3, c: 1, x: 1.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 2, x: 2.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 3, x: 3.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 4, x: 4.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 5, x: 5.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 6, x: 6.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 7, x: 7.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 8, x: 8.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 9, x: 9.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 10, x: 10.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 11, x: 11.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 13, x: 12.75, y: 3.25, w: 2.25, style: 2 },
    // Row 4
    PhysicalKey { r: 4, c: 0, x: 0.0, y: 4.25, w: 2.25, style: 1 },
    PhysicalKey { r: 4, c: 2, x: 2.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 3, x: 3.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 4, x: 4.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 5, x: 5.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 6, x: 6.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 7, x: 7.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 8, x: 8.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 9, x: 9.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 10, x: 10.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 11, x: 11.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 13, x: 12.25, y: 4.25, w: 2.75, style: 1 },
    PhysicalKey { r: 4, c: 14, x: 16.25, y: 4.25, w: 1.0, style: 1 },
    // Row 5
    PhysicalKey { r: 5, c: 0, x: 0.0, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 1, x: 1.25, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 2, x: 2.5, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 5, x: 3.75, y: 5.25, w: 6.25, style: 3 },
    PhysicalKey { r: 5, c: 9, x: 10.0, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 10, x: 11.25, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 11, x: 12.5, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 12, x: 13.75, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 13, x: 15.25, y: 5.25, w: 1.0, style: 1 },
    PhysicalKey { r: 5, c: 14, x: 16.25, y: 5.25, w: 1.0, style: 1 },
    PhysicalKey { r: 5, c: 15, x: 17.25, y: 5.25, w: 1.0, style: 1 },
];

/// Exact 88-key F13 TKL layout for official Crush 80 Non-RGB (Lite) PCB:
/// F13=(4,13), RShift=(4,12), Space=(5,6), Menu/RWin=(5,8).
pub const CRUSH80_LAYOUT_NORGB: &[PhysicalKey] = &[
    // Row 0
    PhysicalKey { r: 0, c: 0, x: 0.0, y: 0.0, w: 1.0, style: 2 },
    PhysicalKey { r: 0, c: 1, x: 1.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 2, x: 2.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 3, x: 3.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 4, x: 4.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 5, x: 5.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 6, x: 6.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 7, x: 7.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 8, x: 8.5, y: 0.0, w: 1.0, style: 0 },
    PhysicalKey { r: 0, c: 9, x: 9.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 10, x: 10.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 11, x: 11.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 12, x: 12.75, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 4, c: 13, x: 14.0, y: 0.0, w: 1.0, style: 2 }, // F13 AnyKey @ Matrix 4,13
    PhysicalKey { r: 0, c: 13, x: 15.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 14, x: 16.25, y: 0.0, w: 1.0, style: 1 },
    PhysicalKey { r: 0, c: 15, x: 17.25, y: 0.0, w: 1.0, style: 1 },
    // Row 1
    PhysicalKey { r: 1, c: 0, x: 0.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 1, x: 1.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 2, x: 2.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 3, x: 3.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 4, x: 4.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 5, x: 5.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 6, x: 6.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 7, x: 7.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 8, x: 8.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 9, x: 9.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 10, x: 10.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 11, x: 11.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 12, x: 12.0, y: 1.25, w: 1.0, style: 0 },
    PhysicalKey { r: 1, c: 13, x: 13.0, y: 1.25, w: 2.0, style: 1 },
    PhysicalKey { r: 1, c: 14, x: 15.25, y: 1.25, w: 1.0, style: 1 },
    PhysicalKey { r: 1, c: 15, x: 16.25, y: 1.25, w: 1.0, style: 1 },
    PhysicalKey { r: 3, c: 14, x: 17.25, y: 1.25, w: 1.0, style: 1 },
    // Row 2
    PhysicalKey { r: 2, c: 0, x: 0.0, y: 2.25, w: 1.5, style: 1 },
    PhysicalKey { r: 2, c: 1, x: 1.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 2, x: 2.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 3, x: 3.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 4, x: 4.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 5, x: 5.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 6, x: 6.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 7, x: 7.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 8, x: 8.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 9, x: 9.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 10, x: 10.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 11, x: 11.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 12, x: 12.5, y: 2.25, w: 1.0, style: 0 },
    PhysicalKey { r: 2, c: 13, x: 13.5, y: 2.25, w: 1.5, style: 1 },
    PhysicalKey { r: 2, c: 14, x: 15.25, y: 2.25, w: 1.0, style: 1 },
    PhysicalKey { r: 2, c: 15, x: 16.25, y: 2.25, w: 1.0, style: 1 },
    PhysicalKey { r: 3, c: 15, x: 17.25, y: 2.25, w: 1.0, style: 1 },
    // Row 3
    PhysicalKey { r: 3, c: 0, x: 0.0, y: 3.25, w: 1.75, style: 1 },
    PhysicalKey { r: 3, c: 1, x: 1.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 2, x: 2.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 3, x: 3.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 4, x: 4.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 5, x: 5.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 6, x: 6.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 7, x: 7.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 8, x: 8.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 9, x: 9.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 10, x: 10.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 11, x: 11.75, y: 3.25, w: 1.0, style: 0 },
    PhysicalKey { r: 3, c: 13, x: 12.75, y: 3.25, w: 2.25, style: 2 },
    // Row 4
    PhysicalKey { r: 4, c: 0, x: 0.0, y: 4.25, w: 2.25, style: 1 },
    PhysicalKey { r: 4, c: 2, x: 2.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 3, x: 3.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 4, x: 4.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 5, x: 5.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 6, x: 6.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 7, x: 7.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 8, x: 8.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 9, x: 9.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 10, x: 10.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 11, x: 11.25, y: 4.25, w: 1.0, style: 0 },
    PhysicalKey { r: 4, c: 12, x: 12.25, y: 4.25, w: 2.75, style: 1 }, // RShift @ Matrix 4,12
    PhysicalKey { r: 4, c: 14, x: 16.25, y: 4.25, w: 1.0, style: 1 },
    // Row 5
    PhysicalKey { r: 5, c: 0, x: 0.0, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 1, x: 1.25, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 2, x: 2.5, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 6, x: 3.75, y: 5.25, w: 6.25, style: 3 }, // Space @ Matrix 5,6
    PhysicalKey { r: 5, c: 9, x: 10.0, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 10, x: 11.25, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 8, x: 12.5, y: 5.25, w: 1.25, style: 1 }, // Menu/RWin @ Matrix 5,8
    PhysicalKey { r: 5, c: 12, x: 13.75, y: 5.25, w: 1.25, style: 1 },
    PhysicalKey { r: 5, c: 13, x: 15.25, y: 5.25, w: 1.0, style: 1 },
    PhysicalKey { r: 5, c: 14, x: 16.25, y: 5.25, w: 1.0, style: 1 },
    PhysicalKey { r: 5, c: 15, x: 17.25, y: 5.25, w: 1.0, style: 1 },
];
