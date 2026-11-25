use crate::i18n::Lang;
use eframe::egui::Key;

pub const MACRO_BUFFER_SIZE: usize = 512;
pub const MACRO_SLOT_COUNT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MacroStep {
    Text(String),
    Tap(u8),    // 0x01, 0x01, kc
    Down(u8),   // 0x01, 0x02, kc
    Up(u8),     // 0x01, 0x03, kc
    Delay(u16), // 0x01, 0x04, <ascii digits>, '|' (or 0x00 at slot end)
}

fn flush_text(steps: &mut Vec<MacroStep>, text_buf: &mut String) {
    if !text_buf.is_empty() {
        steps.push(MacroStep::Text(std::mem::take(text_buf)));
    }
}

/// Decode a 512-byte VIA/NativeHeld macro buffer into 16 slots of `MacroStep`.
///
/// Format per `macro_stream.c`:
/// - 16 slots separated/terminated by `0x00`.
/// - `0x01, 0x01, kc` => `Tap(kc)`
/// - `0x01, 0x02, kc` => `Down(kc)`
/// - `0x01, 0x03, kc` => `Up(kc)`
/// - `0x01, 0x04, '0'..'9'+, '|'` (or `0x00` which also ends the slot) => `Delay(ms)`
/// - Other ASCII bytes `0x02..=0x7F` => `Text(String)`
pub fn decode_macro_buffer(buf: &[u8]) -> Vec<Vec<MacroStep>> {
    let limit = buf.len().min(MACRO_BUFFER_SIZE);
    let mut slots: Vec<Vec<MacroStep>> = Vec::with_capacity(MACRO_SLOT_COUNT);
    let mut steps: Vec<MacroStep> = Vec::new();
    let mut text_buf = String::new();
    let mut i = 0usize;

    while i < limit && slots.len() < MACRO_SLOT_COUNT {
        let b = buf[i];
        i += 1;

        if b == 0x00 {
            flush_text(&mut steps, &mut text_buf);
            slots.push(std::mem::take(&mut steps));
            continue;
        }

        if b == 0x01 {
            flush_text(&mut steps, &mut text_buf);
            if i >= limit {
                break;
            }
            let op = buf[i];
            i += 1;
            match op {
                0x01..=0x03 => {
                    if i >= limit {
                        break;
                    }
                    let kc = buf[i];
                    i += 1;
                    match op {
                        0x01 => steps.push(MacroStep::Tap(kc)),
                        0x02 => steps.push(MacroStep::Down(kc)),
                        0x03 => steps.push(MacroStep::Up(kc)),
                        _ => unreachable!(),
                    }
                }
                0x04 => {
                    let mut val: u32 = 0;
                    let mut has_digit = false;
                    let mut ended_slot = false;
                    while i < limit {
                        let d = buf[i];
                        if d.is_ascii_digit() {
                            i += 1;
                            val = val.saturating_mul(10).saturating_add((d - b'0') as u32);
                            has_digit = true;
                        } else if d == b'|' {
                            i += 1;
                            break;
                        } else if d == 0x00 {
                            i += 1;
                            ended_slot = true;
                            break;
                        } else {
                            break;
                        }
                    }
                    if has_digit {
                        steps.push(MacroStep::Delay(val.min(u16::MAX as u32) as u16));
                    }
                    if ended_slot {
                        slots.push(std::mem::take(&mut steps));
                    }
                }
                _ => {
                    // Unsupported prefix opcode; skip gracefully.
                }
            }
        } else if b <= 0x7F {
            text_buf.push(b as char);
        }
    }

    if slots.len() < MACRO_SLOT_COUNT {
        flush_text(&mut steps, &mut text_buf);
        if !steps.is_empty() {
            slots.push(steps);
        }
    }
    while slots.len() < MACRO_SLOT_COUNT {
        slots.push(Vec::new());
    }
    slots
}

/// Validate macro text input before inserting or encoding:
/// firmware macro stream only supports printable ASCII (`0x20..=0x7E`).
pub fn validate_macro_text(s: &str) -> Result<(), String> {
    for &b in s.as_bytes() {
        if !(0x20..=0x7E).contains(&b) {
            return Err(format!(
                "宏文本仅支持可打印 ASCII 字符 (0x20..=0x7E)，检测到字节 0x{:02X}",
                b
            ));
        }
    }
    Ok(())
}

