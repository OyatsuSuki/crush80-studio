use crate::hid_worker::HidCmd;
use crate::i18n::tr;
use crate::macro_codec::{
    egui_key_to_hid, encode_macro_buffer, encoded_byte_len, format_step, validate_macro_text,
    MacroStep, MACRO_BUFFER_SIZE, MACRO_SLOT_COUNT,
};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{win_sys, Crush80App, WIN32_SPECIAL_HIDS};
use eframe::egui::{self, Color32, RichText, Rounding, Stroke, Vec2};
use std::time::Instant;

impl Crush80App {
    pub fn ui_macros(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let can_write = self.can_via_write();

        // Live keyboard recording capture using focus-gated Win32 hardware-distinguished modifiers
        // (fixes Windows egui `Modifiers::command == ctrl` phantom LWin bug and never spies unfocused apps!).
        if self.macro_recording {
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
                self.macro_prev_special,
                cur_special,
                &WIN32_SPECIAL_HIDS,
                focused,
            );

            for hid in spec_down {
                self.push_recorded_macro_key(hid, true, now);
            }
            if focused {
                for (key, pressed) in events {
                    if let Some(hid) = egui_key_to_hid(key) {
                        if !WIN32_SPECIAL_HIDS.contains(&hid) {
                            self.push_recorded_macro_key(hid, pressed, now);
                        }
                    }
                }
            }
            for hid in spec_up {
                self.push_recorded_macro_key(hid, false, now);
            }
            self.macro_prev_special = next_special;
        }

        let used = encoded_byte_len(&self.macro_slots);
        let over_limit = used > MACRO_BUFFER_SIZE;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.macro_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.macro_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let write_enabled = can_write && !over_limit;
                let write_btn = egui::Button::new(
                    RichText::new(s.write_macros).size(12.0).strong().color(BG_CANVAS),
                )
                .fill(if write_enabled { ACCENT } else { TEXT_DIM })
                .stroke(Stroke::NONE)
                .rounding(Rounding::same(7.0))
                .min_size(Vec2::new(118.0, 28.0));
                if ui.add_enabled(write_enabled, write_btn).clicked() {
                    self.macro_recording = false;
                    self.mark_ui_edit();
                    match encode_macro_buffer(&self.macro_slots) {
                        Ok(raw) => {
                            self.worker.send(HidCmd::WriteMacroBuffer { raw });
                            self.worker.send(HidCmd::TriggerLedFeedback {
                                count: 1,
                                r: 167,
                                g: 139,
                                b: 250,
                            });
                        }
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
                        RichText::new(s.macro_stop)
                            .size(12.0)
                            .strong()
                            .color(Color32::WHITE),
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
                    self.macro_prev_special = win_sys::poll_win32_special_keys();
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
                        ui.label(
                            RichText::new(s.macro_recording_hint)
                                .size(12.0)
                                .strong()
                                .color(ROSE),
                        );
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
                                let b =
                                    egui::Button::new(RichText::new(label).size(11.5).color(col))
                                        .fill(BG_SURFACE)
                                        .stroke(Stroke::new(
                                            1.0_f32,
                                            Color32::from_rgba_unmultiplied(
                                                col.r(),
                                                col.g(),
                                                col.b(),
                                                95,
                                            ),
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

            // Manual Add Text & Add Delay controls (validates printable ASCII immediately!)
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.macro_text_input)
                        .hint_text(s.macro_hint)
                        .desired_width(240.0),
                );
                if chip_button(ui, s.macro_add_text, false) && !self.macro_text_input.is_empty() {
                    match validate_macro_text(&self.macro_text_input) {
                        Ok(()) => {
                            let txt = std::mem::take(&mut self.macro_text_input);
                            self.macro_slots[slot_idx].push(MacroStep::Text(txt));
                        }
                        Err(e) => self.set_toast(e, true),
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
}
