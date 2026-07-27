use crate::app::ScApp;
use crate::daemon::Link;
use crate::{daemon, theme, widgets};
use sc_core::ipc::{Request, Response};

pub fn view(app: &mut ScApp, ui: &mut egui::Ui) {
    ui.add_space(6.0);
    match app.link.clone() {
        Link::Disconnected => disconnected(ui),
        Link::Connected(s) => connected(app, ui, &s),
    }
}

fn disconnected(ui: &mut egui::Ui) {
    widgets::card(ui, "Daemon not running", |ui| {
        ui.colored_label(theme::MUTED, "The SimpleClip daemon (scd) isn't reachable.");
        ui.add_space(6.0);
        ui.label("Start it with:");
        ui.code("scd --foreground");
    });
}

fn connected(app: &mut ScApp, ui: &mut egui::Ui, s: &sc_core::ipc::StatusReport) {
    widgets::card(ui, "Capture", |ui| {
        let (label, color) = widgets::state_badge(s.state);
        ui.horizontal(|ui| {
            widgets::pill(ui, label, color);
            if s.recording {
                widgets::pill(ui, "recording", theme::DANGER);
            }
        });
        ui.add_space(10.0);
        ui.add(egui::ProgressBar::new(s.buffer_fill).text(format!("buffer {}s", s.buffer_secs)));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(format!(
                "encoder: {}",
                s.encoder.map(|e| format!("{e:?}")).unwrap_or("-".into())
            ));
            ui.separator();
            ui.label(format!("A/V drift: {:.1} ms", s.drift_ms));
        });
    });
    ui.add_space(10.0);
    actions(app, ui, s);
}

fn actions(app: &mut ScApp, ui: &mut egui::Ui, s: &sc_core::ipc::StatusReport) {
    widgets::card(ui, "Quick actions", |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Save last clip").clicked() {
                send(app, Request::Save { last_secs: None });
            }
            if ui.button("Screenshot").clicked() {
                send(app, Request::Screenshot);
            }
            let paused = matches!(s.state, sc_core::capture::CaptureState::Paused);
            if paused {
                if ui.button("Resume").clicked() {
                    send(app, Request::Resume);
                }
            } else if ui.button("Pause").clicked() {
                send(app, Request::Pause);
            }
            if s.recording {
                if ui.button("Stop recording").clicked() {
                    send(app, Request::Stop);
                }
            } else if ui.button("Start recording").clicked() {
                send(app, Request::Record);
            }
        });
    });
}

fn send(app: &mut ScApp, req: Request) {
    match daemon::request(req) {
        Ok(Response::Saved { path, .. }) => {
            app.toast(format!("saved {}", path.display()), theme::OK)
        }
        Ok(Response::Error { message }) => app.toast(message, theme::WARN),
        Ok(_) => {}
        Err(e) => app.toast(e.to_string(), theme::DANGER),
    }
}
