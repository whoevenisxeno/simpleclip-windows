//! Reusable controls in the app's visual language: grouped lists, switches,
//! segmented controls, buttons, key caps. Everything draws from the palette.

use crate::theme::{self, mix, pal_ui};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, Response, RichText, Rounding, Sense,
    Stroke, Ui,
};

// ------------------------------------------------------------------ text --

/// A page's large title with an optional quiet subtitle underneath.
pub fn page_title(ui: &mut Ui, title: &str, subtitle: Option<&str>) {
    let p = pal_ui(ui);
    ui.add_space(6.0);
    ui.label(
        RichText::new(title)
            .font(FontId::new(30.0, theme::semibold()))
            .color(p.text),
    );
    if let Some(s) = subtitle {
        ui.add_space(-4.0);
        ui.label(RichText::new(s).size(14.0).color(p.text_2));
    }
    ui.add_space(14.0);
}

/// Small grey section label ("CAPTURE") above a group.
pub fn caption(ui: &mut Ui, text: &str) {
    let p = pal_ui(ui);
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.5)
            .font(FontId::new(11.5, theme::semibold()))
            .color(p.text_2),
    );
    ui.add_space(-2.0);
}

pub fn secondary(ui: &mut Ui, text: impl Into<String>) {
    let p = pal_ui(ui);
    ui.label(RichText::new(text.into()).size(13.0).color(p.text_2));
}

// --------------------------------------------------------------- surfaces --

/// A raised, rounded surface.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card)
        .rounding(Rounding::same(14.0))
        .stroke(Stroke::new(1.0_f32, p.line))
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// An inset grouped list (like iOS/macOS Settings): rows separated by hairlines.
pub struct Section<'a> {
    ui: &'a mut Ui,
    first: bool,
}

pub fn section<R>(ui: &mut Ui, header: Option<&str>, add: impl FnOnce(&mut Section) -> R) -> R {
    if let Some(h) = header {
        caption(ui, h);
    }
    let p = pal_ui(ui);
    let out = egui::Frame::none()
        .fill(p.card)
        .rounding(Rounding::same(12.0))
        .stroke(Stroke::new(1.0_f32, p.line))
        .inner_margin(egui::Margin::symmetric(16.0, 0.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut s = Section { ui, first: true };
            add(&mut s)
        })
        .inner;
    ui.add_space(8.0);
    out
}

impl Section<'_> {
    fn divider(&mut self) {
        if !self.first {
            let p = pal_ui(self.ui);
            let w = self.ui.available_width();
            let (r, _) = self.ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
            self.ui.painter().rect_filled(r, 0.0, p.line);
        }
        self.first = false;
    }

    /// A row: label (and optional hint) on the left, a control on the right.
    pub fn row(&mut self, label: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
        self.divider();
        let p = pal_ui(self.ui);
        let h = if hint.is_some() { 58.0 } else { 48.0 };
        let w = self.ui.available_width();
        self.ui
            .allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // Centre the text block in the row (the vertical fills its height).
                    let text_h = if hint.is_some() { 36.0 } else { 20.0 };
                    ui.add_space(((h - text_h) / 2.0).max(0.0));
                    ui.label(RichText::new(label).size(14.5).color(p.text));
                    if let Some(h) = hint {
                        ui.label(RichText::new(h).size(12.0).color(p.text_2));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| control(ui));
            });
    }

    /// A full-width row for free-form content (previews, notes).
    pub fn full(&mut self, add: impl FnOnce(&mut Ui)) {
        self.divider();
        self.ui.add_space(10.0);
        add(self.ui);
        self.ui.add_space(10.0);
    }
}

// --------------------------------------------------------------- controls --

