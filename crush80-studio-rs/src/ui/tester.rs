use crate::i18n::tr;
use crate::keycodes::{decode_keycode_for, factory_default_key_for, layout_for};
use crate::macro_codec::{egui_key_to_hid, hid_to_name};
use crate::ui::theme::*;
use crate::ui::widgets::*;
use crate::{win_sys, Crush80App, TesterEvent, WIN32_SPECIAL_HIDS};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};
use std::time::Instant;

struct DiagRow {
    hid: u8,
    count: u64,
    best_gap: Option<u64>,
    hold: Option<u64>,
    chatters: u64,
}

/// Returns true for firmware-internal layer/system keycodes (e.g. Fn1 / MO(1)) that never emit an OS HID report.
fn is_firmware_internal_key(kc: u16) -> bool {
    (0x5100..=0x5FFF).contains(&kc) || (0x7F00..=0x7FFF).contains(&kc)
}

/// Paint an indivisible keystroke chip with exact measured width so `horizontal_wrapped`
/// always moves the whole chip to the next line and NEVER breaks characters vertically.
fn draw_event_chip(ui: &mut egui::Ui, ev: &TesterEvent, is_latest: bool) {
    let key_font = egui::FontId::proportional(11.5);
    let meta_font = egui::FontId::proportional(10.5);

    let key_fg = if ev.is_chatter {
        ROSE
    } else if is_latest {
        CYAN
    } else {
        TEXT_PRI
    };

    let meta_str = match (ev.dt_ms, ev.hold_ms) {
        (Some(d), Some(h)) => format!("{}ms · {}ms", d, h),
        (Some(d), None) => format!("{}ms", d),
        (None, Some(h)) => format!("{}ms", h),
        (None, None) => String::new(),
    };
    let meta_fg = if ev.is_chatter { ROSE } else { TEXT_SEC };

    let key_galley = ui.painter().layout_no_wrap(ev.name.clone(), key_font, key_fg);
    let meta_galley = if meta_str.is_empty() {
        None
    } else {
        Some(ui.painter().layout_no_wrap(meta_str, meta_font, meta_fg))
    };

    let pad_x = 9.0;
    let gap = if meta_galley.is_some() { 6.0 } else { 0.0 };
    let total_w = pad_x * 2.0
        + key_galley.size().x
        + gap
        + meta_galley.as_ref().map_or(0.0, |g| g.size().x);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, 25.0), Sense::hover());

    let (fill, stroke) = if ev.is_chatter {
        (
            Color32::from_rgba_unmultiplied(ROSE.r(), ROSE.g(), ROSE.b(), 30),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(ROSE.r(), ROSE.g(), ROSE.b(), 150)),
        )
    } else if is_latest {
        (
            Color32::from_rgba_unmultiplied(CYAN.r(), CYAN.g(), CYAN.b(), 24),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(CYAN.r(), CYAN.g(), CYAN.b(), 130)),
        )
    } else {
        (BG_SURFACE, Stroke::new(1.0_f32, HAIRLINE))
    };

    let p = ui.painter();
    p.rect_filled(rect, Rounding::same(6.0), fill);
    p.rect_stroke(rect, Rounding::same(6.0), stroke);

    let key_pos = Pos2::new(
        rect.min.x + pad_x,
        rect.center().y - key_galley.size().y * 0.5,
    );
    let key_w = key_galley.size().x;
    p.galley(key_pos, key_galley, key_fg);

    if let Some(mg) = meta_galley {
        let meta_pos = Pos2::new(
            rect.min.x + pad_x + key_w + gap,
            rect.center().y - mg.size().y * 0.5,
        );
        p.galley(meta_pos, mg, meta_fg);
    }
}

impl Crush80App {
    pub fn ui_tester(&mut self, ui: &mut egui::Ui) {
        let s = tr(self.lang);

        // Capture live keyboard events + Win32 hardware-distinguished modifiers & special keys
        // (LCtrl/RCtrl/LShift/RShift/LAlt/RAlt/LWin/RWin/CapsLock/PrtSc/ScrLk/Pause/Menu/F13/Mute).
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

        if focused {
            // Surrender egui widget focus on the Key Tester page so pressing Space, Enter,
            // Tab, or arrow keys NEVER accidentally activates buttons like "Clear Stats".
            ui.ctx().memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });

