use crate::i18n::tr;
use crate::keycodes::{decode_keycode_for, factory_default_key_for, layout_for};
use crate::macro_codec::egui_key_to_hid;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{win_sys, Crush80App, WIN32_SPECIAL_HIDS};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Stroke, Vec2};
use std::time::Instant;

impl Crush80App {
    pub fn ui_tester(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);

        // Capture live keyboard events + Win32 hardware-distinguished modifiers & special keys
        // (eliminates Windows egui `Modifiers::command == ctrl` phantom LWin bug and detects
        // LCtrl/RCtrl/LShift/RShift/LAlt/RAlt/LWin/RWin/CapsLock/PrtSc/ScrLk/Pause/Menu/F13!)
        let now = Instant::now();
        let (events, focused) = ui.ctx().input(|i| {
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
            (evs, i.focused)
        });

        let cur_special = win_sys::poll_win32_special_keys();
        let (spec_down, spec_up, next_special) = win_sys::step_special_keys(
            self.tester_prev_special,
            cur_special,
            &WIN32_SPECIAL_HIDS,
            focused,
        );
        for hid in spec_down {
            self.record_tester_press(hid, now);
        }
        for hid in spec_up {
            self.tester_pressed.remove(&hid);
        }
        self.tester_prev_special = next_special;

        if focused {
            for (key, pressed) in events {
                if let Some(hid) = egui_key_to_hid(key) {
                    if !WIN32_SPECIAL_HIDS.contains(&hid) {
                        if pressed {
                            self.record_tester_press(hid, now);
                        } else {
                            self.tester_pressed.remove(&hid);
                        }
                    }
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
        let (origin, unit) = allocate_keyboard_chassis(ui);
        let p = ui.painter();

        for k in layout_for(self.edition) {
            let rect = Rect::from_min_size(
                Pos2::new(origin.x + k.x * unit + 2.5, origin.y + k.y * unit + 2.5),
                Vec2::new(k.w * unit - 5.0, unit - 5.0),
            );
            let idx = (k.r as usize) * 16 + (k.c as usize);
            // Match either current Layer 0 basic keycode or factory default basic keycode
            let cur_code = self.layers[0][idx];
            let factory_code = factory_default_key_for(self.edition, 0, k.r, k.c);
            let cur_hid = if cur_code <= 0xFF { cur_code as u8 } else { 0 };
            let fac_hid = if factory_code <= 0xFF {
                factory_code as u8
            } else {
                0
            };
            let match_set = |set: &std::collections::HashSet<u8>| -> bool {
                (cur_hid != 0 && set.contains(&cur_hid)) || (fac_hid != 0 && set.contains(&fac_hid))
            };

            let is_pressed = match_set(&self.tester_pressed);
            let is_chatter = match_set(&self.tester_chatter_keys);
            let is_tested = match_set(&self.tester_tested);

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

            let dish = Rect::from_min_max(
                rect.min + Vec2::new(2.0, 1.5),
                rect.max - Vec2::new(2.0, 4.5),
            );
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

            let dec = decode_keycode_for(cur_code, self.lang, self.edition);
            let legend_col = if is_pressed {
                Color32::WHITE
            } else if is_chatter {
                ROSE
            } else if is_tested {
                MINT
            } else {
                TEXT_PRI
            };
            let fs = if dec.label.chars().count() > 5 {
                9.0
            } else {
                10.8
            };
            p.text(
                dish.center(),
                egui::Align2::CENTER_CENTER,
                &dec.label,
                egui::FontId::proportional(fs),
                legend_col,
            );
        }
    }
}