/// iOS-style on/off switch.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = pal_ui(ui);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(44.0, 26.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.14);
    let off_col = if p.dark {
        Color32::from_rgb(0x63, 0x63, 0x66)
    } else {
        Color32::from_rgb(0xc7, 0xc7, 0xcc)
    };
    let track = mix(off_col, p.green, t);
    ui.painter().rect_filled(rect, Rounding::same(13.0), track);
    let knob_x = egui::lerp((rect.left() + 13.0)..=(rect.right() - 13.0), t);
    ui.painter().circle_filled(
        pos2(knob_x, rect.center().y + 0.5),
        10.0,
        Color32::from_black_alpha(40),
    );
    ui.painter()
        .circle_filled(pos2(knob_x, rect.center().y), 10.0, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Pill-shaped segmented control. Returns true when the selection changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let p = pal_ui(ui);
    let font = FontId::new(13.0, egui::FontFamily::Proportional);
    let mut changed = false;
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap((*l).to_string(), font.clone(), p.text)
                .size()
                .x
                + 26.0
        })
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 30.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(9.0), p.card_hi);
    let mut x = rect.left() + 2.0;
    for ((val, label), w) in options.iter().zip(widths) {
        let seg = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, 26.0));
        let resp = ui.interact(seg, ui.id().with(label), Sense::click());
        let selected = *val == *value;
        if selected {
            let fill = if p.dark {
                Color32::from_rgb(0x63, 0x63, 0x66)
            } else {
                Color32::WHITE
            };
            ui.painter().rect_filled(
                seg.translate(vec2(0.0, 0.5)),
                Rounding::same(7.0),
                Color32::from_black_alpha(if p.dark { 60 } else { 22 }),
            );
            ui.painter().rect_filled(seg, Rounding::same(7.0), fill);
        }
        ui.painter().text(
            seg.center(),
            Align2::CENTER_CENTER,
            label,
            font.clone(),
            if selected { p.text } else { p.text_2 },
        );
        if resp.clicked() && !selected {
            *value = *val;
            changed = true;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        x += w;
    }
    changed
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Primary,
    Quiet,
}

/// A rounded button with an optional leading icon.
pub fn button(ui: &mut Ui, tone: Tone, glyph: Option<&str>, label: &str) -> Response {
    let p = pal_ui(ui);
    let font = FontId::new(14.0, egui::FontFamily::Proportional);
    let icon_font = FontId::new(15.0, theme::icons());
    let use_icon = glyph.is_some() && theme::has_icons(ui.ctx());
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), p.text)
        .size()
        .x;
    let w = text_w + 28.0 + if use_icon { 24.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(40.0), 34.0), Sense::click());
    let (bg, fg) = match tone {
        Tone::Primary => (p.accent, Color32::WHITE),
        Tone::Quiet => (p.card_hi, p.text),
    };
    let bg = if resp.is_pointer_button_down_on() {
        mix(bg, Color32::BLACK, 0.18)
    } else if resp.hovered() {
        mix(
            bg,
            if p.dark {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
            0.08,
        )
    } else {
        bg
    };
    ui.painter().rect_filled(rect, Rounding::same(9.0), bg);
    let mut x = rect.left() + 14.0;
    if use_icon {
        ui.painter().text(
            pos2(x + 8.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph.unwrap_or(""),
            icon_font,
            fg,
        );
        x += 24.0;
    }
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn primary_button(ui: &mut Ui, glyph: Option<&str>, label: &str) -> Response {
    button(ui, Tone::Primary, glyph, label)
}

pub fn quiet_button(ui: &mut Ui, glyph: Option<&str>, label: &str) -> Response {
    button(ui, Tone::Quiet, glyph, label)
}

/// A square icon-only button (falls back to a text label without the icon font).
pub fn icon_button(ui: &mut Ui, glyph: &str, fallback: &str, tip: &str, danger: bool) -> Response {
    let p = pal_ui(ui);
    let has = theme::has_icons(ui.ctx());
    let size = if has {
        vec2(30.0, 30.0)
    } else {
        vec2(54.0, 28.0)
    };
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let bg = if resp.hovered() {
        mix(
            p.card_hi,
            if p.dark {
                Color32::WHITE
            } else {
                Color32::BLACK
            },
            0.08,
        )
    } else {
        p.card_hi
    };
    ui.painter().rect_filled(rect, Rounding::same(8.0), bg);
    let fg = if danger { p.red } else { p.text };
    if has {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(14.0, theme::icons()),
            fg,
        );
    } else {
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            fallback,
            FontId::new(12.0, egui::FontFamily::Proportional),
            fg,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(tip)
}

/// A filled dot, used for status.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

/// "ALT+F10" drawn as little key caps.
pub fn keycaps(ui: &mut Ui, combo: &str) {
    ui.spacing_mut().item_spacing.x = 5.0;
    for part in combo.split('+').filter(|s| !s.is_empty()) {
        keycap(ui, part);
    }
}

/// Same, for use inside a right-to-left layout.
pub fn keycaps_rtl(ui: &mut Ui, combo: &str) {
    ui.spacing_mut().item_spacing.x = 5.0;
    for part in combo.split('+').filter(|s| !s.is_empty()).rev() {
        keycap(ui, part);
    }
}

fn keycap(ui: &mut Ui, part: &str) {
    let p = pal_ui(ui);
    {
        let label = pretty_key(part);
        let font = FontId::new(12.5, theme::semibold());
        let galley = ui.painter().layout_no_wrap(label, font, p.text);
        let size = vec2((galley.size().x + 16.0).max(28.0), 26.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let r = Rounding::same(6.0);
        ui.painter()
            .rect_filled(rect.translate(vec2(0.0, 1.5)), r, p.line);
        ui.painter().rect_filled(rect, r, p.card_hi);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, p.text);
    }
}

fn pretty_key(k: &str) -> String {
    match k.to_ascii_uppercase().as_str() {
        "CTRL" | "CONTROL" => "Ctrl".into(),
        "ALT" => "Alt".into(),
        "SHIFT" => "Shift".into(),
        "SUPER" | "WIN" | "META" => "Win".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                Some(f) if other.len() > 3 => {
                    f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
                }
                _ => other.to_string(),
            }
        }
    }
}

