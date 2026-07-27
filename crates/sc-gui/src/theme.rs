use egui::{Color32, Rounding, Stroke, Visuals};

pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x9b, 0xff);
pub const OK: Color32 = Color32::from_rgb(0x36, 0xc7, 0x6a);
pub const WARN: Color32 = Color32::from_rgb(0xf5, 0xa8, 0x23);
pub const DANGER: Color32 = Color32::from_rgb(0xe0, 0x53, 0x53);
pub const MUTED: Color32 = Color32::from_rgb(0x8a, 0x90, 0x9c);

const BG: Color32 = Color32::from_rgb(0x14, 0x16, 0x1b);
const PANEL: Color32 = Color32::from_rgb(0x1b, 0x1e, 0x25);
const CARD: Color32 = Color32::from_rgb(0x22, 0x26, 0x2f);

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.override_text_color = Some(Color32::from_rgb(0xe6, 0xe8, 0xed));
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = CARD;
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x2c, 0x31, 0x3c);
    v.widgets.active.bg_fill = ACCENT;
    v.selection.bg_fill = ACCENT.linear_multiply(0.4);
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.widgets.inactive.rounding = Rounding::same(6.0);
    v.widgets.hovered.rounding = Rounding::same(6.0);
    v.widgets.active.rounding = Rounding::same(6.0);
    ctx.set_visuals(v);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    ctx.set_style(style);
}
