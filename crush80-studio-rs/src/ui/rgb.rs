use crate::hid_worker::HidCmd;
use crate::i18n::{norgb_effect_label, rgb_effect_label, tr};
use crate::keycodes::BoardEdition;
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::Crush80App;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

impl Crush80App {
    pub fn ui_rgb(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let is_lite = self.edition == BoardEdition::LiteNoRgb;
        let can_write = self.can_via_write();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if is_lite { s.logo_rgb_title } else { s.rgb_title })
                        .size(19.0)
                        .strong()
                        .color(TEXT_PRI),
                );
                ui.label(
                    RichText::new(if is_lite { s.logo_rgb_sub } else { s.rgb_sub })
                        .size(12.0)
                        .color(TEXT_SEC),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_enabled_ui(can_write, |ui| {
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

        ui.add_space(12.0);
        self.draw_keyboard_twin(ui, false);
        ui.add_space(12.0);

        ui.add_enabled_ui(can_write, |ui| {
            ui.columns(2, |cols| {
                card_frame().show(&mut cols[0], |ui| {
                    ui.label(RichText::new(s.color_mode).size(14.5).strong().color(TEXT_PRI));
                    ui.add_space(10.0);

                    if self.hw.prosuite_supported && !is_lite {
                        egui::Frame::none()
                            .fill(BG_SURFACE)
                            .stroke(Stroke::new(1.0_f32, HAIRLINE))
                            .rounding(Rounding::same(9.0))
                            .inner_margin(egui::Margin::same(4.0))
                            .show(ui, |ui| {
                                ui.columns(2, |scols| {
                                    let rb = self.rgb.rainbow;
                                    let b1 = egui::Button::new(
                                        RichText::new(s.rainbow_mode)
                                            .size(12.5)
                                            .strong()
                                            .color(if rb { BG_CANVAS } else { TEXT_SEC }),
                                    )
                                    .fill(if rb { ACCENT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(6.0))
                                    .min_size(Vec2::new(scols[0].available_width(), 30.0));
                                    if scols[0].add(b1).clicked() {
                                        self.mark_ui_edit();
                                        self.rgb.rainbow = true;
                                        self.worker.send(HidCmd::SetRgb {
                                            brightness: None,
                                            effect: None,
                                            speed: None,
                                            color: None,
                                            rainbow: Some(true),
                                            save: false,
                                        });
                                    }

                                    let b2 = egui::Button::new(
                                        RichText::new(s.single_mode)
                                            .size(12.5)
                                            .strong()
                                            .color(if !rb { BG_CANVAS } else { TEXT_SEC }),
                                    )
                                    .fill(if !rb { ACCENT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::NONE)
                                    .rounding(Rounding::same(6.0))
                                    .min_size(Vec2::new(scols[1].available_width(), 30.0));
                                    if scols[1].add(b2).clicked() {
                                        self.mark_ui_edit();
                                        self.rgb.rainbow = false;
                                        self.worker.send(HidCmd::SetRgb {
                                            brightness: None,
                                            effect: None,
                                            speed: None,
                                            color: Some((self.rgb.hue, self.rgb.saturation)),
                                            rainbow: Some(false),
                                            save: false,
                                        });
                                    }
                                });
                            });
                        ui.add_space(12.0);
                    }

                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.effect).size(12.5).color(TEXT_SEC));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let max_eff = if is_lite { 4u8 } else { 18u8 };
                            let cur = if is_lite {
                                norgb_effect_label(self.lang, self.rgb.effect.min(4))
                            } else {
                                rgb_effect_label(self.lang, self.rgb.effect)
                            };
                            egui::ComboBox::from_id_salt("rgb_eff")
                                .selected_text(cur)
                                .width(210.0)
                                .show_ui(ui, |ui| {
                                    for id in 0..=max_eff {
                                        let lbl = if is_lite {
                                            norgb_effect_label(self.lang, id)
                                        } else {
                                            rgb_effect_label(self.lang, id)
                                        };
                                        if ui.selectable_value(&mut self.rgb.effect, id, lbl).changed()
                                        {
                                            self.mark_ui_edit();
                                            self.worker.send(HidCmd::SetRgb {
                                                brightness: None,
                                                effect: Some(id),
                                                speed: None,
                                                color: None,
                                                rainbow: None,
                                                save: false,
                                            });
                                        }
                                    }
                                });
                        });
                    });

                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(format!("{}  {} / 9", s.brightness, self.rgb.brightness))
                            .size(12.0)
                            .color(TEXT_SEC),
                    );
                    if ui
                        .add(egui::Slider::new(&mut self.rgb.brightness, 0..=9).show_value(false))
                        .changed()
                    {
                        self.mark_ui_edit();
                        self.worker.send(HidCmd::SetRgb {
                            brightness: Some(self.rgb.brightness),
                            effect: None,
                            speed: None,
                            color: None,
                            rainbow: None,
                            save: false,
                        });
                    }

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("{}  {} / 4", s.speed, self.rgb.speed))
                            .size(12.0)
                            .color(TEXT_SEC),
                    );
                    if ui
                        .add(egui::Slider::new(&mut self.rgb.speed, 0..=4).show_value(false))
                        .changed()
                    {
                        self.mark_ui_edit();
                        self.worker.send(HidCmd::SetRgb {
                            brightness: None,
                            effect: None,
                            speed: Some(self.rgb.speed),
                            color: None,
                            rainbow: None,
                            save: false,
                        });
                    }
                });

                card_frame().show(&mut cols[1], |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.color_pad).size(14.5).strong().color(TEXT_PRI));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            pill_badge(
                                ui,
                                &format!("H:{}  S:{}", self.rgb.hue, self.rgb.saturation),
                                ACCENT,
                            );
                        });
                    });
                    ui.add_space(10.0);

                    let pad_w = ui.available_width();
                    let pad_h = 112.0;
                    let (pad_rect, resp) =
                        ui.allocate_exact_size(Vec2::new(pad_w, pad_h), Sense::click_and_drag());
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
                                Pos2::new(
                                    pad_rect.min.x + ix as f32 * cw,
                                    pad_rect.min.y + iy as f32 * ch,
                                ),
                                Vec2::new(cw + 0.8, ch + 0.8),
                            );
                            p.rect_filled(r, Rounding::ZERO, hsv_to_color32(h, sat));
                        }
                    }
                    p.rect_stroke(pad_rect, Rounding::same(8.0), Stroke::new(1.5_f32, HAIRLINE));

                    let rx = pad_rect.min.x + (self.rgb.hue as f32 / 255.0) * pad_rect.width();
                    let ry = pad_rect.min.y
                        + (1.0 - self.rgb.saturation as f32 / 255.0) * pad_rect.height();
                    p.circle_stroke(Pos2::new(rx, ry), 7.0, Stroke::new(2.0_f32, Color32::WHITE));
                    p.circle_stroke(Pos2::new(rx, ry), 8.2, Stroke::new(1.0_f32, Color32::BLACK));

                    if can_write && (resp.clicked() || resp.dragged()) {
                        if let Some(pos) = resp.interact_pointer_pos() {
                            let nx = ((pos.x - pad_rect.min.x) / pad_rect.width()).clamp(0.0, 1.0);
                            let ny =
                                ((pos.y - pad_rect.min.y) / pad_rect.height()).clamp(0.0, 1.0);
                            let nh = (nx * 255.0) as u8;
                            let ns = ((1.0 - ny) * 255.0) as u8;
                            if nh != self.rgb.hue || ns != self.rgb.saturation || self.rgb.rainbow {
                                self.mark_ui_edit();
                                self.rgb.hue = nh;
                                self.rgb.saturation = ns;
                                self.rgb.rainbow = false;
                                self.worker.send(HidCmd::SetRgb {
                                    brightness: None,
                                    effect: None,
                                    speed: None,
                                    color: Some((nh, ns)),
                                    rainbow: Some(false),
                                    save: false,
                                });
                            }
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
                            (105, 255, MINT),
                            (192, 255, VIOLET),
                            (245, 255, ROSE),
                        ];
                        for (h, sat, col) in swatches {
                            let (r, resp) =
                                ui.allocate_exact_size(Vec2::new(26.0, 26.0), Sense::click());
                            ui.painter().circle_filled(r.center(), 11.0, col);
                            if resp.hovered() {
                                ui.painter().circle_stroke(
                                    r.center(),
                                    12.5,
                                    Stroke::new(1.5_f32, TEXT_PRI),
                                );
                            }
                            if can_write && resp.clicked() {
                                self.mark_ui_edit();
                                self.rgb.hue = h;
                                self.rgb.saturation = sat;
                                self.rgb.rainbow = false;
                                self.worker.send(HidCmd::SetRgb {
                                    brightness: None,
                                    effect: None,
                                    speed: None,
                                    color: Some((h, sat)),
                                    rainbow: Some(false),
                                    save: false,
                                });
                            }
                            ui.add_space(4.0);
                        }
                    });
                });
            });
        });
    }
}