/// A hotkey as key caps plus a "Change" button that listens for the next combination.
pub fn hotkey_field(ui: &mut Ui, value: &mut String, id: &str) -> bool {
    let p = pal_ui(ui);
    let rec_id = ui.make_persistent_id(id);
    let recording = ui.data(|d| d.get_temp::<bool>(rec_id)).unwrap_or(false);
    let mut changed = false;
    // Right-to-left parent: add the button first so it sits at the far right.
    if recording {
        let cancel = quiet_button(ui, None, "Cancel");
        if cancel.clicked() {
            ui.data_mut(|d| d.insert_temp(rec_id, false));
        }
        ui.label(RichText::new("Press the new shortcut…").color(p.orange));
        for ev in ui.input(|i| i.events.clone()) {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = ev
            else {
                continue;
            };
            if key != egui::Key::Escape {
                let mut parts = Vec::new();
                if modifiers.ctrl {
                    parts.push("CTRL".to_string());
                }
                if modifiers.alt {
                    parts.push("ALT".to_string());
                }
                if modifiers.shift {
                    parts.push("SHIFT".to_string());
                }
                parts.push(key.name().to_ascii_uppercase());
                *value = parts.join("+");
                changed = true;
            }
            ui.data_mut(|d| d.insert_temp(rec_id, false));
        }
        ui.ctx().request_repaint();
    } else {
        if quiet_button(ui, None, "Change").clicked() {
            ui.data_mut(|d| d.insert_temp(rec_id, true));
        }
        // We're inside a right-to-left row: add the caps last-key-first so they
        // read left to right, sitting just left of the button.
        keycaps_rtl(ui, value);
    }
    changed
}

/// A tinted notice bar (success / warning / error).
pub fn notice(ui: &mut Ui, color: Color32, text: &str) {
    egui::Frame::none()
        .fill(color.linear_multiply(0.14))
        .rounding(Rounding::same(10.0))
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(color).size(13.5));
        });
}

