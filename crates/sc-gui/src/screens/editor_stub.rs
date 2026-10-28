use crate::app::ScApp;
use crate::widgets;
use egui::Ui;
use std::path::Path;

#[derive(Default)]
pub struct EditorState;

impl EditorState {
    pub fn stop_player(&mut self) {}

    pub fn demo(&mut self, _name: &str) {}
}

pub fn view(_: &mut ScApp, ui: &mut Ui, _: &Path) {
    widgets::secondary(ui, "The clip editor needs Windows.");
}
