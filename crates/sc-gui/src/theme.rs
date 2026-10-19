//! Visual design: a quiet, Apple-like system. Two palettes (light and dark,
//! following the OS), one accent, generous spacing, rounded surfaces, hairline
//! borders, and the system's own typeface and icon font where available.

use egui::{Color32, FontFamily, FontId, Rounding, Stroke, TextStyle};

/// Every colour the UI uses. Fetch with [`pal`]; nothing else hard-codes colours.
#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Window background.
    pub bg: Color32,
    /// Sidebar background.
    pub sidebar: Color32,
    /// Raised surfaces: cards, grouped lists.
    pub card: Color32,
    /// A step above `card`: hovered rows, inset fields.
    pub card_hi: Color32,
    /// Hairline borders and dividers.
    pub line: Color32,
    pub text: Color32,
    pub text_2: Color32,
    pub text_3: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub red: Color32,
    pub orange: Color32,
}

const DARK: Palette = Palette {
    dark: true,
    bg: Color32::from_rgb(0x1c, 0x1c, 0x1e),
    sidebar: Color32::from_rgb(0x25, 0x25, 0x28),
    card: Color32::from_rgb(0x2c, 0x2c, 0x2e),
    card_hi: Color32::from_rgb(0x3a, 0x3a, 0x3c),
    line: Color32::from_rgb(0x3d, 0x3d, 0x40),
    text: Color32::from_rgb(0xf5, 0xf5, 0xf7),
    text_2: Color32::from_rgb(0x9a, 0x9a, 0xa0),
    text_3: Color32::from_rgb(0x6c, 0x6c, 0x72),
    accent: Color32::from_rgb(0x0a, 0x84, 0xff),
    green: Color32::from_rgb(0x30, 0xd1, 0x58),
    red: Color32::from_rgb(0xff, 0x45, 0x3a),
    orange: Color32::from_rgb(0xff, 0x9f, 0x0a),
};

const LIGHT: Palette = Palette {
    dark: false,
    bg: Color32::from_rgb(0xf5, 0xf5, 0xf7),
    sidebar: Color32::from_rgb(0xe9, 0xe9, 0xed),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    card_hi: Color32::from_rgb(0xf0, 0xf0, 0xf3),
    line: Color32::from_rgb(0xdc, 0xdc, 0xe1),
    text: Color32::from_rgb(0x1d, 0x1d, 0x1f),
    text_2: Color32::from_rgb(0x6e, 0x6e, 0x73),
    text_3: Color32::from_rgb(0xa1, 0xa1, 0xa6),
    accent: Color32::from_rgb(0x00, 0x7a, 0xff),
    green: Color32::from_rgb(0x34, 0xc7, 0x59),
    red: Color32::from_rgb(0xff, 0x3b, 0x30),
    orange: Color32::from_rgb(0xff, 0x95, 0x00),
};

/// The palette for the current light/dark mode.
pub fn pal(ctx: &egui::Context) -> Palette {
    if ctx.style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

pub fn pal_ui(ui: &egui::Ui) -> Palette {
    pal(ui.ctx())
}

/// Font family for semibold text (titles, emphasis); falls back to the regular one.
pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Font family for the icon glyphs (Segoe Fluent Icons / MDL2 on Windows).
pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

/// Icon glyphs from Segoe Fluent Icons / Segoe MDL2 Assets.
pub mod icon {
    pub const HOME: &str = "\u{E80F}";
    pub const CLIPS: &str = "\u{E714}";
    pub const SETTINGS: &str = "\u{E713}";
    pub const PLAY: &str = "\u{E768}";
    pub const PAUSE: &str = "\u{E769}";
    pub const CAMERA: &str = "\u{E722}";
    pub const DELETE: &str = "\u{E74D}";
    pub const FOLDER: &str = "\u{E8B7}";
    pub const CUT: &str = "\u{E8C6}";
    pub const SAVE: &str = "\u{E74E}";
    pub const SEARCH: &str = "\u{E721}";
    pub const REFRESH: &str = "\u{E72C}";
    pub const BACK: &str = "\u{E72B}";
}

/// Load the system UI font, a semibold weight and the icon font. Anything that
/// isn't installed is skipped and egui's built-in fonts are used instead.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into());
    let load = |file: &str| std::fs::read(format!("{dir}\\Fonts\\{file}")).ok();

    if let Some(bytes) = load("segoeui.ttf") {
        fonts
            .font_data
            .insert("ui".into(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "ui".into());
    }
    let semi = load("seguisb.ttf").map(|b| ("ui-semibold", b));
    if let Some((name, bytes)) = semi {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_owned(bytes));
        let mut family = vec![name.to_string()];
        family.extend(fonts.families[&FontFamily::Proportional].clone());
        fonts.families.insert(semibold(), family);
    } else {
        let fallback = fonts.families[&FontFamily::Proportional].clone();
        fonts.families.insert(semibold(), fallback);
    }
    let icon_bytes = load("SegoeIcons.ttf").or_else(|| load("segmdl2.ttf"));
    let has_icons = icon_bytes.is_some();
    if let Some(bytes) = icon_bytes {
        fonts
            .font_data
            .insert("icons".into(), egui::FontData::from_owned(bytes));
        fonts.families.insert(icons(), vec!["icons".into()]);
    } else {
        fonts.families.insert(icons(), vec![]);
    }
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_persisted(egui::Id::new("has_icons"), has_icons));
}

/// Whether the icon font loaded (otherwise icon buttons show their label).
pub fn has_icons(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.get_persisted::<bool>(egui::Id::new("has_icons")))
        .unwrap_or(false)
}

/// Set up fonts, text sizes and the light/dark visuals. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.style_mut_of(egui::Theme::Dark, |s| style(s, DARK));
    ctx.style_mut_of(egui::Theme::Light, |s| style(s, LIGHT));
}

fn style(s: &mut egui::Style, p: Palette) {
    s.text_styles = [
        (TextStyle::Heading, FontId::new(26.0, semibold())),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();

    s.spacing.item_spacing = egui::vec2(10.0, 10.0);
    s.spacing.button_padding = egui::vec2(14.0, 7.0);
    s.spacing.interact_size.y = 30.0;
    s.spacing.slider_width = 180.0;
    s.spacing.scroll.bar_width = 8.0;
    s.spacing.scroll.floating = true;

    let v = &mut s.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.extreme_bg_color = p.card_hi;
    v.faint_bg_color = p.card;
    v.window_stroke = Stroke::new(1.0_f32, p.line);
    v.window_rounding = Rounding::same(14.0);
    v.selection.bg_fill = p.accent.linear_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.hyperlink_color = p.accent;

    let r = Rounding::same(8.0);
    for (w, fill) in [
        (&mut v.widgets.inactive, p.card_hi),
        (&mut v.widgets.hovered, p.card_hi),
        (&mut v.widgets.active, p.card_hi),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.rounding = r;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.0_f32, p.text);
    }
    v.widgets.hovered.weak_bg_fill = mix(p.card_hi, p.text, 0.06);
    v.widgets.hovered.bg_fill = v.widgets.hovered.weak_bg_fill;
    v.widgets.active.weak_bg_fill = mix(p.card_hi, p.text, 0.12);
    v.widgets.active.bg_fill = v.widgets.active.weak_bg_fill;
    v.widgets.noninteractive.bg_fill = p.card;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_2);
    v.widgets.noninteractive.rounding = r;
    v.widgets.open.rounding = r;
}

/// Blend `b` over `a` by `t` (0..1).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}
