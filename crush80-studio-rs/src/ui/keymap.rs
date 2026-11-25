use crate::hid_worker::HidCmd;
use crate::i18n::tr;
use crate::keycodes::{
    basic_keycode_label, decode_keycode_for, decode_keycode_lang, f13_matrix, layout_for,
    special_keycodes_for, BoardEdition, KeyCategory, BASIC_KEYCODES,
};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{Crush80App, PickerTab};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

impl Crush80App {
    pub fn ui_keymap(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let can_write = self.can_via_write();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.keymap_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.keymap_sub).size(12.0).color(TEXT_SEC));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(9.0))
                    .inner_margin(egui::Margin::same(4.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let names = ["L0 Win", "L1 Win Fn", "L2 Mac", "L3 Mac Fn"];
                        // Inside right_to_left, render reversed so visual order is L0 -> L3 and box hugs content tightly
                        for (i, n) in names.iter().enumerate().rev() {
                            let active = self.current_layer == i;
                            let b = egui::Button::new(
                                RichText::new(*n)
                                    .size(12.0)
                                    .strong()
                                    .color(if active { BG_CANVAS } else { TEXT_SEC }),
                            )
                            .fill(if active { ACCENT } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(6.0))
                            .min_size(Vec2::new(72.0, 26.0));
                            if ui.add(b).clicked() {
                                self.current_layer = i;
                                self.sync_selected_hex();
                            }
                        }
                    });
            });
        });

        ui.add_space(10.0);
        self.draw_keyboard_twin(ui, true);
        ui.add_space(10.0);

        ui.columns(2, |cols| {
            // Left: 4-Layer Inspector
            card_frame().show(&mut cols[0], |ui| {
                ui.set_min_height(256.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(s.inspector).size(14.5).strong().color(TEXT_PRI));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some((r, c)) = self.selected_key {
                            pill_badge(ui, &format!("R{} · C{}", r, c), CYAN);
                        }
                        ui.add_enabled_ui(can_write, |ui| {
                            if chip_button(ui, s.restore_layer, false) {
                                self.restore_current_layer();
                            }
                            if self.selected_key.is_some() && chip_button(ui, s.restore_key, false) {
                                self.restore_selected_key();
                            }
                        });
                    });
                });
                ui.add_space(10.0);

                if let Some((r, c)) = self.selected_key {
                    let idx = (r as usize) * 16 + (c as usize);
                    ui.columns(4, |lcols| {
                        for l in 0..4 {
                            let dec =
                                decode_keycode_for(self.layers[l][idx], self.lang, self.edition);
                            let is_cur = l == self.current_layer;
                            egui::Frame::none()
                                .fill(if is_cur { ACCENT_SOFT } else { BG_SURFACE })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if is_cur { ACCENT } else { HAIRLINE },
                                ))
                                .rounding(Rounding::same(8.0))
                                .inner_margin(egui::Margin::symmetric(8.0, 7.0))
                                .show(&mut lcols[l], |ui| {
                                    ui.label(
                                        RichText::new(format!("L{}", l))
                                            .size(10.0)
                                            .color(if is_cur { ACCENT } else { TEXT_DIM }),
                                    );
                                    ui.label(
                                        RichText::new(&dec.label).size(13.0).strong().color(TEXT_PRI),
                                    );
                                    ui.label(RichText::new(&dec.name).size(9.5).color(TEXT_SEC));
                                });
                        }
                    });
                }

                ui.add_space(12.0);
                ui.label(RichText::new(s.presets).size(12.0).strong().color(TEXT_SEC));
                ui.add_space(6.0);
                ui.add_enabled_ui(can_write, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (label, code) in [
                            ("Caps -> Ctrl/Esc", 0x2129),
                            ("Space -> L1/Space", 0x412C),
                            ("CapsWord", 0x7C73),
                            ("Esc / ~", 0x7C16),
                            ("OSM Shift", 0x52A2),
                            ("OSL Layer 1", 0x5281),
                        ] {
                            if chip_button(ui, label, false) {
                                self.apply_keycode(code);
                            }
                        }
                    });

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(s.hex_write).size(12.0).color(TEXT_SEC));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.raw_hex_input)
                                .desired_width(100.0),
                        );
                        if primary_button(ui, s.apply, 76.0) {
                            let clean = self
                                .raw_hex_input
                                .trim()
                                .trim_start_matches("0x")
                                .trim_start_matches("0X");
                            match u16::from_str_radix(clean, 16) {
                                Ok(v) => self.apply_keycode(v),
                                Err(_) => self.set_toast(
                                    format!("无效的 16 进制键码: {}", self.raw_hex_input),
                                    true,
                                ),
                            }
                        }
                    });
                });
            });

            // Right: Modular Key Drawer
            card_frame().show(&mut cols[1], |ui| {
                ui.set_min_height(256.0);
                ui.horizontal(|ui| {
                    for (t, name) in [
                        (PickerTab::ProBuilder, s.tab_builder),
                        (PickerTab::Basic, s.tab_basic),
                        (PickerTab::Layer, s.tab_layer),
                        (PickerTab::Wob, s.tab_wob),
                        (PickerTab::Macro, s.tab_macro),
                    ] {
                        if chip_button(ui, name, self.picker_tab == t) {
                            self.picker_tab = t;
                        }
                    }
                });
                ui.add_space(12.0);

                ui.add_enabled_ui(can_write, |ui| match self.picker_tab {
                    PickerTab::ProBuilder => {
                        ui.label(RichText::new(s.mt_title).size(12.0).color(VIOLET).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("mt_m")
                                .selected_text(mod_label(self.mt_mod))
                                .width(115.0)
                                .show_ui(ui, |ui| {
                                    for (v, n) in [
                                        (1, "LCtrl"),
                                        (2, "LShift"),
                                        (4, "LAlt"),
                                        (8, "LWin"),
                                        (3, "Ctrl+Shift"),
                                        (15, "Hyper"),
                                    ] {
                                        ui.selectable_value(&mut self.mt_mod, v, n);
                                    }
                                });
                            egui::ComboBox::from_id_salt("mt_k")
                                .selected_text(decode_keycode_lang(self.mt_key, self.lang).label)
                                .width(95.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, _) in
                                        BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65)
                                    {
                                        let lbl = basic_keycode_label(c, self.lang);
                                        ui.selectable_value(&mut self.mt_key, c, lbl);
                                    }
                                });
                            if primary_button(ui, s.apply, 72.0) {
                                self.apply_keycode(
                                    0x2000
                                        | (((self.mt_mod as u16) & 0x1F) << 8)
                                        | (self.mt_key & 0xFF),
                                );
                            }
                        });

                        ui.add_space(10.0);
                        ui.label(RichText::new(s.lt_title).size(12.0).color(VIOLET).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt("lt_l")
                                .selected_text(format!("Layer {}", self.lt_layer))
                                .width(115.0)
                                .show_ui(ui, |ui| {
                                    for l in 0..4u8 {
                                        ui.selectable_value(
                                            &mut self.lt_layer,
                                            l,
                                            format!("Layer {}", l),
                                        );
                                    }
                                });
                            egui::ComboBox::from_id_salt("lt_k")
                                .selected_text(decode_keycode_lang(self.lt_key, self.lang).label)
                                .width(95.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, _) in
                                        BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65)
                                    {
                                        let lbl = basic_keycode_label(c, self.lang);
                                        ui.selectable_value(&mut self.lt_key, c, lbl);
                                    }
                                });
                            if primary_button(ui, s.apply, 72.0) {
                                self.apply_keycode(
                                    0x4000
                                        | (((self.lt_layer as u16) & 0x0F) << 8)
                                        | (self.lt_key & 0xFF),
                                );
                            }
                        });

                        ui.add_space(10.0);
                        ui.label(RichText::new(s.mods_title).size(12.0).color(MINT).strong());
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if chip_button(ui, "Ctrl", self.mod_c) {
                                self.mod_c = !self.mod_c;
                            }
                            if chip_button(ui, "Shift", self.mod_s) {
                                self.mod_s = !self.mod_s;
                            }
                            if chip_button(ui, "Alt", self.mod_a) {
                                self.mod_a = !self.mod_a;
                            }
                            if chip_button(ui, "Win", self.mod_g) {
                                self.mod_g = !self.mod_g;
                            }
                            egui::ComboBox::from_id_salt("mk")
                                .selected_text(decode_keycode_lang(self.mod_key, self.lang).label)
                                .width(72.0)
                                .show_ui(ui, |ui| {
                                    for &(c, _, _) in
                                        BASIC_KEYCODES.iter().filter(|k| k.0 >= 4 && k.0 <= 0x65)
                                    {
                                        let lbl = basic_keycode_label(c, self.lang);
                                        ui.selectable_value(&mut self.mod_key, c, lbl);
                                    }
                                });
                            if primary_button(ui, s.apply, 58.0) {
                                let mut m = 0u16;
                                if self.mod_c {
                                    m |= 1;
                                }
                                if self.mod_s {
                                    m |= 2;
                                }
                                if self.mod_a {
                                    m |= 4;
                                }
                                if self.mod_g {
                                    m |= 8;
                                }
                                if m > 0 {
                                    self.apply_keycode((m << 8) | (self.mod_key & 0xFF));
                                }
                            }
                        });
                    }
                    PickerTab::Basic => {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.kc_search)
                                .hint_text(s.search_hint)
                                .desired_width(ui.available_width()),
                        );
                        ui.add_space(8.0);
                        let q = self.kc_search.to_lowercase();
                        egui::ScrollArea::vertical().max_height(185.0).show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for &(code, name, zh_label) in BASIC_KEYCODES {
                                    let loc_label = basic_keycode_label(code, self.lang);
                                    if !q.is_empty()
                                        && !name.to_lowercase().contains(&q)
                                        && !loc_label.to_lowercase().contains(&q)
                                        && !zh_label.to_lowercase().contains(&q)
                                    {
                                        continue;
                                    }
                                    if chip_button(ui, &loc_label, false) {
                                        self.apply_keycode(code);
                                    }
                                }
                            });
                        });
                    }
                    PickerTab::Layer => {
                        let ops = [
                            (0x0001, "▽ TRNS"),
                            (0x0000, "NO"),
                            (0x5221, "MO(1)"),
                            (0x5222, "MO(2)"),
                            (0x5223, "MO(3)"),
                            (0x5261, "TG(1)"),
                            (0x5262, "TG(2)"),
                            (0x5263, "TG(3)"),
                            (0x5200, "TO(0)"),
                            (0x5201, "TO(1)"),
                            (0x5202, "TO(2)"),
                            (0x5240, "DF(0)"),
                            (0x5242, "DF(2)"),
                            (0x5281, "OSL(1)"),
                            (0x52C1, "TT(1)"),
                        ];
                        ui.horizontal_wrapped(|ui| {
                            for (c, l) in ops {
                                if chip_button(ui, l, false) {
                                    self.apply_keycode(c);
                                }
                            }
                        });
                    }
                    PickerTab::Wob => {
                        ui.horizontal_wrapped(|ui| {
                            for &(code, _, _) in special_keycodes_for(self.edition) {
                                let lbl = decode_keycode_for(code, self.lang, self.edition).label;
                                if chip_button(ui, &lbl, false) {
                                    self.apply_keycode(code);
                                }
                            }
                        });
                    }
                    PickerTab::Macro => {
                        ui.horizontal_wrapped(|ui| {
                            for i in 0..16u16 {
                                if chip_button(ui, &format!("M{}", i), false) {
                                    self.apply_keycode(0x7700 + i);
                                }
                            }
                        });
                    }
                });
            });
        });
    }

    pub fn draw_keyboard_twin(&mut self, ui: &mut egui::Ui, interactive: bool) {
        let (origin, unit) = allocate_keyboard_chassis(ui);
        let p = ui.painter();
        let t = self.start_time.elapsed().as_secs_f32();
        let mut clicked = None;

        let f13 = f13_matrix(self.edition);
        for k in layout_for(self.edition) {
            let rect = Rect::from_min_size(
                Pos2::new(origin.x + k.x * unit + 2.5, origin.y + k.y * unit + 2.5),
                Vec2::new(k.w * unit - 5.0, unit - 5.0),
            );
            let resp = if interactive {
                Some(ui.interact(rect, ui.id().with(("kcap", k.r, k.c)), Sense::click()))
            } else {
                None
            };
            if let Some(ref r) = resp {
                if r.clicked() {
                    clicked = Some((k.r, k.c));
                }
            }

            let selected = interactive && self.selected_key == Some((k.r, k.c));
            let hovered = resp.as_ref().map(|r| r.hovered()).unwrap_or(false);
            let code = self.layers[self.current_layer][(k.r as usize) * 16 + (k.c as usize)];
            let dec = decode_keycode_for(code, self.lang, self.edition);

            let glow_col = if self.edition == BoardEdition::LiteNoRgb
                || !self.hw.sys_rgb_on
                || self.rgb.effect == 0
                || self.rgb.brightness == 0
            {
                None
            } else if self.rgb.rainbow {
                let hue = (((k.x / 18.25) * 255.0 + t * (24.0 + self.rgb.speed as f32 * 14.0))
                    as u32
                    % 256) as u8;
                Some(hsv_to_color32(hue, 230))
            } else {
                Some(hsv_to_color32(self.rgb.hue, self.rgb.saturation))
            };

            if let Some(gc) = glow_col {
                let alpha = (18 + self.rgb.brightness * 6).min(75);
                let bleed = Color32::from_rgba_unmultiplied(gc.r(), gc.g(), gc.b(), alpha);
                p.rect_filled(rect.expand(1.5), Rounding::same(7.5), bleed);
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
            let dish_col = if selected {
                Color32::from_rgb(48, 36, 18)
            } else if hovered {
                Color32::from_rgb(40, 48, 64)
            } else {
                match k.style {
                    2 => Color32::from_rgb(56, 40, 20),
                    1 => Color32::from_rgb(27, 32, 44),
                    _ => Color32::from_rgb(35, 41, 55),
                }
            };
            p.rect_filled(dish, Rounding::same(5.0), dish_col);

            let cat_col = match dec.category {
                KeyCategory::TapHold => Some(VIOLET),
                KeyCategory::Layer => Some(ACCENT),
                KeyCategory::Mods => Some(MINT),
                KeyCategory::Macro => Some(ROSE),
                _ => None,
            };
            if let Some(cc) = cat_col {
                p.circle_filled(Pos2::new(dish.max.x - 5.0, dish.min.y + 5.0), 2.3, cc);
            }

            if selected {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(2.0_f32, ACCENT));
            } else if hovered {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(1.2_f32, TEXT_SEC));
            }

            let legend_col = if dec.code == 0 {
                TEXT_DIM
            } else if selected {
                ACCENT
            } else if let Some(gc) = glow_col {
                blend_color(TEXT_PRI, gc, 0.25)
            } else {
                TEXT_PRI
            };
            if (k.r, k.c) == f13 {
                p.text(
                    Pos2::new(dish.center().x, dish.center().y - 5.0),
                    egui::Align2::CENTER_CENTER,
                    "F13",
                    egui::FontId::proportional(10.5),
                    legend_col,
                );
                p.text(
                    Pos2::new(dish.center().x, dish.center().y + 7.5),
                    egui::Align2::CENTER_CENTER,
                    &dec.label,
                    egui::FontId::proportional(8.0),
                    TEXT_SEC,
                );
            } else {
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

        if let Some(sel) = clicked {
            self.selected_key = Some(sel);
            self.sync_selected_hex();
        }
    }

    pub fn apply_keycode(&mut self, code: u16) {
        let Some((r, c)) = self.selected_key else {
            return;
        };
        self.mark_ui_edit();
        self.worker.send(HidCmd::SetKey {
            layer: self.current_layer as u8,
            row: r,
            col: c,
            keycode: code,
        });
    }
}
