use crate::autoswitch::{save_config, SwitchRule};
use crate::hid_win::RgbState;
use crate::i18n::tr;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::Crush80App;
use eframe::egui::{self, RichText, Rounding, Stroke};
use std::time::{SystemTime, UNIX_EPOCH};

impl Crush80App {
    pub fn ui_autoswitch(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let mut save_needed = false;
        let (last_exe, active_name, mut enabled, mut rules) = {
            let st = self.autoswitch.lock().unwrap_or_else(|e| e.into_inner());
            let active = st.active_rule_id.as_ref().and_then(|id| {
                st.cfg
                    .rules
                    .iter()
                    .find(|r| &r.id == id)
                    .map(|r| r.name.clone())
            });
            (
                st.last_exe.clone(),
                active,
                st.cfg.enabled,
                st.cfg.rules.clone(),
            )
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
                ui.label(
                    RichText::new(if enabled { s.running } else { s.stopped })
                        .size(12.5)
                        .color(if enabled { MINT } else { TEXT_SEC }),
                );
            });
        });

        ui.add_space(14.0);

        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(s.fg_app).size(12.0).color(TEXT_DIM));
                pill_badge(ui, if last_exe.is_empty() { "-" } else { &last_exe }, CYAN);
                ui.add_space(16.0);
                ui.label(RichText::new(s.matched_rule).size(12.0).color(TEXT_DIM));
                pill_badge(
                    ui,
                    active_name.as_deref().unwrap_or(s.default_state),
                    MINT,
                );
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
                            if toggle_switch(ui, &mut r.enabled) {
                                save_needed = true;
                            }
                            if ui
                                .add(egui::TextEdit::singleline(&mut r.name).desired_width(140.0))
                                .changed()
                            {
                                save_needed = true;
                            }
                            if ui
                                .add(egui::TextEdit::singleline(&mut r.exe).desired_width(190.0))
                                .changed()
                            {
                                save_needed = true;
                            }
                            if chip_button(
                                ui,
                                if r.rgb.rainbow { s.rainbow } else { s.single_mode },
                                r.rgb.rainbow,
                            ) {
                                r.rgb.rainbow = !r.rgb.rainbow;
                                save_needed = true;
                            }
                            if ui
                                .add(egui::DragValue::new(&mut r.rgb.effect).range(0..=18))
                                .changed()
                            {
                                save_needed = true;
                            }
                            if ui
                                .add(egui::DragValue::new(&mut r.rgb.brightness).range(0..=9))
                                .changed()
                            {
                                save_needed = true;
                            }
                            if !r.rgb.rainbow
                                && ui
                                    .add(egui::DragValue::new(&mut r.rgb.hue).range(0..=255))
                                    .changed()
                            {
                                save_needed = true;
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if danger_button(ui, s.delete) {
                                    del_idx = Some(idx);
                                }
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
                    id: format!(
                        "rule-{}",
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis()
                    ),
                    name: "Rule".into(),
                    exe: "notepad.exe".into(),
                    action: "rgb".into(),
                    rgb: RgbState {
                        brightness: 6,
                        effect: 6,
                        speed: 2,
                        hue: 135,
                        saturation: 255,
                        rainbow: false,
                    },
                    profile: String::new(),
                    enabled: true,
                });
                save_needed = true;
            }
        });

        if save_needed {
            let save_res = {
                let mut st = self.autoswitch.lock().unwrap_or_else(|e| e.into_inner());
                if enabled && !st.cfg.enabled {
                    st.cfg.default_rgb = Some(self.rgb.clone());
                }
                st.cfg.enabled = enabled;
                st.cfg.rules = rules;
                st.active_rule_id = None;
                save_config(&self.autoswitch_path, &st.cfg)
            };
            if let Err(e) = save_res {
                self.set_toast(e, true);
            }
        }
    }
}
