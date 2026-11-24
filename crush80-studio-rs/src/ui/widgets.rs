use crate::ui::theme::*;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Rounding, Sense, Stroke, Vec2};

pub fn nav_rail_button(ui: &mut egui::Ui, label: &str, icon_id: u8, active: bool) -> bool {
    let size = Vec2::new(ui.available_width(), 33.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();

    let bg = if active {
        ACCENT_SOFT
    } else if resp.hovered() {
        BG_SURFACE
    } else {
        Color32::TRANSPARENT
    };
    p.rect_filled(rect, Rounding::same(8.0), bg);
    if active {
        let bar = Rect::from_min_size(
            rect.min + Vec2::new(0.0, 7.0),
            Vec2::new(3.0, rect.height() - 14.0),
        );
        p.rect_filled(bar, Rounding::same(2.0), ACCENT);
    }

    let col = if active {
        ACCENT
    } else if resp.hovered() {
        TEXT_PRI
    } else {
        TEXT_SEC
    };
    let ic = Pos2::new(rect.min.x + 20.0, rect.center().y);
    match icon_id {
        0 => {
            let r = Rect::from_center_size(ic, Vec2::new(14.0, 9.5));
            p.rect_stroke(r, Rounding::same(2.0), Stroke::new(1.4_f32, col));
            p.line_segment(
                [Pos2::new(ic.x - 4.0, ic.y + 1.5), Pos2::new(ic.x + 4.0, ic.y + 1.5)],
                Stroke::new(1.4_f32, col),
            );
        }
        1 => {
            p.circle_stroke(ic, 4.2, Stroke::new(1.5_f32, col));
            p.circle_filled(ic, 1.8, col);
        }
        2 => {
            p.line_segment(
                [Pos2::new(ic.x + 2.0, ic.y - 6.0), Pos2::new(ic.x - 3.0, ic.y + 0.5)],
                Stroke::new(1.6_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x - 3.0, ic.y + 0.5), Pos2::new(ic.x + 2.5, ic.y + 0.5)],
                Stroke::new(1.6_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x + 2.5, ic.y + 0.5), Pos2::new(ic.x - 2.0, ic.y + 6.0)],
                Stroke::new(1.6_f32, col),
            );
        }
        3 => {
            let r1 = Rect::from_center_size(Pos2::new(ic.x, ic.y - 2.5), Vec2::new(12.0, 5.0));
            p.rect_stroke(r1, Rounding::same(1.5), Stroke::new(1.4_f32, col));
            p.line_segment(
                [Pos2::new(ic.x - 5.5, ic.y + 3.5), Pos2::new(ic.x + 5.5, ic.y + 3.5)],
                Stroke::new(1.4_f32, col),
            );
        }
        5 => {
            p.line_segment(
                [Pos2::new(ic.x - 6.0, ic.y - 3.0), Pos2::new(ic.x + 6.0, ic.y - 3.0)],
                Stroke::new(1.4_f32, col),
            );
            p.circle_filled(Pos2::new(ic.x - 2.0, ic.y - 3.0), 2.3, col);
            p.line_segment(
                [Pos2::new(ic.x - 6.0, ic.y + 3.0), Pos2::new(ic.x + 6.0, ic.y + 3.0)],
                Stroke::new(1.4_f32, col),
            );
            p.circle_filled(Pos2::new(ic.x + 2.5, ic.y + 3.0), 2.3, col);
        }
        6 => {
            p.line_segment(
                [Pos2::new(ic.x - 6.5, ic.y + 0.5), Pos2::new(ic.x - 2.5, ic.y + 0.5)],
                Stroke::new(1.4_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x - 2.5, ic.y + 0.5), Pos2::new(ic.x - 0.5, ic.y - 5.0)],
                Stroke::new(1.4_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x - 0.5, ic.y - 5.0), Pos2::new(ic.x + 2.0, ic.y + 4.5)],
                Stroke::new(1.4_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x + 2.0, ic.y + 4.5), Pos2::new(ic.x + 4.0, ic.y + 0.5)],
                Stroke::new(1.4_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x + 4.0, ic.y + 0.5), Pos2::new(ic.x + 6.5, ic.y + 0.5)],
                Stroke::new(1.4_f32, col),
            );
        }
        7 => {
            p.line_segment(
                [Pos2::new(ic.x, ic.y + 4.0), Pos2::new(ic.x, ic.y - 4.5)],
                Stroke::new(1.5_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x - 3.5, ic.y - 1.5), Pos2::new(ic.x, ic.y - 5.0)],
                Stroke::new(1.5_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x + 3.5, ic.y - 1.5), Pos2::new(ic.x, ic.y - 5.0)],
                Stroke::new(1.5_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x - 5.5, ic.y + 5.5), Pos2::new(ic.x + 5.5, ic.y + 5.5)],
                Stroke::new(1.4_f32, col),
            );
        }
        _ => {
            p.circle_stroke(ic, 5.5, Stroke::new(1.4_f32, col));
            p.line_segment(
                [Pos2::new(ic.x - 2.5, ic.y), Pos2::new(ic.x + 2.5, ic.y)],
                Stroke::new(1.4_f32, col),
            );
            p.line_segment(
                [Pos2::new(ic.x, ic.y - 2.5), Pos2::new(ic.x, ic.y + 2.5)],
                Stroke::new(1.4_f32, col),
            );
        }
    }

    p.text(
        Pos2::new(rect.min.x + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.0),
        if active { TEXT_PRI } else { col },
    );
    resp.clicked()
}