            let cur_special = win_sys::poll_win32_special_keys();
            let (spec_down, spec_up, next_special) = win_sys::step_special_keys(
                self.tester_prev_special,
                cur_special,
                &WIN32_SPECIAL_HIDS,
                true,
            );
            for hid in spec_down {
                self.record_tester_press(hid, now);
            }
            for hid in spec_up {
                self.record_tester_release(hid, now);
            }
            self.tester_prev_special = next_special;

            for (key, pressed) in events {
                if let Some(hid) = egui_key_to_hid(key) {
                    if !WIN32_SPECIAL_HIDS.contains(&hid) {
                        if pressed {
                            self.record_tester_press(hid, now);
                        } else {
                            self.record_tester_release(hid, now);
                        }
                    }
                }
            }
        } else {
            // Focus lost (e.g. screenshot hotkey Ctrl+Alt+A or Alt+Tab):
            // release all currently active keys immediately so nothing ever sticks blue.
            if !self.tester_pressed.is_empty() {
                let held: Vec<u8> = self.tester_pressed.iter().copied().collect();
                for hid in held {
                    self.record_tester_release(hid, now);
                }
            }
            self.tester_prev_special = [false; win_sys::SPECIAL_KEY_COUNT];
        }

        // Compute testable key coverage (exclude firmware-internal keys like Fn1)
        let layout = layout_for(self.edition);
        let mut testable_total = 0usize;
        let mut testable_hit = 0usize;
        for k in layout {
            let idx = (k.r as usize) * 16 + (k.c as usize);
            let cur_code = self.layers[0][idx];
            let fac_code = factory_default_key_for(self.edition, 0, k.r, k.c);
            if is_firmware_internal_key(cur_code) && is_firmware_internal_key(fac_code) {
                continue;
            }
            testable_total += 1;
            let cur_hid = if cur_code <= 0xFF { cur_code as u8 } else { 0 };
            let fac_hid = if fac_code <= 0xFF { fac_code as u8 } else { 0 };
            if (cur_hid != 0 && self.tester_tested.contains(&cur_hid))
                || (fac_hid != 0 && self.tester_tested.contains(&fac_hid))
            {
                testable_hit += 1;
            }
        }

        // Header: Title + Legend + Chatter Threshold selector + Reset
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(s.tester_title).size(19.0).strong().color(TEXT_PRI));
                ui.label(RichText::new(s.tester_sub).size(12.0).color(TEXT_SEC));
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if chip_button(ui, s.tester_reset, false) {
                    self.reset_tester();
                }

                ui.add_space(6.0);
                // Threshold segmented control (rendered in reverse inside right_to_left)
                egui::Frame::none()
                    .fill(BG_CARD)
                    .stroke(Stroke::new(1.0_f32, HAIRLINE))
                    .rounding(Rounding::same(8.0))
                    .inner_margin(egui::Margin::same(3.0))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for &ms in [100u64, 80, 50].iter() {
                            let active = self.tester_threshold_ms == ms;
                            let btn = egui::Button::new(
                                RichText::new(format!("{}ms", ms))
                                    .size(11.0)
                                    .strong()
                                    .color(if active { BG_CANVAS } else { TEXT_SEC }),
                            )
                            .fill(if active { ACCENT } else { Color32::TRANSPARENT })
                            .stroke(Stroke::NONE)
                            .rounding(Rounding::same(5.5))
                            .min_size(Vec2::new(42.0, 22.0));
                            if ui.add(btn).clicked() {
                                self.tester_threshold_ms = ms;
                            }
                        }
                    });
                ui.label(RichText::new(s.tester_threshold).size(11.5).color(TEXT_SEC));

                ui.add_space(10.0);
                for (col, lbl) in [
                    (ROSE, s.tester_legend_chatter),
                    (MINT, s.tester_legend_ok),
                    (CYAN, s.tester_legend_active),
                ] {
                    ui.label(RichText::new(lbl).size(11.0).color(TEXT_SEC));
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(7.5), Sense::hover());
                    ui.painter().circle_filled(r.center(), 3.5, col);
                    ui.add_space(4.0);
                }
            });
        });

        ui.add_space(10.0);
        self.draw_keyboard_twin_tester(ui);
        ui.add_space(12.0);

        // Two-column diagnostic dashboard (matches Keymap & Hardware page rhythm)
        let total_w = ui.available_width();
        let col_gap = 12.0;
        let left_w = ((total_w - col_gap) * 0.56).floor();
        let right_w = (total_w - col_gap - left_w).max(240.0);
        let card_h = 248.0;

        ui.horizontal_top(|ui| {
            // LEFT CARD: KPI bar + Live Keystroke Stream (indivisible chips)
            ui.allocate_ui_with_layout(
                Vec2::new(left_w, card_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    card_frame().show(ui, |ui| {
                        ui.set_min_width(left_w - 28.0);
                        ui.set_min_height(card_h - 28.0);

                        // Top header + KPIs
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(s.tester_stream_title)
                                    .size(13.0)
                                    .strong()
                                    .color(TEXT_PRI),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let chatter_col = if self.tester_chatter_count > 0 { ROSE } else { MINT };
                                pill_badge(
                                    ui,
                                    &format!("{}: {}", s.tester_chatter, self.tester_chatter_count),
                                    chatter_col,
                                );
                                let min_str = match self.tester_min_dt_ms {
                                    Some(ms) => format!("{}: {}ms", s.tester_min_dt, ms),
                                    None => format!("{}: —", s.tester_min_dt),
                                };
                                let min_col = if self
                                    .tester_min_dt_ms
                                    .map(|m| m < self.tester_threshold_ms)
                                    .unwrap_or(false)
                                {
                                    ROSE
                                } else {
                                    ACCENT
                                };
                                pill_badge(ui, &min_str, min_col);
                                pill_badge(
                                    ui,
                                    &format!("{}: {}", s.tester_total, self.tester_total),
                                    CYAN,
                                );
                            });
                        });

                        ui.add_space(7.0);
                        // Coverage bar
                        let cov_frac = if testable_total > 0 {
                            testable_hit as f32 / testable_total as f32
                        } else {
                            0.0
                        };
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{} {} / {}",
                                    s.tester_coverage, testable_hit, testable_total
                                ))
                                .size(11.0)
                                .color(TEXT_SEC),
                            );
                            let bar_w = (ui.available_width() - 4.0).max(60.0);
                            let (bar_r, _) = ui.allocate_exact_size(Vec2::new(bar_w, 5.0), Sense::hover());
                            ui.painter().rect_filled(bar_r, Rounding::same(3.0), BG_SURFACE);
                            if cov_frac > 0.0 {
                                let fill_r = Rect::from_min_size(
                                    bar_r.min,
                                    Vec2::new((bar_w * cov_frac).max(5.0), bar_r.height()),
                                );
                                ui.painter().rect_filled(fill_r, Rounding::same(3.0), MINT);
                            }
                        });

                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);

                        egui::ScrollArea::vertical()
                            .id_salt("tester_stream_scroll")
                            .max_height(card_h - 108.0)
                            .show(ui, |ui| {
                                if self.tester_log.is_empty() {
                                    ui.add_space(40.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(s.tester_empty_hint)
                                                .size(12.0)
                                                .color(TEXT_DIM),
                                        );
                                    });
                                } else {
                                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                                    ui.horizontal_wrapped(|ui| {
                                        for (idx, ev) in self.tester_log.iter().enumerate() {
                                            draw_event_chip(ui, ev, idx == 0);
                                        }
                                    });
                                }
                            });
                    });
                },
            );

            ui.add_space(col_gap);

            // RIGHT CARD: Per-Key Chatter & Speed Diagnostics Table
            ui.allocate_ui_with_layout(
                Vec2::new(right_w, card_h),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    card_frame().show(ui, |ui| {
                        ui.set_min_width(right_w - 28.0);
                        ui.set_min_height(card_h - 28.0);

                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(s.tester_diag_title)
                                    .size(13.0)
                                    .strong()
                                    .color(TEXT_PRI),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let status_col = if self.tester_chatter_count > 0 { ROSE } else { MINT };
                                let status_lbl = if self.tester_chatter_count > 0 {
                                    s.tester_chatter_warn
                                } else {
                                    s.tester_clean
                                };
                                ui.label(RichText::new(status_lbl).size(11.0).color(status_col));
                            });
                        });

                        ui.add_space(7.0);
                        // Fixed column widths for clean tabular alignment
                        let inner_w = (right_w - 32.0).max(200.0);
                        let c_key = 64.0;
                        let c_hits = 44.0;
                        let c_min = 66.0;
                        let c_hold = 58.0;
                        let c_state = (inner_w - c_key - c_hits - c_min - c_hold).max(52.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.add_sized([c_key, 16.0], egui::Label::new(RichText::new(s.tester_col_key).size(10.5).color(TEXT_DIM)));
                            ui.add_sized([c_hits, 16.0], egui::Label::new(RichText::new(s.tester_col_hits).size(10.5).color(TEXT_DIM)));
                            ui.add_sized([c_min, 16.0], egui::Label::new(RichText::new(s.tester_col_min).size(10.5).color(TEXT_DIM)));
                            ui.add_sized([c_hold, 16.0], egui::Label::new(RichText::new(s.tester_col_hold).size(10.5).color(TEXT_DIM)));
                            ui.add_sized([c_state, 16.0], egui::Label::new(RichText::new(s.tester_col_state).size(10.5).color(TEXT_DIM)));
                        });
                        ui.separator();

                        // Sort tested keys: chatter count DESC -> shortest gap ASC -> hits DESC
                        let mut rows: Vec<DiagRow> = self
                            .tester_stats
                            .iter()
                            .map(|(&hid, st)| {
                                let best_gap = match (st.min_dt_ms, st.min_bounce_ms) {
                                    (Some(a), Some(b)) => Some(a.min(b)),
                                    (Some(a), None) => Some(a),
                                    (None, Some(b)) => Some(b),
                                    (None, None) => None,
                                };
                                DiagRow {
                                    hid,
                                    count: st.count,
                                    best_gap,
                                    hold: st.last_hold_ms,
                                    chatters: st.chatter_count,
                                }
                            })
                            .collect();
                        rows.sort_by(|a, b| {
                            b.chatters
                                .cmp(&a.chatters)
                                .then_with(|| {
                                    a.best_gap
                                        .unwrap_or(u64::MAX)
                                        .cmp(&b.best_gap.unwrap_or(u64::MAX))
                                })
                                .then_with(|| b.count.cmp(&a.count))
                        });

                        egui::ScrollArea::vertical()
                            .id_salt("tester_diag_scroll")
                            .max_height(card_h - 94.0)
                            .show(ui, |ui| {
                                if rows.is_empty() {
                                    ui.add_space(40.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(s.tester_diag_empty)
                                                .size(11.5)
                                                .color(TEXT_DIM),
                                        );
                                    });
                                } else {
                                    ui.spacing_mut().item_spacing.y = 3.0;
                                    for row in rows {
                                        let is_bad = row.chatters > 0
                                            || row
                                                .best_gap
                                                .map(|g| g < self.tester_threshold_ms)
                                                .unwrap_or(false);
                                        let row_bg = if is_bad {
                                            Color32::from_rgba_unmultiplied(ROSE.r(), ROSE.g(), ROSE.b(), 20)
                                        } else {
                                            BG_SURFACE
                                        };
                                        egui::Frame::none()
                                            .fill(row_bg)
                                            .rounding(Rounding::same(5.5))
                                            .inner_margin(egui::Margin::symmetric(0.0, 3.5))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.spacing_mut().item_spacing.x = 0.0;
                                                    let name = hid_to_name(row.hid);
                                                    ui.add_sized(
                                                        [c_key, 17.0],
                                                        egui::Label::new(
                                                            RichText::new(name)
                                                                .size(11.5)
                                                                .strong()
                                                                .color(if is_bad { ROSE } else { TEXT_PRI }),
                                                        ),
                                                    );
                                                    ui.add_sized(
                                                        [c_hits, 17.0],
                                                        egui::Label::new(
                                                            RichText::new(format!("{}", row.count))
                                                                .size(11.0)
                                                                .color(TEXT_SEC),
                                                        ),
                                                    );
                                                    let gap_txt = match row.best_gap {
                                                        Some(g) => format!("{}ms", g),
                                                        None => "—".to_string(),
                                                    };
                                                    ui.add_sized(
                                                        [c_min, 17.0],
                                                        egui::Label::new(
                                                            RichText::new(gap_txt)
                                                                .size(11.0)
                                                                .color(if is_bad { ROSE } else { TEXT_PRI }),
                                                        ),
                                                    );
                                                    let hold_txt = match row.hold {
                                                        Some(h) => format!("{}ms", h),
                                                        None => "—".to_string(),
                                                    };
                                                    ui.add_sized(
                                                        [c_hold, 17.0],
                                                        egui::Label::new(
                                                            RichText::new(hold_txt)
                                                                .size(11.0)
                                                                .color(TEXT_SEC),
                                                        ),
                                                    );
                                                    let state_txt = if row.chatters > 0 {
                                                        format!("{} ×{}", s.tester_state_chatter, row.chatters)
                                                    } else if is_bad {
                                                        s.tester_state_chatter.to_string()
                                                    } else {
                                                        s.tester_state_ok.to_string()
                                                    };
                                                    ui.add_sized(
                                                        [c_state, 17.0],
                                                        egui::Label::new(
                                                            RichText::new(state_txt)
                                                                .size(11.0)
                                                                .strong()
                                                                .color(if is_bad { ROSE } else { MINT }),
                                                        ),
                                                    );
                                                });
                                            });
                                    }
                                }
                            });
                    });
                },
            );
        });
    }

    fn draw_keyboard_twin_tester(&self, ui: &mut egui::Ui) {
        let s = tr(self.lang);
        let (origin, unit) = allocate_keyboard_chassis(ui);
        let p = ui.painter();

        for k in layout_for(self.edition) {
            let rect = Rect::from_min_size(
                Pos2::new(origin.x + k.x * unit + 2.5, origin.y + k.y * unit + 2.5),
                Vec2::new(k.w * unit - 5.0, unit - 5.0),
            );
            let idx = (k.r as usize) * 16 + (k.c as usize);
            let cur_code = self.layers[0][idx];
            let factory_code = factory_default_key_for(self.edition, 0, k.r, k.c);
            let is_fn_internal =
                is_firmware_internal_key(cur_code) && is_firmware_internal_key(factory_code);
            let cur_hid = if cur_code <= 0xFF { cur_code as u8 } else { 0 };
            let fac_hid = if factory_code <= 0xFF {
                factory_code as u8
            } else {
                0
            };
            let match_set = |set: &std::collections::HashSet<u8>| -> bool {
                (cur_hid != 0 && set.contains(&cur_hid)) || (fac_hid != 0 && set.contains(&fac_hid))
            };
            let matched_hid = if cur_hid != 0 && self.tester_stats.contains_key(&cur_hid) {
                Some(cur_hid)
            } else if fac_hid != 0 && self.tester_stats.contains_key(&fac_hid) {
                Some(fac_hid)
            } else {
                None
            };

            let is_pressed = match_set(&self.tester_pressed);
            let is_chatter = match_set(&self.tester_chatter_keys);
            let is_tested = match_set(&self.tester_tested);

            if is_pressed {
                let bleed = Color32::from_rgba_unmultiplied(CYAN.r(), CYAN.g(), CYAN.b(), 115);
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
                Color32::from_rgb(16, 86, 108)
            } else if is_chatter {
                Color32::from_rgb(82, 22, 36)
            } else if is_tested {
                Color32::from_rgb(17, 60, 50)
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
                p.circle_filled(Pos2::new(rect.max.x - 6.0, rect.min.y + 6.0), 2.8, ROSE);
            } else if is_tested {
                p.rect_stroke(rect, Rounding::same(6.5), Stroke::new(1.3_f32, MINT));
            }

            let dec = decode_keycode_for(cur_code, self.lang, self.edition);
            let legend_col = if is_pressed {
                Color32::WHITE
            } else if is_chatter {
                ROSE
            } else if is_tested {
                MINT
            } else if is_fn_internal {
                TEXT_DIM
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

            // Non-focusable hover tooltip showing per-key live stats (or Fn internal explanation)
            let resp = ui.interact(rect, ui.id().with(("tkey", k.r, k.c)), Sense::hover());
            if resp.hovered() {
                resp.on_hover_ui(|ui| {
                    if is_fn_internal {
                        ui.label(RichText::new(dec.label).strong().color(TEXT_PRI));
                        ui.label(RichText::new(s.tester_fn_tooltip).size(11.0).color(TEXT_SEC));
                    } else if let Some(hid) = matched_hid {
                        if let Some(st) = self.tester_stats.get(&hid) {
                            let best_gap = match (st.min_dt_ms, st.min_bounce_ms) {
                                (Some(a), Some(b)) => Some(a.min(b)),
                                (Some(a), None) => Some(a),
                                (None, Some(b)) => Some(b),
                                (None, None) => None,
                            };
                            ui.label(RichText::new(dec.label).strong().color(TEXT_PRI));
                            let gap_s = best_gap
                                .map(|g| format!("{} ms", g))
                                .unwrap_or_else(|| "—".into());
                            let hold_s = st
                                .last_hold_ms
                                .map(|h| format!("{} ms", h))
                                .unwrap_or_else(|| "—".into());
                            ui.label(
                                RichText::new(format!(
                                    "{}: {}  ·  {}: {}  ·  {}: {}",
                                    s.tester_col_hits, st.count, s.tester_col_min, gap_s, s.tester_col_hold, hold_s
                                ))
                                .size(11.0)
                                .color(TEXT_SEC),
                            );
                        }
                    } else {
                        ui.label(RichText::new(dec.label).strong().color(TEXT_PRI));
                    }
                });
            }
        }
    }
}