/// Encode up to 16 macro slots into an exact 512-byte VIA/NativeHeld macro buffer.
pub fn encode_macro_buffer(slots: &[Vec<MacroStep>]) -> Result<Vec<u8>, String> {
    if slots.len() > MACRO_SLOT_COUNT {
        return Err(format!(
            "Too many macro slots: {} (max {})",
            slots.len(),
            MACRO_SLOT_COUNT
        ));
    }

    let mut out = Vec::with_capacity(MACRO_BUFFER_SIZE);

    for idx in 0..MACRO_SLOT_COUNT {
        if let Some(slot) = slots.get(idx) {
            for step in slot {
                match step {
                    MacroStep::Text(s) => {
                        validate_macro_text(s)
                            .map_err(|e| format!("Macro slot {}: {}", idx, e))?;
                        out.extend_from_slice(s.as_bytes());
                    }
                    MacroStep::Tap(kc) => {
                        if *kc == 0x00 {
                            return Err(format!("Macro slot {} contains invalid keycode 0x00", idx));
                        }
                        out.push(0x01);
                        out.push(0x01);
                        out.push(*kc);
                    }
                    MacroStep::Down(kc) => {
                        if *kc == 0x00 {
                            return Err(format!("Macro slot {} contains invalid keycode 0x00", idx));
                        }
                        out.push(0x01);
                        out.push(0x02);
                        out.push(*kc);
                    }
                    MacroStep::Up(kc) => {
                        if *kc == 0x00 {
                            return Err(format!("Macro slot {} contains invalid keycode 0x00", idx));
                        }
                        out.push(0x01);
                        out.push(0x03);
                        out.push(*kc);
                    }
                    MacroStep::Delay(ms) => {
                        out.push(0x01);
                        out.push(0x04);
                        out.extend_from_slice(ms.to_string().as_bytes());
                        out.push(b'|');
                    }
                }
            }
        }
        out.push(0x00);
    }

    if out.len() > MACRO_BUFFER_SIZE {
        return Err(format!(
            "Macro buffer overflow: {} / {} bytes",
            out.len(),
            MACRO_BUFFER_SIZE
        ));
    }

    out.resize(MACRO_BUFFER_SIZE, 0x00);
    Ok(out)
}

/// Compute the encoded byte count (including all 16 null terminators) for UI usage display.
pub fn encoded_byte_len(slots: &[Vec<MacroStep>]) -> usize {
    let mut total = MACRO_SLOT_COUNT; // 16 null terminators
    for slot in slots.iter().take(MACRO_SLOT_COUNT) {
        for step in slot {
            total += match step {
                MacroStep::Text(s) => s.len(),
                MacroStep::Tap(_) | MacroStep::Down(_) | MacroStep::Up(_) => 3,
                MacroStep::Delay(ms) => {
                    let digits = if *ms >= 10000 {
                        5
                    } else if *ms >= 1000 {
                        4
                    } else if *ms >= 100 {
                        3
                    } else if *ms >= 10 {
                        2
                    } else {
                        1
                    };
                    2 + digits + 1 // 0x01, 0x04, digits, '|'
                }
            };
        }
    }
    total
}

/// Format a single `MacroStep` into a concise localized UI label.
pub fn format_step(step: &MacroStep, lang: Lang) -> String {
    match step {
        MacroStep::Text(s) => format!("\"{}\"", s),
        MacroStep::Tap(kc) => match lang {
            Lang::Zh => format!("轻按 {}", hid_to_name(*kc)),
            Lang::En => format!("Tap {}", hid_to_name(*kc)),
            Lang::Ja => format!("タップ {}", hid_to_name(*kc)),
        },
        MacroStep::Down(kc) => match lang {
            Lang::Zh => format!("按下 {}", hid_to_name(*kc)),
            Lang::En => format!("Down {}", hid_to_name(*kc)),
            Lang::Ja => format!("押下 {}", hid_to_name(*kc)),
        },
        MacroStep::Up(kc) => match lang {
            Lang::Zh => format!("松开 {}", hid_to_name(*kc)),
            Lang::En => format!("Up {}", hid_to_name(*kc)),
            Lang::Ja => format!("離す {}", hid_to_name(*kc)),
        },
        MacroStep::Delay(ms) => match lang {
            Lang::Zh => format!("延时 {}ms", ms),
            Lang::En => format!("Delay {}ms", ms),
            Lang::Ja => format!("待機 {}ms", ms),
        },
    }
}