pub fn toggle_switch(ui: &mut egui::Ui, on: &mut bool) -> bool {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(38.0, 20.0), Sense::click());
    if ui.is_enabled() && resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let bg = if !ui.is_enabled() {
        Color32::from_rgb(30, 36, 48)
    } else if *on {
        ACCENT
    } else {
        Color32::from_rgb(44, 52, 68)
    };
    p.rect_filled(rect, Rounding::same(10.0), bg);
    let cx = if *on {
        rect.max.x - 10.0
    } else {
        rect.min.x + 10.0
    };
    let knob = if ui.is_enabled() { Color32::WHITE } else { TEXT_DIM };
    p.circle_filled(Pos2::new(cx, rect.center().y), 7.2, knob);
    resp.changed()
}

pub fn primary_button(ui: &mut egui::Ui, label: &str, min_w: f32) -> bool {
    let b = egui::Button::new(RichText::new(label).size(12.0).strong().color(BG_CANVAS))
        .fill(ACCENT)
        .stroke(Stroke::NONE)
        .rounding(Rounding::same(7.0))
        .min_size(Vec2::new(min_w, 28.0));
    ui.add(b).clicked()
}

pub fn chip_button(ui: &mut egui::Ui, label: &str, active: bool) -> bool {
    let b = egui::Button::new(
        RichText::new(label)
            .size(11.5)
            .color(if active { ACCENT } else { TEXT_PRI }),
    )
    .fill(if active { ACCENT_SOFT } else { BG_SURFACE })
    .stroke(Stroke::new(1.0_f32, if active { ACCENT } else { HAIRLINE }))
    .rounding(Rounding::same(6.5))
    .min_size(Vec2::new(0.0, 26.0));
    ui.add(b).clicked()
}

pub fn danger_button(ui: &mut egui::Ui, label: &str) -> bool {
    let b = egui::Button::new(RichText::new(label).size(11.5).color(ROSE))
        .fill(Color32::from_rgba_unmultiplied(251, 113, 133, 20))
        .stroke(Stroke::new(
            1.0_f32,
            Color32::from_rgba_unmultiplied(251, 113, 133, 80),
        ))
        .rounding(Rounding::same(6.5))
        .min_size(Vec2::new(0.0, 26.0));
    ui.add(b).clicked()
}

