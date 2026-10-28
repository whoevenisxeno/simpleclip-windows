use crate::theme::{self, mix, pal_ui};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Layout, Rect, Response, RichText, Rounding, Sense,
    Stroke, Ui,
};

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

pub fn caption(ui: &mut Ui, text: &str) {
    let p = pal_ui(ui);
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .font(FontId::new(12.0, theme::semibold()))
            .color(p.text_2),
    );
    ui.add_space(-2.0);
}

pub fn secondary(ui: &mut Ui, text: impl Into<String>) {
    let p = pal_ui(ui);
    ui.label(RichText::new(text.into()).size(13.0).color(p.text_2));
}

pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let p = pal_ui(ui);
    egui::Frame::none()
        .fill(p.card)
        .rounding(Rounding::same(theme::radius::CARD))
        .stroke(Stroke::new(1.0_f32, p.line))
        .shadow(theme::card_shadow(&p))
        .inner_margin(egui::Margin::same(18.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

const SEARCH_ID: &str = "settings-search";
const ROWS_ID: &str = "settings-rows";

pub fn search_query(ctx: &egui::Context) -> String {
    ctx.data(|d| d.get_temp::<String>(egui::Id::new(SEARCH_ID)))
        .unwrap_or_default()
}

pub fn set_search_query(ctx: &egui::Context, query: &str) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(SEARCH_ID), query.trim().to_lowercase()));
}

pub fn reset_rows(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(ROWS_ID), 0usize));
}

pub fn rows_drawn(ctx: &egui::Context) -> usize {
    ctx.data(|d| d.get_temp::<usize>(egui::Id::new(ROWS_ID)))
        .unwrap_or(0)
}

fn bump_rows(ctx: &egui::Context) {
    let n = rows_drawn(ctx) + 1;
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(ROWS_ID), n));
}

pub fn matches_query(ctx: &egui::Context, texts: &[&str]) -> bool {
    let q = search_query(ctx);
    q.is_empty() || texts.iter().any(|t| t.to_lowercase().contains(&q))
}

pub struct Section<'a> {
    ui: &'a mut Ui,
    first: bool,
    flat: bool,
}

pub fn section<R>(ui: &mut Ui, header: Option<&str>, add: impl FnOnce(&mut Section) -> R) -> R {
    if !search_query(ui.ctx()).is_empty() {
        let mut s = Section {
            ui,
            first: true,
            flat: true,
        };
        return add(&mut s);
    }
    if let Some(h) = header {
        caption(ui, h);
    }
    let p = pal_ui(ui);
    let out = egui::Frame::none()
        .fill(p.card)
        .rounding(Rounding::same(theme::radius::CARD))
        .stroke(Stroke::new(1.0_f32, p.line))
        .shadow(theme::card_shadow(&p))
        .inner_margin(egui::Margin::symmetric(16.0, 0.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut s = Section {
                ui,
                first: true,
                flat: false,
            };
            add(&mut s)
        })
        .inner;
    ui.add_space(20.0);
    out
}

impl Section<'_> {
    fn divider(&mut self) {
        let draw = if self.flat {
            rows_drawn(self.ui.ctx()) > 0
        } else {
            !self.first
        };
        if draw {
            let p = pal_ui(self.ui);
            let w = self.ui.available_width();
            let (r, _) = self.ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
            self.ui.painter().rect_filled(r, 0.0, p.line);
        }
        self.first = false;
        if self.flat {
            bump_rows(self.ui.ctx());
        }
    }

    pub fn row(&mut self, label: &str, hint: Option<&str>, control: impl FnOnce(&mut Ui)) {
        if self.flat && !matches_query(self.ui.ctx(), &[label, hint.unwrap_or("")]) {
            return;
        }
        self.divider();
        let p = pal_ui(self.ui);
        let h = if hint.is_some() { 60.0 } else { 50.0 };
        let w = self.ui.available_width();
        if w < 560.0 {
            self.ui.add_space(12.0);
            self.ui.label(RichText::new(label).size(14.0).color(p.text));
            if let Some(h) = hint {
                self.ui.label(RichText::new(h).size(12.0).color(p.text_2));
            }
            self.ui.add_space(8.0);
            self.ui.allocate_ui_with_layout(
                vec2(w, 36.0),
                Layout::right_to_left(Align::Center),
                |ui| control(ui),
            );
            self.ui.add_space(12.0);
            return;
        }
        self.ui
            .allocate_ui_with_layout(vec2(w, h), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_height(h);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let text_h = if hint.is_some() { 36.0 } else { 20.0 };
                    ui.add_space(((h - text_h) / 2.0).max(0.0));
                    ui.label(RichText::new(label).size(14.0).color(p.text));
                    if let Some(h) = hint {
                        ui.label(RichText::new(h).size(12.0).color(p.text_2));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| control(ui));
            });
    }

    pub fn full(&mut self, keywords: &str, add: impl FnOnce(&mut Ui)) {
        if self.flat && !matches_query(self.ui.ctx(), &[keywords]) {
            return;
        }
        self.divider();
        self.ui.add_space(10.0);
        add(self.ui);
        self.ui.add_space(10.0);
    }
}

pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let p = pal_ui(ui);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(51.0, 31.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.2);
    let off_col = if p.dark {
        Color32::from_rgb(0x39, 0x39, 0x3d)
    } else {
        Color32::from_rgb(0xe3, 0xe3, 0xe8)
    };
    let track = mix(off_col, p.green, t);
    let radius = rect.height() / 2.0;
    ui.painter()
        .rect_filled(rect, Rounding::same(radius), track);
    let knob_r = radius - 2.0;
    let knob = pos2(
        egui::lerp((rect.left() + radius)..=(rect.right() - radius), t),
        rect.center().y,
    );
    ui.painter().circle_filled(
        knob + vec2(0.0, 1.5),
        knob_r + 0.5,
        Color32::from_black_alpha(34),
    );
    ui.painter().circle_filled(knob, knob_r, Color32::WHITE);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

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
    let (rect, _) = ui.allocate_exact_size(vec2(total, 32.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, Rounding::same(theme::radius::CONTROL), p.card_hi);
    let mut starts = Vec::with_capacity(widths.len());
    let mut at = rect.left() + 2.0;
    for w in &widths {
        starts.push(at);
        at += w;
    }
    if let Some(i) = options.iter().position(|(v, _)| *v == *value) {
        let id = ui
            .id()
            .with(("seg-thumb", options.first().map(|o| o.1).unwrap_or("")));
        let x = ui.ctx().animate_value_with_time(id, starts[i], 0.18);
        let w = ui
            .ctx()
            .animate_value_with_time(id.with("w"), widths[i], 0.18);
        let seg = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, 28.0));
        let fill = if p.dark {
            Color32::from_rgb(0x63, 0x63, 0x66)
        } else {
            Color32::WHITE
        };
        let r = Rounding::same(theme::radius::SMALL);
        ui.painter().rect_filled(
            seg.translate(vec2(0.0, 1.0)),
            r,
            Color32::from_black_alpha(if p.dark { 60 } else { 24 }),
        );
        ui.painter().rect_filled(seg, r, fill);
    }
    let mut x = rect.left() + 2.0;
    for ((val, label), w) in options.iter().zip(widths) {
        let seg = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(w, 28.0));
        let resp = ui.interact(seg, ui.id().with(label), Sense::click());
        let selected = *val == *value;
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