/// Map an `egui::Key` to its standard USB HID keyboard usage code.
pub fn egui_key_to_hid(key: Key) -> Option<u8> {
    Some(match key {
        Key::A => 0x04,
        Key::B => 0x05,
        Key::C => 0x06,
        Key::D => 0x07,
        Key::E => 0x08,
        Key::F => 0x09,
        Key::G => 0x0A,
        Key::H => 0x0B,
        Key::I => 0x0C,
        Key::J => 0x0D,
        Key::K => 0x0E,
        Key::L => 0x0F,
        Key::M => 0x10,
        Key::N => 0x11,
        Key::O => 0x12,
        Key::P => 0x13,
        Key::Q => 0x14,
        Key::R => 0x15,
        Key::S => 0x16,
        Key::T => 0x17,
        Key::U => 0x18,
        Key::V => 0x19,
        Key::W => 0x1A,
        Key::X => 0x1B,
        Key::Y => 0x1C,
        Key::Z => 0x1D,
        Key::Num1 => 0x1E,
        Key::Num2 => 0x1F,
        Key::Num3 => 0x20,
        Key::Num4 => 0x21,
        Key::Num5 => 0x22,
        Key::Num6 => 0x23,
        Key::Num7 => 0x24,
        Key::Num8 => 0x25,
        Key::Num9 => 0x26,
        Key::Num0 => 0x27,
        Key::Enter => 0x28,
        Key::Escape => 0x29,
        Key::Backspace => 0x2A,
        Key::Tab => 0x2B,
        Key::Space => 0x2C,
        Key::Minus => 0x2D,
        Key::Equals | Key::Plus => 0x2E,
        Key::OpenBracket => 0x2F,
        Key::CloseBracket => 0x30,
        Key::Backslash | Key::Pipe => 0x31,
        Key::Semicolon | Key::Colon => 0x33,
        Key::Quote => 0x34,
        Key::Backtick => 0x35,
        Key::Comma => 0x36,
        Key::Period => 0x37,
        Key::Slash | Key::Questionmark => 0x38,
        Key::F1 => 0x3A,
        Key::F2 => 0x3B,
        Key::F3 => 0x3C,
        Key::F4 => 0x3D,
        Key::F5 => 0x3E,
        Key::F6 => 0x3F,
        Key::F7 => 0x40,
        Key::F8 => 0x41,
        Key::F9 => 0x42,
        Key::F10 => 0x43,
        Key::F11 => 0x44,
        Key::F12 => 0x45,
        Key::Insert => 0x49,
        Key::Home => 0x4A,
        Key::PageUp => 0x4B,
        Key::Delete => 0x4C,
        Key::End => 0x4D,
        Key::PageDown => 0x4E,
        Key::ArrowRight => 0x4F,
        Key::ArrowLeft => 0x50,
        Key::ArrowDown => 0x51,
        Key::ArrowUp => 0x52,
        Key::F13 => 0x68,
        Key::F14 => 0x69,
        Key::F15 => 0x6A,
        Key::F16 => 0x6B,
        Key::F17 => 0x6C,
        Key::F18 => 0x6D,
        Key::F19 => 0x6E,
        Key::F20 => 0x6F,
        Key::F21 => 0x70,
        Key::F22 => 0x71,
        Key::F23 => 0x72,
        Key::F24 => 0x73,
        _ => return None,
    })
}

