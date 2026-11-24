use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Rounding, Stroke, Vec2};
use std::path::PathBuf;

// --- Wootility x Keychron Industrial Dark Palette ---
pub const BG_CANVAS: Color32 = Color32::from_rgb(10, 12, 16);
pub const BG_RAIL: Color32 = Color32::from_rgb(14, 17, 23);
pub const BG_CARD: Color32 = Color32::from_rgb(19, 23, 32);
pub const BG_SURFACE: Color32 = Color32::from_rgb(26, 31, 43);
pub const BG_SURFACE_HOV: Color32 = Color32::from_rgb(35, 42, 58);
pub const HAIRLINE: Color32 = Color32::from_rgb(36, 43, 58);
pub const TEXT_PRI: Color32 = Color32::from_rgb(241, 245, 249);
pub const TEXT_SEC: Color32 = Color32::from_rgb(148, 163, 184);
pub const TEXT_DIM: Color32 = Color32::from_rgb(92, 104, 124);

pub const ACCENT: Color32 = Color32::from_rgb(245, 158, 11);
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(45, 30, 8, 255);
pub const CYAN: Color32 = Color32::from_rgb(56, 189, 248);
pub const MINT: Color32 = Color32::from_rgb(52, 211, 153);
pub const VIOLET: Color32 = Color32::from_rgb(167, 139, 250);
pub const ROSE: Color32 = Color32::from_rgb(251, 113, 133);

/// Exact 8 quick-pick indices into OEM firmware 45-color spectrum (`0x1C388`).
pub const LOGO_SIDE_PALETTE: [(u8, Color32); 8] = [
    (0, Color32::from_rgb(255, 20, 20)),
    (3, Color32::from_rgb(255, 110, 0)),
    (6, Color32::from_rgb(255, 235, 0)),
    (14, Color32::from_rgb(34, 211, 94)),
    (21, CYAN),
    (29, Color32::from_rgb(59, 130, 246)),
    (34, VIOLET),
    (38, Color32::from_rgb(244, 63, 180)),
];

pub fn blend_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let inv = 1.0 - t;
    Color32::from_rgb(
        (a.r() as f32 * inv + b.r() as f32 * t) as u8,
        (a.g() as f32 * inv + b.g() as f32 * t) as u8,
        (a.b() as f32 * inv + b.b() as f32 * t) as u8,
    )
}

pub fn hsv_to_color32(hue: u8, sat: u8) -> Color32 {
    let h = (hue as f32 / 255.0) * 6.0;
    let s = sat as f32 / 255.0;
    let v = 0.94f32;
    let c = v * s;
    let x = c * (1.0 - ((h % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color32::from_rgb(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8)
}

pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let windir = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let fonts_dir = windir.join("Fonts");

    if let Ok(bytes) = std::fs::read(fonts_dir.join("msyh.ttc")) {
        fonts.font_data.insert("zh".into(), FontData::from_owned(bytes));
        fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "zh".into());
        fonts.families.entry(FontFamily::Monospace).or_default().push("zh".into());
    }
    for jp in ["YuGothM.ttc", "meiryo.ttc"] {
        if let Ok(bytes) = std::fs::read(fonts_dir.join(jp)) {
            fonts.font_data.insert("ja".into(), FontData::from_owned(bytes));
            fonts.families.entry(FontFamily::Proportional).or_default().push("ja".into());
            break;
        }
    }
    ctx.set_fonts(fonts);
}

pub fn build_app_icon() -> egui::IconData {
    egui::IconData {
        rgba: include_bytes!("../../assets/icon_64.rgba").to_vec(),
        width: 64,
        height: 64,
    }
}

pub fn setup_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG_CANVAS;
    v.window_fill = BG_CARD;
    v.faint_bg_color = BG_SURFACE;
    v.extreme_bg_color = Color32::from_rgb(13, 16, 23);
    v.override_text_color = Some(TEXT_PRI);
    v.window_rounding = Rounding::same(12.0);
    v.window_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.menu_rounding = Rounding::same(8.0);
    v.widgets.noninteractive.bg_fill = BG_CARD;
    v.widgets.noninteractive.weak_bg_fill = BG_CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_SEC);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.widgets.inactive.bg_fill = BG_SURFACE;
    v.widgets.inactive.weak_bg_fill = BG_SURFACE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRI);
    v.widgets.inactive.rounding = Rounding::same(7.0);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
    v.widgets.hovered.bg_fill = BG_SURFACE_HOV;
    v.widgets.hovered.weak_bg_fill = BG_SURFACE_HOV;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRI);
    v.widgets.hovered.rounding = Rounding::same(7.0);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.bg_fill = ACCENT_SOFT;
    v.widgets.active.weak_bg_fill = ACCENT_SOFT;
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.active.rounding = Rounding::same(7.0);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.bg_fill = BG_SURFACE_HOV;
    v.widgets.open.weak_bg_fill = BG_SURFACE_HOV;
    v.widgets.open.fg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.open.rounding = Rounding::same(7.0);
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    v.selection.bg_fill = ACCENT_SOFT;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);

    style.visuals = v;
    style.spacing.item_spacing = Vec2::new(10.0, 7.0);
    style.spacing.button_padding = Vec2::new(12.0, 5.5);
    style.spacing.slider_width = 240.0;
    style.spacing.scroll = egui::style::ScrollStyle::floating();
    ctx.set_theme(egui::Theme::Dark);
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style(style);
}
