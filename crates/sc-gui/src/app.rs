use crate::daemon::Link;
use crate::{config_io, daemon, screens, theme};
use sc_core::config::Config;
use std::path::PathBuf;

#[derive(PartialEq, Clone)]
pub enum Screen {
    Wizard,
    Dashboard,
    Gallery,
    Trim(PathBuf),
    Settings,
}

pub struct ScApp {
    pub cfg: Config,
    pub link: Link,
    pub link_rx: crossbeam_channel::Receiver<Link>,
    pub screen: Screen,
    pub wizard: screens::wizard::WizardState,
    pub toast: Option<(String, egui::Color32)>,
}

impl ScApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = crossbeam_channel::unbounded();
        daemon::spawn_status_poller(cc.egui_ctx.clone(), tx);
        let cfg = config_io::load();
        // Fresh install (no monitor chosen yet) drops straight into the wizard.
        let screen = if cfg.capture.monitor_id.is_empty() {
            Screen::Wizard
        } else {
            Screen::Dashboard
        };
        Self {
            cfg,
            link: Link::Disconnected,
            link_rx: rx,
            screen,
            wizard: Default::default(),
            toast: None,
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>, color: egui::Color32) {
        self.toast = Some((msg.into(), color));
    }
}

impl eframe::App for ScApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(link) = self.link_rx.try_recv() {
            self.link = link;
        }
        egui::TopBottomPanel::top("statusbar").show(ctx, |ui| self.status_bar(ui));
        if let Some((msg, color)) = self.toast.clone() {
            egui::TopBottomPanel::bottom("toast").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(color, "\u{25CF}");
                    ui.label(msg);
                    if ui.small_button("dismiss").clicked() {
                        self.toast = None;
                    }
                });
            });
        }
        if self.screen != Screen::Wizard {
            egui::SidePanel::left("nav")
                .exact_width(150.0)
                .show(ctx, |ui| self.nav(ui));
        }
        egui::CentralPanel::default().show(ctx, |ui| match self.screen.clone() {
            Screen::Wizard => screens::wizard::view(self, ui),
            Screen::Dashboard => screens::dashboard::view(self, ui),
            Screen::Gallery => screens::gallery::view(self, ui),
            Screen::Trim(path) => screens::trim::view(self, ui, &path),
            Screen::Settings => screens::settings::view(self, ui),
        });
    }
}

impl ScApp {
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("SimpleClip");
            ui.add_space(8.0);
            match &self.link {
                Link::Connected(s) => {
                    let (label, color) = crate::widgets::state_badge(s.state);
                    crate::widgets::pill(ui, label, color);
                    ui.label(format!(
                        "buffer {}s \u{2022} {:.0}%",
                        s.buffer_secs,
                        s.buffer_fill * 100.0
                    ));
                    if s.recording {
                        crate::widgets::pill(ui, "REC", theme::DANGER);
                    }
                }
                Link::Disconnected => crate::widgets::pill(ui, "daemon offline", theme::MUTED),
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Save clip").clicked() {
                    self.send_save();
                }
            });
        });
        ui.add_space(4.0);
    }

    fn send_save(&mut self) {
        match daemon::request(sc_core::ipc::Request::Save { last_secs: None }) {
            Ok(sc_core::ipc::Response::Saved { path, .. }) => {
                self.toast(format!("saved {}", path.display()), theme::OK)
            }
            Ok(sc_core::ipc::Response::Error { message }) => self.toast(message, theme::WARN),
            Ok(_) => {}
            Err(e) => self.toast(e.to_string(), theme::DANGER),
        }
    }
}

impl ScApp {
    fn nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(10.0);
        let items = [
            (Screen::Dashboard, "\u{2302}  Dashboard"),
            (Screen::Gallery, "\u{25A6}  Gallery"),
            (Screen::Settings, "\u{2699}  Settings"),
        ];
        for (screen, label) in items {
            let selected = self.screen == screen
                || matches!((&self.screen, &screen), (Screen::Trim(_), Screen::Gallery));
            if ui
                .selectable_label(selected, egui::RichText::new(label).size(15.0))
                .clicked()
            {
                self.screen = screen;
            }
            ui.add_space(2.0);
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(10.0);
            if ui.small_button("Re-run setup wizard").clicked() {
                self.wizard = Default::default();
                self.screen = Screen::Wizard;
            }
        });
    }
}
