//! Checks GitHub for a newer release, shows its changelog, and installs it on
//! request. Network access goes through `curl.exe`, which ships with Windows 10
//! and 11, so the app needs no HTTP library of its own.
//!
//! The changelog shown is the release's notes, so every release must carry its
//! `CHANGELOG.md` entry as its notes.

use crate::theme::{self, pal};
use crate::widgets;
use egui::{Align2, Color32, FontId, RichText, Rounding, Stroke};
use sc_core::update::curl;
pub use sc_core::update::Release;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The version this build is.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const PAGE: &str = "https://github.com/whoevenisxeno/simpleclip-windows/releases/latest";
/// How often a running app looks again.
const RECHECK: Duration = Duration::from_secs(6 * 3600);

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release),
    /// The installer is downloaded; the app should hand over to it and close.
    Ready(PathBuf),
    Failed(String),
}

pub struct Updater {
    state: Arc<Mutex<State>>,
    checked: Option<Instant>,
    started: Instant,
    /// Whether the details window is open.
    pub open: bool,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Idle)),
            checked: None,
            started: Instant::now(),
            open: false,
        }
    }
}

impl Updater {
    fn set(state: &Arc<Mutex<State>>, s: State, ctx: &egui::Context) {
        if let Ok(mut g) = state.lock() {
            *g = s;
        }
        ctx.request_repaint();
    }

    pub fn state(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or(State::Idle)
    }

    /// Look for a newer release in the background.
    pub fn check(&mut self, ctx: &egui::Context) {
        if matches!(self.state(), State::Checking | State::Downloading(_)) {
            return;
        }
        self.checked = Some(Instant::now());
        Self::set(&self.state, State::Checking, ctx);
        let (state, ctx) = (self.state.clone(), ctx.clone());
        std::thread::spawn(move || {
            let next = match sc_core::update::fetch_latest(CURRENT) {
                Ok(Some(r)) => State::Available(r),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(e),
            };
            Self::set(&state, next, &ctx);
        });
    }

    /// Download the installer in the background; the state becomes `Ready` when done.
    fn download(&mut self, r: Release, ctx: &egui::Context) {
        Self::set(&self.state, State::Downloading(r.clone()), ctx);
        let (state, ctx) = (self.state.clone(), ctx.clone());
        std::thread::spawn(move || {
            let dest = std::env::temp_dir().join(format!("SimpleClip-{}-Setup.exe", r.version));
            let _ = std::fs::remove_file(&dest);
            let done = curl(&["--retry", "2", "-o", &dest.to_string_lossy(), &r.url])
                .and_then(|_| {
                    let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                    if got == 0 || (r.size > 0 && got != r.size) {
                        Err("the download was incomplete".to_string())
                    } else {
                        Ok(())
                    }
                })
                .map(|_| dest);
            Self::set(
                &state,
                match done {
                    Ok(p) => State::Ready(p),
                    Err(e) => State::Failed(e),
                },
                &ctx,
            );
        });
    }