pub fn button(ui: &mut Ui, tone: Tone, glyph: Option<&str>, label: &str) -> Response {
    let p = pal_ui(ui);
    let font = FontId::new(
        14.0,
        if tone == Tone::Primary {
            theme::semibold()
        } else {
            egui::FontFamily::Proportional
        },
    );
    let icon_font = FontId::new(15.0, theme::icons());
    let use_icon = glyph.is_some() && theme::has_icons(ui.ctx());
    let text_w = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), p.text)
        .size()
        .x;
    let w = text_w + 28.0 + if use_icon { 24.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(40.0), 32.0), Sense::click());
    let (bg, fg) = match tone {
        Tone::Primary => (p.accent, theme::on_accent(&p)),
        Tone::Quiet => (p.card_hi, p.text),
    };
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let lift = if p.dark {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let bg = mix(bg, lift, 0.08 * hover);
    let bg = if resp.is_pointer_button_down_on() {
        mix(bg, Color32::BLACK, 0.16)
    } else {
        bg
    };
    ui.painter()
        .rect_filled(rect, Rounding::same(theme::radius::CONTROL), bg);
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

pub fn icon_button(ui: &mut Ui, glyph: &str, fallback: &str, tip: &str, danger: bool) -> Response {
    let p = pal_ui(ui);
    let has = theme::has_icons(ui.ctx());
    let size = if has {
        vec2(32.0, 32.0)
    } else {
        vec2(54.0, 28.0)
    };
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let bg = mix(
        p.card_hi,
        if p.dark {
            Color32::WHITE
        } else {
            Color32::BLACK
        },
        0.08 * hover,
    );
    ui.painter()
        .rect_filled(rect, Rounding::same(theme::radius::CONTROL - 1.0), bg);
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

pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

pub fn keycaps(ui: &mut Ui, combo: &str) {
    ui.spacing_mut().item_spacing.x = 5.0;
    for part in combo.split('+').filter(|s| !s.is_empty()) {
        keycap(ui, part);
    }
}

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
        "PAGEUP" => "Page up".into(),
        "PAGEDOWN" => "Page down".into(),
        "CAPSLOCK" => "Caps lock".into(),
        "NUMLOCK" => "Num lock".into(),
        "SCROLLLOCK" => "Scroll lock".into(),
        "BACKSPACE" => "Backspace".into(),
        "APPS" => "Menu".into(),
        "ESCAPE" => "Esc".into(),
        "NUMADD" => "Num +".into(),
        "NUMSUBTRACT" => "Num -".into(),
        "NUMMULTIPLY" => "Num *".into(),
        "NUMDIVIDE" => "Num /".into(),
        "NUMDECIMAL" => "Num .".into(),
        n if n.len() == 4 && n.starts_with("NUM") => format!("Num {}", &n[3..]),
        n if n.starts_with("VK_") => format!("Key {}", &n[3..]),
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

pub fn hotkey_field(ui: &mut Ui, value: &mut String, id: &str) -> bool {
    let p = pal_ui(ui);
    let rec_id = ui.make_persistent_id(id);
    let recording = ui.data(|d| d.get_temp::<bool>(rec_id)).unwrap_or(false);
    let mut changed = false;
    if recording {
        let cancel = quiet_button(ui, None, "Cancel");
        if cancel.clicked() {
            ui.data_mut(|d| d.insert_temp(rec_id, false));
        }
        ui.label(RichText::new("Press the new shortcut…").color(p.orange));
        #[cfg(windows)]
        if ui.ctx().input(|i| i.focused) {
            let armed_id = rec_id.with("armed");
            let armed = ui.data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
            match keypoll::poll(armed) {
                keypoll::Poll::Waiting => {}
                keypoll::Poll::Armed => ui.data_mut(|d| d.insert_temp(armed_id, true)),
                keypoll::Poll::Cancelled => ui.data_mut(|d| d.insert_temp(rec_id, false)),
                keypoll::Poll::Combo(text) => {
                    *value = text;
                    changed = true;
                    ui.data_mut(|d| d.insert_temp(rec_id, false));
                }
            }
        }
        #[cfg(not(windows))]
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
            ui.data_mut(|d| {
                d.insert_temp(rec_id, true);
                d.insert_temp(rec_id.with("armed"), false);
            });
        }
        keycaps_rtl(ui, value);
    }
    changed
}

#[cfg(windows)]
mod keypoll {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    pub enum Poll {
        Waiting,
        Armed,
        Cancelled,
        Combo(String),
    }

    const VK_ESCAPE: i32 = 0x1B;

    fn modifier_names(down: &impl Fn(i32) -> bool) -> Vec<String> {
        let mut parts = Vec::new();
        if down(0x11) {
            parts.push("CTRL".to_string());
        }
        if down(0x12) {
            parts.push("ALT".to_string());
        }
        if down(0x10) {
            parts.push("SHIFT".to_string());
        }
        if down(0x5B) || down(0x5C) {
            parts.push("WIN".to_string());
        }
        parts
    }

    pub fn combo(down: impl Fn(i32) -> bool) -> Option<String> {
        let mut parts = modifier_names(&down);
        let key = (0x08..=0xFEu32).find_map(|vk| {
            let usable = down(vk as i32) && (vk != VK_ESCAPE as u32 || !parts.is_empty());
            usable.then(|| sc_core::keys::name_from_vk(vk)).flatten()
        })?;
        parts.push(key);
        Some(parts.join("+"))
    }

    pub fn poll(armed: bool) -> Poll {
        let down = |vk: i32| unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 };
        let combo = combo(down);
        if !armed {
            return if combo.is_none() && !down(VK_ESCAPE) {
                Poll::Armed
            } else {
                Poll::Waiting
            };
        }
        if down(VK_ESCAPE) && modifier_names(&down).is_empty() {
            return Poll::Cancelled;
        }
        match combo {
            Some(text) => Poll::Combo(text),
            None => Poll::Waiting,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::combo;

        #[test]
        fn alt_f10_keeps_the_alt() {
            let held = [0x12, 0x79];
            assert_eq!(combo(|vk| held.contains(&vk)).unwrap(), "ALT+F10");
        }

        #[test]
        fn modifiers_come_in_a_fixed_order() {
            let held = [0x10, 0x12, 0x11, 0x47];
            assert_eq!(combo(|vk| held.contains(&vk)).unwrap(), "CTRL+ALT+SHIFT+G");
            let win = [0x5B, 0x31];
            assert_eq!(combo(|vk| win.contains(&vk)).unwrap(), "WIN+1");
        }

        #[test]
        fn any_key_can_be_recorded() {
            assert_eq!(combo(|vk| vk == 0xC0).unwrap(), "`");
            assert_eq!(combo(|vk| vk == 0x25).unwrap(), "LEFT");
            assert_eq!(
                combo(|vk| [0x11, 0x6B].contains(&vk)).unwrap(),
                "CTRL+NUMADD"
            );
            assert_eq!(combo(|vk| vk == 0xFA).unwrap(), "VK_FA");
        }

        #[test]
        fn escape_is_a_key_only_with_a_modifier() {
            assert!(combo(|vk| vk == 0x1B).is_none());
            assert_eq!(
                combo(|vk| [0x11, 0x1B].contains(&vk)).unwrap(),
                "CTRL+ESCAPE"
            );
        }

        #[test]
        fn modifiers_alone_record_nothing() {
            let held = [0x11, 0x12, 0x10];
            assert!(combo(|vk| held.contains(&vk)).is_none());
        }
    }
}

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

pub fn nav_item(
    ui: &mut Ui,
    glyph: &str,
    label: &str,
    selected: bool,
    badge: Option<&str>,
) -> Response {
    let p = pal_ui(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 38.0), Sense::click());
    let hover = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
    let on = ui
        .ctx()
        .animate_bool_with_time(resp.id.with("on"), selected, 0.16);
    let round = Rounding::same(theme::radius::CONTROL - 1.0);
    if on < 1.0 {
        ui.painter().rect_filled(
            rect,
            round,
            p.text.linear_multiply(0.06 * hover * (1.0 - on)),
        );
    }
    if on > 0.0 {
        ui.painter()
            .rect_filled(rect, round, mix(p.sidebar, p.card, on));
        ui.painter().rect_stroke(
            rect,
            round,
            Stroke::new(1.0_f32, p.line.linear_multiply(on)),
        );
        let bar = Rect::from_center_size(
            pos2(rect.left() + 3.0, rect.center().y),
            vec2(3.0, 16.0 * on),
        );
        ui.painter().rect_filled(bar, Rounding::same(1.5), p.accent);
    }
    let fg = if selected { p.text } else { p.text_2 };
    let icon_col = if selected { p.accent } else { p.text_2 };
    let mut x = rect.left() + 14.0;
    if theme::has_icons(ui.ctx()) {
        ui.painter().text(
            pos2(x + 9.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            FontId::new(16.0, theme::icons()),
            icon_col,
        );
        x += 29.0;
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
    if let Some(b) = badge {
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            b,
            FontId::new(12.0, egui::FontFamily::Proportional),
            p.text_3,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

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
    if *start > *end - min_gap {
        let mid = (*start + *end) / 2.0;
        *start = (mid - min_gap / 2.0).max(0.0);
        *end = (*start + min_gap).min(max);
        *start = (*end - min_gap).max(0.0);
    }
    changed
}

pub fn bookmark_pill(ui: &mut Ui, name: &str, rgb: [u8; 3], filled: bool, small: bool) -> Response {
    let p = pal_ui(ui);
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
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

fn theme_preview(
    painter: &egui::Painter,
    rect: Rect,
    base: &theme::Palette,
    accent: Option<Color32>,
) {
    let mut pal = *base;
    if let Some(a) = accent {
        pal.accent = a;
    }
    let p = &pal;
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
    for i in 0..3 {
        let y = rect.top() + 12.0 + i as f32 * 9.0;
        let w = side.width() - 14.0;
        painter.rect_filled(
            Rect::from_min_size(pos2(side.left() + 7.0, y), vec2(w, 4.0)),
            2.0,
            if i == 0 { p.accent } else { p.card_hi },
        );
    }
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

pub fn theme_picker(
    ui: &mut Ui,
    current: &mut String,
    compact: bool,
    accent: Option<&str>,
) -> bool {
    let accent = accent.and_then(crate::themes::parse_hex);
    let p = pal_ui(ui);
    let (tw, th) = if compact { (80.0, 52.0) } else { (132.0, 80.0) };
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(10.0, 8.0);
        for t in crate::themes::THEMES {
            let selected = current.as_str() == t.id;
            let (rect, resp) = ui.allocate_exact_size(vec2(tw, th + 22.0), Sense::click());
            let tile = Rect::from_min_size(rect.min, vec2(tw, th));
            match &t.pal {
                Some(pal) => theme_preview(ui.painter(), tile, pal, accent),
                None => {
                    theme_preview(ui.painter(), tile, &theme::DARK, accent);
                    let right = Rect::from_min_max(pos2(tile.center().x, tile.top()), tile.max);
                    theme_preview(
                        &ui.painter().with_clip_rect(right),
                        tile,
                        &theme::LIGHT,
                        accent,
                    );
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

pub fn accent_picker(ui: &mut Ui, current: &mut Option<String>) -> bool {
    let p = pal_ui(ui);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
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

pub fn percent_slider(ui: &mut Ui, value: &mut u32, max: u32) -> Response {
    let p = pal_ui(ui);
    let track_w = 180.0;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(track_w, 32.0), Sense::click_and_drag());
    let knob_r = 11.0;
    let left = rect.left() + knob_r;
    let right = rect.right() - knob_r;
    let x_of = |v: u32| left + (right - left) * (v as f32 / max.max(1) as f32);
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - left) / (right - left)).clamp(0.0, 1.0);
            let new = (t * max as f32).round() as u32;
            if new != *value {
                *value = new;
                resp.mark_changed();
            }
        }
    }
    if resp.double_clicked() && *value != 100 {
        *value = 100;
        resp.mark_changed();
    }
    let cy = rect.center().y;
    let track = Rect::from_min_max(pos2(left, cy - 2.0), pos2(right, cy + 2.0));
    ui.painter()
        .rect_filled(track, Rounding::same(2.0), p.card_hi);
    if *value != 100 && max > 100 {
        ui.painter()
            .circle_filled(pos2(x_of(100), cy), 1.5, p.text_3);
    }
    let kx = x_of((*value).min(max));
    ui.painter().rect_filled(
        Rect::from_min_max(track.min, pos2(kx, track.max.y)),
        Rounding::same(2.0),
        p.accent,
    );
    let knob = pos2(kx, cy);
    let lift = if resp.dragged() { 0.5 } else { 0.0 };
    ui.painter().circle_filled(
        knob + vec2(0.0, 1.5),
        knob_r + 1.0 + lift,
        Color32::from_black_alpha(if p.dark { 70 } else { 36 }),
    );
    ui.painter().circle_filled(knob, knob_r, Color32::WHITE);
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

pub fn pretty_combo(combo: &str) -> String {
    combo
        .split('+')
        .filter(|s| !s.is_empty())
        .map(pretty_key)
        .collect::<Vec<_>>()
        .join("+")
}

pub fn monitor_picker(
    ui: &mut Ui,
    devices: &mut crate::app::Devices,
    selected: &mut String,
) -> bool {
    let p = pal_ui(ui);
    devices.refresh_previews(ui.ctx());
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_secs(4));
    let monitors = devices.monitors.clone();
    let primary_id = monitors
        .iter()
        .find(|m| m.primary)
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let active = if selected.is_empty() || !monitors.iter().any(|m| m.id == *selected) {
        primary_id
    } else {
        selected.clone()
    };
    let gap = 12.0;
    let avail = ui.available_width();
    let cols = (((avail + gap) / (190.0 + gap)).floor() as usize).clamp(1, monitors.len().max(1));
    let tw = ((avail - gap * (cols as f32 - 1.0)) / cols as f32)
        .floor()
        .min(270.0);
    let th = (tw * 9.0 / 16.0).round();
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, 14.0);
        for m in &monitors {
            let (rect, resp) = ui.allocate_exact_size(vec2(tw, th + 44.0), Sense::click());
            let frame = Rect::from_min_size(rect.min, vec2(tw, th));
            let on = m.id == active;
            let hover =
                ui.ctx()
                    .animate_bool_with_time(resp.id.with("hover"), resp.hovered(), 0.12);
            ui.painter()
                .rect_filled(frame, Rounding::same(10.0), Color32::from_gray(14));
            let (mw, mh) = (m.width.max(1) as f32, m.height.max(1) as f32);
            let fit = (tw / mw).min(th / mh);
            let image = Rect::from_center_size(frame.center(), vec2(mw * fit, mh * fit));
            match devices.preview(&m.id) {
                Some(tex) => {
                    egui::Image::new(egui::load::SizedTexture::new(tex.id(), image.size()))
                        .rounding(Rounding::same(8.0))
                        .paint_at(ui, image);
                }
                None => {
                    ui.painter().text(
                        frame.center(),
                        Align2::CENTER_CENTER,
                        "Preview unavailable",
                        FontId::new(12.0, egui::FontFamily::Proportional),
                        p.text_3,
                    );
                }
            }
            if !on && hover < 1.0 {
                ui.painter().rect_filled(
                    frame,
                    Rounding::same(10.0),
                    Color32::from_black_alpha((70.0 * (1.0 - hover)) as u8),
                );
            }
            let (stroke, width) = if on {
                (p.accent, 2.5)
            } else if resp.hovered() {
                (p.text_3, 1.5)
            } else {
                (p.line, 1.0_f32)
            };
            ui.painter()
                .rect_stroke(frame, Rounding::same(10.0), Stroke::new(width, stroke));
            let badge = Rect::from_min_size(frame.left_top() + vec2(8.0, 8.0), vec2(22.0, 22.0));
            ui.painter()
                .rect_filled(badge, Rounding::same(7.0), Color32::from_black_alpha(170));
            ui.painter().text(
                badge.center(),
                Align2::CENTER_CENTER,
                &m.id,
                FontId::new(12.0, theme::semibold()),
                Color32::WHITE,
            );
            if on {
                let tag = "Recording this screen";
                let galley = ui.painter().layout_no_wrap(
                    tag.to_string(),
                    FontId::new(11.0, theme::semibold()),
                    theme::on_accent(&p),
                );
                let pill = Rect::from_min_size(
                    pos2(frame.left() + 8.0, frame.bottom() - 28.0),
                    galley.size() + vec2(16.0, 8.0),
                );
                ui.painter()
                    .rect_filled(pill, Rounding::same(pill.height() / 2.0), p.accent);
                ui.painter().galley(
                    pill.center() - galley.size() / 2.0,
                    galley,
                    theme::on_accent(&p),
                );
            }
            let hz = (m.refresh_mhz as f32 / 1000.0).round();
            let mut line = format!("{}\u{00D7}{}", m.width, m.height);
            if hz > 0.0 {
                line.push_str(&format!(" \u{00B7} {hz:.0} Hz"));
            }
            if m.primary {
                line.push_str(" \u{00B7} Main");
            }
            ui.painter().text(
                pos2(rect.left() + 2.0, frame.bottom() + 8.0),
                Align2::LEFT_TOP,
                &m.name,
                FontId::new(13.0, theme::semibold()),
                p.text,
            );
            ui.painter().text(
                pos2(rect.left() + 2.0, frame.bottom() + 26.0),
                Align2::LEFT_TOP,
                line,
                FontId::new(12.0, egui::FontFamily::Proportional),
                p.text_2,
            );
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
                && !on
            {
                *selected = if m.primary {
                    String::new()
                } else {
                    m.id.clone()
                };
                changed = true;
            }
        }
    });
    changed
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything() {
        let ctx = egui::Context::default();
        set_search_query(&ctx, "   ");
        assert!(search_query(&ctx).is_empty());
        assert!(matches_query(&ctx, &["Display"]));
    }

    #[test]
    fn a_query_matches_label_or_hint_ignoring_case() {
        let ctx = egui::Context::default();
        set_search_query(&ctx, " Sound ");
        assert_eq!(search_query(&ctx), "sound");
        assert!(matches_query(&ctx, &["System sound", ""]));
        assert!(matches_query(
            &ctx,
            &["Save as a preset", "Keeps the current sound"]
        ));
        assert!(!matches_query(&ctx, &["Display", "Which screen to record"]));
    }

    #[test]
    fn the_row_counter_starts_over_each_frame() {
        let ctx = egui::Context::default();
        reset_rows(&ctx);
        assert_eq!(rows_drawn(&ctx), 0);
        bump_rows(&ctx);
        bump_rows(&ctx);
        assert_eq!(rows_drawn(&ctx), 2);
        reset_rows(&ctx);
        assert_eq!(rows_drawn(&ctx), 0);
    }
}
