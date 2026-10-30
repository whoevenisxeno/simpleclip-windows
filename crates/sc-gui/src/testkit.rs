use crate::theme;
use egui::epaint::Shape as Paint;
use egui::{vec2, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui};

pub(crate) struct Harness {
    pub ctx: egui::Context,
    pub time: f64,
    pub texts: Vec<(String, Rect)>,
    pub right: f32,
    pub size: egui::Vec2,
}

fn collect(shape: &Paint, out: &mut Vec<(String, Rect)>) {
    match shape {
        Paint::Text(t) => out.push((
            t.galley.text().to_string(),
            t.galley.rect.translate(t.pos.to_vec2()),
        )),
        Paint::Vec(list) => list.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

pub(crate) fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    }
}

impl Harness {
    pub fn new() -> Self {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut h = Self {
            ctx,
            time: 0.0,
            texts: Vec::new(),
            right: 0.0,
            size: vec2(1000.0, 1600.0),
        };
        h.frame(Vec::new(), Modifiers::NONE, &mut |_| {});
        h
    }

    pub fn sized(mut self, width: f32, height: f32) -> Self {
        self.size = vec2(width, height);
        self
    }

    pub fn frame(
        &mut self,
        events: Vec<Event>,
        modifiers: Modifiers,
        add: &mut dyn FnMut(&mut Ui),
    ) {
        self.time += 0.05;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            modifiers,
            events,
            ..Default::default()
        };
        let mut right = 0.0;
        let out = self.ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                right = ui.available_rect_before_wrap().right();
                add(ui);
            });
        });
        self.right = right;
        self.texts.clear();
        for clipped in &out.shapes {
            let mut found = Vec::new();
            collect(&clipped.shape, &mut found);
            self.texts.extend(
                found
                    .into_iter()
                    .filter(|(_, r)| clipped.clip_rect.intersects(*r)),
            );
        }
    }

    pub fn at(&self, text: &str, nth: usize) -> Option<Rect> {
        self.texts
            .iter()
            .filter(|(t, _)| t.trim() == text)
            .nth(nth)
            .map(|(_, r)| *r)
    }

    pub fn has(&self, text: &str) -> bool {
        self.at(text, 0).is_some()
    }

    pub fn press(at: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    pub fn click(&mut self, at: Pos2, add: &mut dyn FnMut(&mut Ui)) {
        self.time += 0.6;
        for events in [
            vec![Event::PointerMoved(at)],
            vec![Self::press(at, true)],
            vec![Self::press(at, false)],
            Vec::new(),
            Vec::new(),
        ] {
            self.frame(events, Modifiers::NONE, add);
        }
    }

    pub fn click_text(&mut self, text: &str, add: &mut dyn FnMut(&mut Ui)) {
        let at = self
            .at(text, 0)
            .unwrap_or_else(|| panic!("no \"{text}\" on screen"))
            .center();
        self.click(at, add);
    }

    pub fn click_nth(&mut self, text: &str, nth: usize, add: &mut dyn FnMut(&mut Ui)) {
        let at = self
            .at(text, nth)
            .unwrap_or_else(|| panic!("no \"{text}\" number {nth} on screen"))
            .center();
        self.click(at, add);
    }

    pub fn drag(&mut self, from: Pos2, to: Pos2, add: &mut dyn FnMut(&mut Ui)) {
        self.time += 0.6;
        let mid = from + (to - from) * 0.5;
        for events in [
            vec![Event::PointerMoved(from)],
            vec![Self::press(from, true)],
            vec![Event::PointerMoved(mid)],
            vec![Event::PointerMoved(to)],
            vec![Self::press(to, false)],
            Vec::new(),
            Vec::new(),
        ] {
            self.frame(events, Modifiers::NONE, add);
        }
    }

    pub fn key(&mut self, key: Key, modifiers: Modifiers, add: &mut dyn FnMut(&mut Ui)) {
        let down = Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        };
        let up = Event::Key {
            key,
            physical_key: Some(key),
            pressed: false,
            repeat: false,
            modifiers,
        };
        self.frame(vec![down], modifiers, add);
        self.frame(vec![up], Modifiers::NONE, add);
    }
}