    /// Run the downloaded installer after this app has closed, then reopen the
    /// daemon and the app from the install folder.
    fn hand_over(setup: &std::path::Path) {
        use std::os::windows::process::CommandExt;
        let dir = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.to_path_buf()))
            .filter(|d| d.join("unins000.exe").exists())
            .or_else(|| {
                std::env::var_os("LOCALAPPDATA")
                    .map(|l| PathBuf::from(l).join("Programs").join("SimpleClip"))
            })
            .unwrap_or_default();
        let script = format!(
            "ping -n 3 127.0.0.1 >nul & \"{}\" /SILENT /NORESTART /SUPPRESSMSGBOXES & start \"\" \"{}\" & start \"\" \"{}\"",
            setup.display(),
            dir.join("scd.exe").display(),
            dir.join("sc-gui.exe").display()
        );
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .raw_arg(script)
            .creation_flags(0x0800_0008) // detached, no console window
            .spawn();
    }

    /// The uninstaller of this installed copy, if this is one (not for a portable folder).
    pub fn uninstall_exe() -> Option<PathBuf> {
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.join("unins000.exe")))
            .filter(|p| p.exists())
    }

    /// Run the uninstaller after this app has closed (it can not replace files in use).
    pub fn start_uninstall(exe: &std::path::Path) {
        use std::os::windows::process::CommandExt;
        let script = format!("ping -n 3 127.0.0.1 >nul & \"{}\"", exe.display());
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .raw_arg(script)
            .creation_flags(0x0800_0008)
            .spawn();
    }

    /// A one-line summary for the Settings page.
    pub fn status_text(&self) -> String {
        match self.state() {
            State::Idle => format!("Version {CURRENT}"),
            State::Checking => "Checking\u{2026}".into(),
            State::UpToDate => format!("Version {CURRENT} is the latest"),
            State::Available(r) | State::Downloading(r) => {
                format!("Version {} is available", r.version)
            }
            State::Ready(_) => "Installing\u{2026}".into(),
            State::Failed(e) => format!("Couldn't check: {e}"),
        }
    }

    /// Pretend a newer release exists (for the design-review run).
    pub fn demo(&mut self) {
        let notes = "## 4.2.0\n\n### Added\n- Check for updates from inside the app\n- Play every audio track in the preview player\n\n### Fixed\n- Seek bar jumped to the wrong time when dragged\n- Picture could freeze if the sound device stalled\n";
        if let Ok(mut s) = self.state.lock() {
            *s = State::Available(Release {
                version: "4.2.0".into(),
                notes: notes.into(),
                url: String::new(),
                size: 0,
            });
        }
        self.open = true;
    }

    /// Per-frame work: schedule checks, draw the top-right button and the details window.
    pub fn show(&mut self, ctx: &egui::Context, auto_check: bool, wizard: bool) {
        if auto_check
            && self.started.elapsed() > Duration::from_secs(3)
            && self.checked.is_none_or(|t| t.elapsed() > RECHECK)
        {
            self.check(ctx);
        }
        if let State::Ready(p) = self.state() {
            Self::hand_over(&p);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if wizard {
            return;
        }
        let state = self.state();
        let release = match &state {
            State::Available(r) | State::Downloading(r) => Some(r.clone()),
            _ => None,
        };
        let Some(r) = release else {
            self.open = false;
            return;
        };
        let p = pal(ctx);

        // The small button, top right.
        egui::Area::new(egui::Id::new("update-pill"))
            .anchor(Align2::RIGHT_TOP, [-18.0, 14.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let b = egui::Button::new(
                    RichText::new("Update available")
                        .size(12.5)
                        .color(Color32::WHITE)
                        .font(FontId::new(12.5, theme::semibold())),
                )
                .fill(p.accent)
                .rounding(Rounding::same(14.0))
                .min_size(egui::vec2(0.0, 28.0));
                if ui
                    .add(b)
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    self.open = !self.open;
                }
            });

        if !self.open {
            return;
        }
        let downloading = matches!(state, State::Downloading(_));
        let mut close = false;
        let mut install = false;
        egui::Window::new("update-window")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .auto_sized()
            .frame(
                egui::Frame::none()
                    .fill(p.card)
                    .rounding(Rounding::same(14.0))
                    .stroke(Stroke::new(1.0_f32, p.line))
                    .inner_margin(egui::Margin::same(20.0))
                    .shadow(egui::Shadow {
                        offset: egui::vec2(0.0, 8.0),
                        blur: 28.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(90),
                    }),
            )
            .show(ctx, |ui| {
                ui.set_width(460.0);
                ui.label(
                    RichText::new(format!("SimpleClip {} is available", r.version))
                        .font(FontId::new(19.0, theme::semibold()))
                        .color(p.text),
                );
                ui.add_space(2.0);
                widgets::secondary(ui, format!("You have {CURRENT}. Here is what changed:"));
                ui.add_space(10.0);
                egui::Frame::none()
                    .fill(p.card_hi)
                    .rounding(Rounding::same(10.0))
                    .inner_margin(egui::Margin::same(12.0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| changelog(ui, &r.notes));
                    });
                ui.add_space(14.0);
                if let State::Failed(e) = &state {
                    widgets::notice(ui, p.red, e);
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let label = if downloading {
                        "Downloading\u{2026}"
                    } else {
                        "Update now"
                    };
                    if widgets::primary_button(ui, None, label).clicked() && !downloading {
                        install = true;
                    }
                    if widgets::quiet_button(ui, None, "Later").clicked() {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.hyperlink_to("Release page", PAGE);
                    });
                });
                if downloading {
                    ui.add_space(6.0);
                    widgets::secondary(ui, "SimpleClip will restart by itself when it is done.");
                }
            });
        if close {
            self.open = false;
        }
        if install {
            self.download(r, ctx);
        }
    }
}

/// Show release notes (a little Markdown: headings and bullets).
fn changelog(ui: &mut egui::Ui, notes: &str) {
    let p = pal(ui.ctx());
    if notes.trim().is_empty() {
        widgets::secondary(ui, "No release notes were provided.");
        return;
    }
    for line in notes.lines() {
        let t = line.trim_end().replace("**", "").replace('`', "");
        if t.trim().is_empty() {
            ui.add_space(4.0);
        } else if let Some(h) = t.strip_prefix("### ") {
            ui.add_space(4.0);
            ui.label(
                RichText::new(h)
                    .font(FontId::new(13.0, theme::semibold()))
                    .color(p.text),
            );
        } else if let Some(h) = t.strip_prefix("## ") {
            ui.label(
                RichText::new(h)
                    .font(FontId::new(15.0, theme::semibold()))
                    .color(p.text),
            );
        } else if let Some(b) = t.trim_start().strip_prefix("- ") {
            ui.add(
                egui::Label::new(
                    RichText::new(format!("\u{2022}  {b}"))
                        .size(13.0)
                        .color(p.text_2),
                )
                .wrap(),
            );
        } else {
            ui.add(egui::Label::new(RichText::new(t).size(13.0).color(p.text_2)).wrap());
        }
    }
}