/// Draw a vertical-centred row icon + label inside a rect (sidebar items).
pub fn nav_item(ui: &mut Ui, glyph: &str, label: &str, selected: bool) -> Response {
    let p = pal_ui(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 36.0), Sense::click());
    if selected {
        ui.painter()
            .rect_filled(rect, Rounding::same(8.0), p.accent.linear_multiply(0.18));
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, Rounding::same(8.0), p.text.linear_multiply(0.06));
    }
    let fg = if selected { p.accent } else { p.text };
    let mut x = rect.left() + 12.0;
    if theme::has_icons(ui.ctx()) {
        ui.painter().text(
            pos2(x + 9.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(16.0, theme::icons()),
            fg,
        );
        x += 28.0;
    }
    ui.painter().text(
        pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::new(
            14.5,
            if selected {
                theme::semibold()
            } else {
                egui::FontFamily::Proportional
            },
        ),
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Two-handle range selector over `0..=max` seconds. Returns true if either
/// handle moved. Keeps at least `min_gap` seconds between the handles.
pub fn range_slider(ui: &mut Ui, start: &mut f64, end: &mut f64, max: f64, min_gap: f64) -> bool {
    let p = pal_ui(ui);
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(w, 48.0), Sense::hover());
    let track = Rect::from_min_max(
        pos2(rect.left() + 8.0, rect.center().y - 4.0),
        pos2(rect.right() - 8.0, rect.center().y + 4.0),
    );
    let x_of = |t: f64| track.left() + (t / max.max(0.001)) as f32 * track.width();
    let t_of = |x: f32| (((x - track.left()) / track.width()) as f64 * max).clamp(0.0, max);

    ui.painter()
        .rect_filled(track, Rounding::same(4.0), p.card_hi);
    let sel = Rect::from_min_max(
        pos2(x_of(*start), track.top()),
        pos2(x_of(*end), track.bottom()),
    );
    ui.painter().rect_filled(sel, Rounding::same(4.0), p.accent);

    let mut changed = false;
    for (is_start, value) in [(true, &mut *start), (false, &mut *end)] {
        let cx = x_of(*value);
        let handle = Rect::from_center_size(pos2(cx, rect.center().y), vec2(18.0, 38.0));
        let id = ui
            .id()
            .with(if is_start { "range-start" } else { "range-end" });
        let resp = ui.interact(handle.expand2(vec2(6.0, 4.0)), id, Sense::drag());
        if resp.dragged() {
            if let Some(pos) = resp.interact_pointer_pos() {
                *value = t_of(pos.x);
                changed = true;
            }
        }
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        ui.painter().rect_filled(
            handle.translate(vec2(0.0, 1.0)),
            Rounding::same(7.0),
            Color32::from_black_alpha(50),
        );
        ui.painter()
            .rect_filled(handle, Rounding::same(7.0), Color32::WHITE);
        ui.painter().rect_filled(
            Rect::from_center_size(handle.center(), vec2(2.0, 16.0)),
            Rounding::same(1.0),
            Color32::from_rgb(0xc7, 0xc7, 0xcc),
        );
    }
    // Keep the handles in order and apart.
    if *start > *end - min_gap {
        // Whichever moved last gives way to the other; clamp both to the range.
        let mid = (*start + *end) / 2.0;
        *start = (mid - min_gap / 2.0).max(0.0);
        *end = (*start + min_gap).min(max);
        *start = (*end - min_gap).max(0.0);
    }
    changed
}

/// A named, coloured bookmark tag. `filled` shows it as applied (solid colour);
/// otherwise it is an outlined, quieter chip. `small` is the compact card size.
pub fn bookmark_pill(ui: &mut Ui, name: &str, rgb: [u8; 3], filled: bool, small: bool) -> Response {
    let p = pal_ui(ui);
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    // Quiet by design: on cards just a small dot and the name in grey with no
    // background; in the sheet a faint tint when applied.
    let size = if small { 11.5 } else { 13.0 };
    let font = FontId::new(size, egui::FontFamily::Proportional);
    let galley = ui.painter().layout_no_wrap(name.to_string(), font, p.text);
    let pad = if small {
        vec2(2.0, 2.0)
    } else {
        vec2(12.0, 7.0)
    };
    let dot = if small { 6.0 } else { 8.0 };
    let w = galley.size().x + pad.x * 2.0 + dot + 6.0;
    let (rect, resp) =
        ui.allocate_exact_size(vec2(w, galley.size().y + pad.y * 2.0), Sense::click());
    let r = Rounding::same(rect.height() / 2.0);
    let (bg, fg, dot_col) = if small {
        (Color32::TRANSPARENT, p.text_2, color)
    } else if filled {
        (color.linear_multiply(0.10), p.text, color)
    } else if resp.hovered() {
        (mix(p.card_hi, p.text, 0.06), p.text_2, color)
    } else {
        (p.card_hi, p.text_2, color.linear_multiply(0.6))
    };
    ui.painter().rect_filled(rect, r, bg);
    ui.painter().circle_filled(
        pos2(rect.left() + pad.x + dot / 2.0, rect.center().y),
        dot / 2.0,
        dot_col,
    );
    ui.painter().galley_with_override_text_color(
        pos2(
            rect.left() + pad.x + dot + 6.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        fg,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// A row of colour swatches; returns the chosen colour index when one is clicked.
pub fn swatches(ui: &mut Ui, colors: &[[u8; 3]], selected: Option<[u8; 3]>) -> Option<usize> {
    let p = pal_ui(ui);
    let mut picked = None;
    ui.spacing_mut().item_spacing.x = 7.0;
    for (i, c) in colors.iter().enumerate() {
        let col = Color32::from_rgb(c[0], c[1], c[2]);
        let (rect, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::click());
        ui.painter().circle_filled(rect.center(), 8.5, col);
        if selected == Some(*c) {
            ui.painter()
                .circle_stroke(rect.center(), 11.0, Stroke::new(2.0_f32, p.text));
        } else if resp.hovered() {
            ui.painter()
                .circle_stroke(rect.center(), 11.0, Stroke::new(1.5_f32, p.text_3));
        }
        if resp.clicked() {
            picked = Some(i);
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    picked
}

// ----------------------------------------------------------------- themes --

/// A tiny picture of the app in one palette: sidebar, a card and an accent.
fn theme_preview(painter: &egui::Painter, rect: Rect, p: &theme::Palette) {
    let r = Rounding::same(9.0);
    painter.rect_filled(rect, r, p.bg);
    let side = Rect::from_min_max(
        rect.min,
        pos2(rect.left() + rect.width() * 0.28, rect.bottom()),
    );
    painter.rect_filled(
        side,
        Rounding {
            ne: 0.0,
            se: 0.0,
            ..r
        },
        p.sidebar,
    );
    // Sidebar items.
    for i in 0..3 {
        let y = rect.top() + 12.0 + i as f32 * 9.0;
        let w = side.width() - 14.0;
        painter.rect_filled(
            Rect::from_min_size(pos2(side.left() + 7.0, y), vec2(w, 4.0)),
            2.0,
            if i == 0 { p.accent } else { p.card_hi },
        );
    }
    // A card with a line of text and an accent button.
    let card = Rect::from_min_max(
        pos2(side.right() + 8.0, rect.top() + 10.0),
        pos2(rect.right() - 8.0, rect.bottom() - 10.0),
    );
    painter.rect_filled(card, 5.0, p.card);
    painter.rect_stroke(card, 5.0, Stroke::new(1.0_f32, p.line));
    painter.rect_filled(
        Rect::from_min_size(card.min + vec2(6.0, 6.0), vec2(card.width() * 0.6, 4.0)),
        2.0,
        p.text_2,
    );
    painter.rect_filled(
        Rect::from_min_size(
            pos2(card.left() + 6.0, card.bottom() - 14.0),
            vec2(card.width() * 0.4, 8.0),
        ),
        4.0,
        p.accent,
    );
}

/// A grid of theme tiles. Returns true when the choice changed. `compact` makes
/// the tiles smaller (the setup guide).
pub fn theme_picker(ui: &mut Ui, current: &mut String, compact: bool) -> bool {
    let p = pal_ui(ui);
    let (tw, th) = if compact { (98.0, 60.0) } else { (132.0, 80.0) };
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
        for t in crate::themes::THEMES {
            let selected = current.as_str() == t.id;
            let (rect, resp) = ui.allocate_exact_size(vec2(tw, th + 22.0), Sense::click());
            let tile = Rect::from_min_size(rect.min, vec2(tw, th));
            match &t.pal {
                Some(pal) => theme_preview(ui.painter(), tile, pal),
                None => {
                    // The default follows Windows, so show both halves.
                    theme_preview(ui.painter(), tile, &theme::DARK);
                    let right = Rect::from_min_max(pos2(tile.center().x, tile.top()), tile.max);
                    theme_preview(&ui.painter().with_clip_rect(right), tile, &theme::LIGHT);
                }
            }
            let (stroke, width) = if selected {
                (p.accent, 2.5)
            } else if resp.hovered() {
                (p.text_3, 1.5)
            } else {
                (p.line, 1.0_f32)
            };
            ui.painter()
                .rect_stroke(tile, 9.0, Stroke::new(width, stroke));
            ui.painter().text(
                pos2(tile.left() + 2.0, tile.bottom() + 5.0),
                Align2::LEFT_TOP,
                t.name,
                FontId::new(
                    12.5,
                    if selected {
                        theme::semibold()
                    } else {
                        egui::FontFamily::Proportional
                    },
                ),
                if selected { p.text } else { p.text_2 },
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !selected
            {
                *current = t.id.to_string();
                changed = true;
            }
        }
    });
    changed
}

/// Accent colour swatches (and "theme default"). Returns true when it changed.
pub fn accent_picker(ui: &mut Ui, current: &mut Option<String>) -> bool {
    let p = pal_ui(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        // "Theme default": a ring with no fill.
        let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
        ui.painter()
            .circle_stroke(rect.center(), 10.0, Stroke::new(1.5_f32, p.text_3));
        ui.painter().line_segment(
            [
                rect.center() + vec2(-6.0, 6.0),
                rect.center() + vec2(6.0, -6.0),
            ],
            Stroke::new(1.5_f32, p.text_3),
        );
        if current.is_none() {
            ui.painter()
                .circle_stroke(rect.center(), 13.0, Stroke::new(2.0_f32, p.accent));
        }
        if resp
            .on_hover_text("Theme default")
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
            && current.is_some()
        {
            *current = None;
            changed = true;
        }
        for (hex, name) in crate::themes::ACCENTS {
            let color = crate::themes::parse_hex(hex).unwrap_or(p.accent);
            let (rect, resp) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 10.0, color);
            let chosen = current.as_deref() == Some(*hex);
            if chosen {
                ui.painter()
                    .circle_stroke(rect.center(), 13.0, Stroke::new(2.0_f32, color));
            }
            if resp
                .on_hover_text(*name)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !chosen
            {
                *current = Some((*hex).to_string());
                changed = true;
            }
        }
    });
    changed
}