pub fn pill_badge(ui: &mut egui::Ui, text: &str, col: Color32) {
    let bg = Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 26);
    egui::Frame::none()
        .fill(bg)
        .rounding(Rounding::same(99.0))
        .inner_margin(egui::Margin::symmetric(9.0, 3.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.0).strong().color(col));
        });
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
        .rounding(Rounding::same(12.0))
        .inner_margin(egui::Margin::same(13.0))
}

pub fn titlebar_win_btn(ui: &mut egui::Ui, kind: u8) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(38.0, 28.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        let bg = if kind == 2 {
            Color32::from_rgb(220, 38, 38)
        } else {
            BG_SURFACE_HOV
        };
        p.rect_filled(rect, Rounding::same(6.0), bg);
    }
    let col = if resp.hovered() && kind == 2 {
        Color32::WHITE
    } else if resp.hovered() {
        TEXT_PRI
    } else {
        TEXT_SEC
    };
    let c = rect.center();
    match kind {
        0 => {
            p.line_segment(
                [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)],
                Stroke::new(1.4_f32, col),
            );
        }
        1 => {
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(9.0)),
                Rounding::same(1.5),
                Stroke::new(1.3_f32, col),
            );
        }
        _ => {
            p.line_segment(
                [Pos2::new(c.x - 4.5, c.y - 4.5), Pos2::new(c.x + 4.5, c.y + 4.5)],
                Stroke::new(1.5_f32, col),
            );
            p.line_segment(
                [Pos2::new(c.x + 4.5, c.y - 4.5), Pos2::new(c.x - 4.5, c.y + 4.5)],
                Stroke::new(1.5_f32, col),
            );
        }
    }
    resp.clicked()
}

pub fn mod_label(m: u8) -> &'static str {
    match m {
        1 => "LCtrl",
        2 => "LShift",
        4 => "LAlt",
        8 => "LWin",
        3 => "Ctrl+Shift",
        15 => "Hyper",
        _ => "Mod",
    }
}

/// Shared keyboard chassis + plate allocation used by Keymap, RGB, and Key Tester.
pub fn allocate_keyboard_chassis(ui: &mut egui::Ui) -> (Pos2, f32) {
    let avail_w = ui.available_width().max(640.0);
    let pad_x = 18.0;
    let pad_y = 14.0;
    let unit = ((avail_w - pad_x * 2.0) / 18.25).clamp(32.0, 47.0);
    let board_w = unit * 18.25 + pad_x * 2.0;
    let board_h = unit * 6.25 + pad_y * 2.0;

    let (chassis, _) = ui.allocate_exact_size(Vec2::new(board_w, board_h), Sense::hover());
    let p = ui.painter();
    p.rect_filled(chassis, Rounding::same(16.0), Color32::from_rgb(20, 24, 33));
    p.rect_stroke(
        chassis,
        Rounding::same(16.0),
        Stroke::new(1.5_f32, Color32::from_rgb(50, 58, 76)),
    );
    let plate = chassis.shrink2(Vec2::new(pad_x - 8.0, pad_y - 8.0));
    p.rect_filled(plate, Rounding::same(10.0), Color32::from_rgb(10, 12, 17));
    (chassis.min + Vec2::new(pad_x, pad_y), unit)
}

/// Shared 8-color dot selector for Logo Badge & Side Underglow (`CONFIG[34]` / `CONFIG[38]`).
pub fn render_spectrum_dots(ui: &mut egui::Ui, current: u8) -> Option<u8> {
    let mut picked = None;
    for (idx, col) in LOGO_SIDE_PALETTE {
        let (r, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
        ui.painter().circle_filled(r.center(), 7.5, col);
        if current == idx {
            ui.painter()
                .circle_stroke(r.center(), 9.2, Stroke::new(1.6_f32, ACCENT));
        }
        if ui.is_enabled() && resp.clicked() {
            picked = Some(idx);
        }
    }
    picked
}