/// Convert a USB HID keyboard usage code to a concise human-readable key name.
pub fn hid_to_name(kc: u8) -> &'static str {
    match kc {
        0x04 => "A",
        0x05 => "B",
        0x06 => "C",
        0x07 => "D",
        0x08 => "E",
        0x09 => "F",
        0x0A => "G",
        0x0B => "H",
        0x0C => "I",
        0x0D => "J",
        0x0E => "K",
        0x0F => "L",
        0x10 => "M",
        0x11 => "N",
        0x12 => "O",
        0x13 => "P",
        0x14 => "Q",
        0x15 => "R",
        0x16 => "S",
        0x17 => "T",
        0x18 => "U",
        0x19 => "V",
        0x1A => "W",
        0x1B => "X",
        0x1C => "Y",
        0x1D => "Z",
        0x1E => "1",
        0x1F => "2",
        0x20 => "3",
        0x21 => "4",
        0x22 => "5",
        0x23 => "6",
        0x24 => "7",
        0x25 => "8",
        0x26 => "9",
        0x27 => "0",
        0x28 => "Enter",
        0x29 => "Esc",
        0x2A => "Bksp",
        0x2B => "Tab",
        0x2C => "Space",
        0x2D => "-",
        0x2E => "=",
        0x2F => "[",
        0x30 => "]",
        0x31 => "\\",
        0x32 => "#",
        0x33 => ";",
        0x34 => "'",
        0x35 => "`",
        0x36 => ",",
        0x37 => ".",
        0x38 => "/",
        0x39 => "Caps",
        0x3A => "F1",
        0x3B => "F2",
        0x3C => "F3",
        0x3D => "F4",
        0x3E => "F5",
        0x3F => "F6",
        0x40 => "F7",
        0x41 => "F8",
        0x42 => "F9",
        0x43 => "F10",
        0x44 => "F11",
        0x45 => "F12",
        0x46 => "PrtSc",
        0x47 => "ScrLk",
        0x48 => "Pause",
        0x49 => "Ins",
        0x4A => "Home",
        0x4B => "PgUp",
        0x4C => "Del",
        0x4D => "End",
        0x4E => "PgDn",
        0x4F => "Right",
        0x50 => "Left",
        0x51 => "Down",
        0x52 => "Up",
        0x64 => "\\",
        0x65 => "Menu",
        0x68 => "F13",
        0x69 => "F14",
        0x6A => "F15",
        0x6B => "F16",
        0x6C => "F17",
        0x6D => "F18",
        0x6E => "F19",
        0x6F => "F20",
        0x70 => "F21",
        0x71 => "F22",
        0x72 => "F23",
        0x73 => "F24",
        0xA8 => "Mute",
        0xA9 => "Vol+",
        0xAA => "Vol-",
        0xE0 => "LCtrl",
        0xE1 => "LShift",
        0xE2 => "LAlt",
        0xE3 => "LWin",
        0xE4 => "RCtrl",
        0xE5 => "RShift",
        0xE6 => "RAlt",
        0xE7 => "RWin",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_macro_steps() {
        let mut slots: Vec<Vec<MacroStep>> = vec![Vec::new(); 16];
        slots[0] = vec![
            MacroStep::Down(0xE0),
            MacroStep::Tap(0x06),
            MacroStep::Up(0xE0),
            MacroStep::Delay(150),
            MacroStep::Text("hello!".to_string()),
        ];
        slots[3] = vec![MacroStep::Delay(50)];

        let encoded = encode_macro_buffer(&slots).expect("encode");
        assert_eq!(encoded.len(), MACRO_BUFFER_SIZE);
        let expected = b"\x01\x02\xe0\x01\x01\x06\x01\x03\xe0\x01\x04150|hello!";
        assert_eq!(&encoded[0..expected.len()], expected);
        assert_eq!(encoded[expected.len()], 0x00);

        let decoded = decode_macro_buffer(&encoded);
        assert_eq!(decoded, slots);
    }

    #[test]
    fn decode_delay_terminated_by_null() {
        let mut buf = vec![0u8; 512];
        // Slot 0: Delay(250) terminated directly by 0x00 (valid per macro_stream.c)
        // Slot 1: "ok"
        buf[0..8].copy_from_slice(b"\x01\x04250\x00ok");
        let decoded = decode_macro_buffer(&buf);
        assert_eq!(decoded[0], vec![MacroStep::Delay(250)]);
        assert_eq!(decoded[1], vec![MacroStep::Text("ok".to_string())]);
    }

    #[test]
    fn overflow_returns_err() {
        let mut slots: Vec<Vec<MacroStep>> = vec![Vec::new(); 16];
        slots[0] = vec![MacroStep::Text("a".repeat(500))];
        assert!(encode_macro_buffer(&slots).is_err());
    }

    #[test]
    fn test_encode_rejects_zero_keycode_in_tap_down_up() {
        for bad in [MacroStep::Tap(0), MacroStep::Down(0), MacroStep::Up(0)] {
            let mut slots: Vec<Vec<MacroStep>> = vec![Vec::new(); 16];
            slots[0] = vec![bad];
            assert!(encode_macro_buffer(&slots).is_err());
        }
    }

    #[test]
    fn test_encode_rejects_control_chars_and_non_ascii() {
        for bad in ["\n", "\x07", "\x1F", "\x7F", "中文"] {
            assert!(validate_macro_text(bad).is_err(), "should reject {:?}", bad);
            let mut slots: Vec<Vec<MacroStep>> = vec![Vec::new(); 16];
            slots[0] = vec![MacroStep::Text(bad.to_string())];
            assert!(encode_macro_buffer(&slots).is_err());
        }
        assert!(validate_macro_text("Hello, World! ~09").is_ok());
    }
}
