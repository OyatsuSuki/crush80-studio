use crate::hid_worker::HidCmd;
use crate::i18n::tr;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::win_sys;
use crate::Crush80App;
use eframe::egui::{self, Color32, RichText, Rounding, Stroke};

impl Crush80App {
    pub fn ui_hardware(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let can_ember = self.can_ember_write();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.hw_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.hw_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(self.can_via_write(), |ui| {
                    if primary_button(ui, s.save_flash, 118.0) {
                        self.mark_ui_edit();
                        self.worker.send(HidCmd::SetRgb {
                            brightness: None,
                            effect: None,
                            speed: None,
                            color: None,
                            rainbow: None,
                            save: true,
                        });
                        self.worker.send(HidCmd::TriggerLedFeedback {
                            count: 1,
                            r: 245,
                            g: 158,
                            b: 11,
                        });
                    }
                });
            });
        });

        ui.add_space(14.0);

        if self.status.connected && !self.hw.prosuite_supported {
            egui::Frame::none()
                .fill(Color32::from_rgba_unmultiplied(56, 189, 248, 18))
                .stroke(Stroke::new(1.0_f32, CYAN))
                .rounding(Rounding::same(8.0))
                .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(s.prosuite_required_hint)
                            .size(12.0)
                            .color(CYAN),
                    );
                });
            ui.add_space(10.0);
        }

        ui.add_enabled_ui(can_ember, |ui| {
            ui.columns(2, |cols| {
                card_frame().show(&mut cols[0], |ui| {
                    ui.label(RichText::new(s.perf_card).size(14.5).strong().color(TEXT_PRI));
                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(s.low_latency)
                                    .size(13.0)
                                    .strong()
                                    .color(TEXT_PRI),
                            );
                            ui.label(RichText::new(s.low_latency_sub).size(11.0).color(TEXT_SEC));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if toggle_switch(ui, &mut self.hw.low_latency) {
                                self.mark_ui_edit();
                                let on = self.hw.low_latency;
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 29,
                                    val: if on { 0xFF } else { 0x00 },
                                    save: false,
                                });
                                self.worker.send(HidCmd::TriggerLedFeedback {
                                    count: if on { 3 } else { 1 },
                                    r: 255,
                                    g: 255,
                                    b: 255,
                                });
                            }
                        });
                    });

                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(s.win_lock).size(13.0).strong().color(TEXT_PRI),
                            );
                            ui.label(RichText::new(s.win_lock_sub).size(11.0).color(TEXT_SEC));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if toggle_switch(ui, &mut self.hw.win_lock) {
                                self.mark_ui_edit();
                                let locked = self.hw.win_lock;
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 21,
                                    val: if locked { 0xFF } else { 0x00 },
                                    save: false,
                                });
                                self.worker.send(HidCmd::TriggerLedFeedback {
                                    count: if locked { 2 } else { 1 },
                                    r: 255,
                                    g: 160,
                                    b: 0,
                                });
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
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 26,
                                    val: 1,
                                    save: false,
                                });
                                self.worker.send(HidCmd::TriggerLedFeedback {
                                    count: 3,
                                    r: 255,
                                    g: 255,
                                    b: 0,
                                });
                            }
                            if chip_button(ui, "Windows", !self.hw.mac_mode) {
                                self.mark_ui_edit();
                                self.hw.mac_mode = false;
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 26,
                                    val: 0,
                                    save: false,
                                });
                                self.worker.send(HidCmd::TriggerLedFeedback {
                                    count: 1,
                                    r: 255,
                                    g: 255,
                                    b: 0,
                                });
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
                    if ui
                        .add(egui::Slider::new(&mut units, 10..=30).show_value(false))
                        .changed()
                    {
                        self.mark_ui_edit();
                        self.hw.tapping_term_ms = (units as u16) * 10;
                        self.worker.send(HidCmd::SetConfigByte {
                            id: 60,
                            val: units,
                            save: false,
                        });
                    }
                });

                card_frame().show(&mut cols[1], |ui| {
                    ui.label(
                        RichText::new(s.ambient_card)
                            .size(14.5)
                            .strong()
                            .color(TEXT_PRI),
                    );
                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(s.master_rgb)
                                    .size(13.0)
                                    .strong()
                                    .color(TEXT_PRI),
                            );
                            ui.label(RichText::new(s.master_rgb_sub).size(11.0).color(TEXT_SEC));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if toggle_switch(ui, &mut self.hw.sys_rgb_on) {
                                self.mark_ui_edit();
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 40,
                                    val: if self.hw.sys_rgb_on { 0 } else { 1 },
                                    save: false,
                                });
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
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 35,
                                    val: if self.hw.logo_on { 0 } else { 1 },
                                    save: false,
                                });
                            }
                            ui.label(
                                RichText::new(if self.hw.logo_on { s.on } else { s.off })
                                    .size(11.5)
                                    .color(TEXT_SEC),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.mode_lbl).size(12.0).color(TEXT_SEC));
                        for (m, lbl) in [(0u8, "01"), (1, "02"), (2, "03"), (3, "04")] {
                            if chip_button(ui, lbl, self.hw.logo_mode == m) {
                                self.mark_ui_edit();
                                self.hw.logo_mode = m;
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 32,
                                    val: m,
                                    save: false,
                                });
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.color_lbl).size(12.0).color(TEXT_SEC));
                        if chip_button(ui, s.rainbow, self.hw.logo_color == 0xFF) {
                            self.mark_ui_edit();
                            self.hw.logo_color = 0xFF;
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 33,
                                val: 0xFF,
                                save: false,
                            });
                        }
                        if let Some(c) = render_spectrum_dots(ui, self.hw.logo_color) {
                            self.mark_ui_edit();
                            self.hw.logo_color = c;
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 33,
                                val: 0x00,
                                save: false,
                            });
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 34,
                                val: c,
                                save: false,
                            });
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
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 39,
                                    val: if self.hw.side_on { 0 } else { 1 },
                                    save: false,
                                });
                            }
                            ui.label(
                                RichText::new(if self.hw.side_on { s.on } else { s.off })
                                    .size(11.5)
                                    .color(TEXT_SEC),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.mode_lbl).size(12.0).color(TEXT_SEC));
                        for (m, lbl) in [(0u8, "01"), (1, "02"), (2, "03"), (3, "04")] {
                            if chip_button(ui, lbl, self.hw.side_mode == m) {
                                self.mark_ui_edit();
                                self.hw.side_mode = m;
                                self.worker.send(HidCmd::SetConfigByte {
                                    id: 36,
                                    val: m,
                                    save: false,
                                });
                            }
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.color_lbl).size(12.0).color(TEXT_SEC));
                        if chip_button(ui, s.rainbow, self.hw.side_color == 0xFF) {
                            self.mark_ui_edit();
                            self.hw.side_color = 0xFF;
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 37,
                                val: 0xFF,
                                save: false,
                            });
                        }
                        if let Some(c) = render_spectrum_dots(ui, self.hw.side_color) {
                            self.mark_ui_edit();
                            self.hw.side_color = c;
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 37,
                                val: 0x00,
                                save: false,
                            });
                            self.worker.send(HidCmd::SetConfigByte {
                                id: 38,
                                val: c,
                                save: false,
                            });
                        }
                    });
                });
            });
        });

        ui.add_space(12.0);
        // Windows Auto-Start always works regardless of keyboard firmware/connection
        card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(s.autostart)
                                .size(13.5)
                                .strong()
                                .color(TEXT_PRI),
                        );
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
}
