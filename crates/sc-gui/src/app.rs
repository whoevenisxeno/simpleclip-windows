use crate::daemon::Link;
use crate::theme::{self, icon};
use crate::{config_io, daemon, screens, widgets};
use egui::{vec2, Align2, Color32, FontId, RichText};
use sc_core::config::Config;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(PartialEq, Clone)]
pub enum Screen {
    Wizard,
    Dashboard,
    Gallery,
    Trim(PathBuf),
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
pub enum ToastKind {
    Ok,
    Warn,
    Error,
}

pub struct Toast {
    text: String,
    kind: ToastKind,
    born: Instant,
}

/// Settings are saved as you change them; this tracks that.
#[derive(Default)]
pub struct AutoSave {
    dirty_since: Option<Instant>,
    pub saved_at: Option<Instant>,
    pub error: Option<String>,
    attempted: Option<Config>,
}

pub struct ScApp {
    pub cfg: Config,
    /// What is on disk, to spot unsaved changes.
    saved_cfg: Config,
    pub autosave: AutoSave,
    pub link: Link,
    pub link_rx: crossbeam_channel::Receiver<Link>,
    pub screen: Screen,
    pub wizard: screens::wizard::WizardState,
    pub toast: Option<Toast>,
    /// Displays and audio outputs reported by the daemon (empty until fetched).
    pub devices: Devices,
    pub gallery: screens::gallery::Cache,
    pub trim: screens::trim::TrimState,
    pub thumbs: crate::thumbs::Thumbs,
    shot: Option<Shot>,
}

#[derive(Default)]
pub struct Devices {
    pub monitors: Vec<sc_core::capture::MonitorInfo>,
    pub audio: Vec<sc_core::audio::AudioDevice>,
    pub loaded: bool,
}

impl Devices {
    /// Ask the daemon for the current displays and audio devices.
    pub fn refresh(&mut self) {
        use sc_core::ipc::{Request, Response};
        if let Ok(Response::Monitors { monitors: m }) = daemon::request(Request::ListMonitors) {
            self.monitors = m;
        }
        if let Ok(Response::AudioDevices { devices: a }) =
            daemon::request(Request::ListAudioDevices)
        {
            self.audio = a;
        }
        self.loaded = true;
    }

    /// Fetch once the daemon is reachable.
    pub fn ensure(&mut self, link: &Link) {
        if !self.loaded && matches!(link, Link::Connected(_)) {
            self.refresh();
        }
    }
}

impl ScApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let (tx, rx) = crossbeam_channel::unbounded();
        // Bring the daemon up if it isn't already (it outlives this window).
        std::thread::spawn(daemon::ensure_running);
        daemon::spawn_status_poller(cc.egui_ctx.clone(), tx);
        let cfg = config_io::load();
        // Show the wizard only until it has been completed once.
        let screen = if cfg.general.setup_complete {
            Screen::Dashboard
        } else {
            Screen::Wizard
        };
        Self {
            saved_cfg: cfg.clone(),
            cfg,
            autosave: AutoSave::default(),
            link: Link::Disconnected,
            link_rx: rx,
            screen,
            wizard: Default::default(),
            toast: None,
            devices: Devices::default(),
            gallery: Default::default(),
            trim: Default::default(),
            thumbs: crate::thumbs::Thumbs::new(cc.egui_ctx.clone()),
            shot: Shot::from_env(),
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            born: Instant::now(),
        });
    }

    pub fn goto(&mut self, screen: Screen) {
        self.flush_config();
        self.screen = screen;
    }

    /// Write any unsaved settings now (used when leaving the Settings page).
    pub fn flush_config(&mut self) {
        if self.cfg != self.saved_cfg {
            self.save_now();
        }
    }

    fn save_now(&mut self) {
        match config_io::save(&self.cfg) {
            Ok(()) => {
                self.saved_cfg = self.cfg.clone();
                self.autosave.saved_at = Some(Instant::now());
                self.autosave.error = None;
            }
            Err(e) => self.autosave.error = Some(e),
        }
        self.autosave.dirty_since = None;
        self.autosave.attempted = Some(self.cfg.clone());
    }

    /// Settings page: save shortly after the last change, unless it's invalid.
    fn autosave(&mut self, ctx: &egui::Context) {
        if self.screen != Screen::Settings || self.cfg == self.saved_cfg {
            self.autosave.dirty_since = None;
            return;
        }
        if self.autosave.attempted.as_ref() == Some(&self.cfg) {
            return; // already tried this exact state and it failed validation
        }
        let since = *self.autosave.dirty_since.get_or_insert_with(Instant::now);
        if since.elapsed() > Duration::from_millis(600) {
            self.save_now();
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl eframe::App for ScApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(link) = self.link_rx.try_recv() {
            self.link = link;
        }
        self.thumbs.poll();
        self.autosave(ctx);

        if self.screen == Screen::Wizard {
            egui::CentralPanel::default().show(ctx, |ui| screens::wizard::view(self, ui));
        } else {
            let p = theme::pal(ctx);
            egui::SidePanel::left("nav")
                .exact_width(216.0)
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(p.sidebar)
                        .inner_margin(egui::Margin::symmetric(14.0, 18.0))
                        .stroke(egui::Stroke::new(1.0_f32, p.line)),
                )
                .show(ctx, |ui| self.sidebar(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(p.bg))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Comfortable reading width, centred when the window is wide.
                            let avail = ui.available_width();
                            let content = avail.min(980.0) - 56.0;
                            let side = ((avail - content) / 2.0).max(28.0);
                            ui.horizontal_top(|ui| {
                                ui.add_space(side);
                                ui.vertical(|ui| {
                                    ui.set_width(content.max(320.0));
                                    ui.add_space(20.0);
                                    match self.screen.clone() {
                                        Screen::Dashboard => screens::dashboard::view(self, ui),
                                        Screen::Gallery => screens::gallery::view(self, ui),
                                        Screen::Trim(path) => screens::trim::view(self, ui, &path),
                                        Screen::Settings => screens::settings::view(self, ui),
                                        Screen::Wizard => {}
                                    }
                                    ui.add_space(32.0);
                                });
                            });
                        });
                });
        }
        self.show_toast(ctx);
        self.drive_shot(ctx);
    }
}

impl ScApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal_ui(ui);
        // App mark and name.
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), egui::Sense::hover());
            ui.painter().rect_filled(
                rect,
                egui::Rounding::same(7.0),
                Color32::from_rgb(0x1f, 0x21, 0x2b),
            );
            ui.painter().circle_stroke(
                rect.center(),
                7.0,
                egui::Stroke::new(2.0_f32, Color32::from_rgb(0xee, 0xf0, 0xf6)),
            );
            ui.painter()
                .circle_filled(rect.center(), 3.6, Color32::from_rgb(0xe5, 0x39, 0x35));
            ui.label(
                RichText::new("SimpleClip")
                    .font(FontId::new(17.0, theme::semibold()))
                    .color(p.text),
            );
        });
        ui.add_space(18.0);

        ui.spacing_mut().item_spacing.y = 4.0;
        let items = [
            (Screen::Dashboard, icon::HOME, "Home"),
            (Screen::Gallery, icon::CLIPS, "Clips"),
            (Screen::Settings, icon::SETTINGS, "Settings"),
        ];
        for (screen, glyph, label) in items {
            let selected = self.screen == screen
                || matches!((&self.screen, &screen), (Screen::Trim(_), Screen::Gallery));
            if widgets::nav_item(ui, glyph, label, selected).clicked() {
                self.goto(screen);
            }
        }

        // Push the status block and guide link to the bottom: spend the remaining
        // height on a spacer, leaving exactly what they need.
        const FOOT: f32 = 98.0;
        ui.add_space((ui.available_height() - FOOT).max(8.0));
        self.status_block(ui);
        ui.add_space(6.0);
        if ui
            .add(
                egui::Label::new(RichText::new("Setup guide").size(12.5).color(p.text_2))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
        {
            self.wizard = Default::default();
            self.goto(Screen::Wizard);
        }
    }

    /// Live state at the foot of the sidebar: a dot, a word, and the buffer.
    fn status_block(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal_ui(ui);
        let (color, word, sub) = match &self.link {
            Link::Connected(s) => {
                use sc_core::capture::CaptureState::*;
                let (c, w) = match s.state {
                    Active => (p.green, "Recording"),
                    Paused => (p.orange, "Paused"),
                    NeedsConsent => (p.orange, "Needs permission"),
                    Stopped => (p.text_3, "Stopped"),
                };
                (
                    c,
                    w,
                    format!(
                        "{}s buffer \u{2022} {:.0}% full",
                        s.buffer_secs,
                        s.buffer_fill * 100.0
                    ),
                )
            }
            Link::Disconnected => (p.text_3, "Not running", "Start from Home".to_string()),
        };
        egui::Frame::none()
            .fill(p.card)
            .rounding(egui::Rounding::same(10.0))
            .stroke(egui::Stroke::new(1.0_f32, p.line))
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.horizontal(|ui| {
                        widgets::dot(ui, color);
                        ui.label(RichText::new(word).font(FontId::new(13.5, theme::semibold())));
                    });
                    ui.label(RichText::new(sub).size(12.0).color(p.text_2));
                });
            });
    }

    fn show_toast(&mut self, ctx: &egui::Context) {
        let Some(t) = &self.toast else { return };
        if t.born.elapsed() > Duration::from_secs(4) {
            self.toast = None;
            return;
        }
        let p = theme::pal(ctx);
        let color = match t.kind {
            ToastKind::Ok => p.green,
            ToastKind::Warn => p.orange,
            ToastKind::Error => p.red,
        };
        let glyph = match t.kind {
            ToastKind::Ok => "\u{2713}",
            ToastKind::Warn => "!",
            ToastKind::Error => "\u{2715}",
        };
        let text = t.text.clone();
        egui::Area::new(egui::Id::new("toast"))
            .anchor(Align2::CENTER_BOTTOM, vec2(0.0, -22.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(if p.dark {
                        Color32::from_rgb(0x3a, 0x3a, 0x3c)
                    } else {
                        Color32::from_rgb(0x1d, 0x1d, 0x1f)
                    })
                    .rounding(egui::Rounding::same(14.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 12.0))
                    .shadow(egui::Shadow {
                        offset: vec2(0.0, 6.0),
                        blur: 24.0,
                        spread: 0.0,
                        color: Color32::from_black_alpha(90),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(glyph).color(color).strong());
                            ui.label(RichText::new(text).color(Color32::WHITE).size(14.0));
                        });
                    });
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

// ------------------------------------------------- screenshot test harness --

/// With `SC_GUI_SHOTS=<dir>` the app walks through its screens in dark and light
/// mode, saves a PNG of each, and exits. Used to review the design; hidden.
struct Shot {
    dir: PathBuf,
    step: usize,
    waited: u32,
    requested: bool,
}

const SHOT_PLAN: &[(&str, bool)] = &[
    ("home", true),
    ("clips", true),
    ("settings", true),
    ("trim", true),
    ("wizard1", true),
    ("wizard2", true),
    ("wizard3", true),
    ("home", false),
    ("clips", false),
    ("settings", false),
    ("wizard2", false),
];

impl Shot {
    fn from_env() -> Option<Self> {
        let dir = PathBuf::from(std::env::var_os("SC_GUI_SHOTS")?);
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self {
            dir,
            step: 0,
            waited: 0,
            requested: false,
        })
    }
}

impl ScApp {
    fn drive_shot(&mut self, ctx: &egui::Context) {
        let Some(mut shot) = self.shot.take() else {
            return;
        };
        ctx.request_repaint();
        // Save any finished screenshot and move on.
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let (Some(img), true) = (image, shot.requested) {
            let (name, dark) = SHOT_PLAN[shot.step];
            let path = shot.dir.join(format!(
                "{}-{}.png",
                name,
                if dark { "dark" } else { "light" }
            ));
            if let Ok(file) = std::fs::File::create(&path) {
                let mut enc = png::Encoder::new(file, img.width() as u32, img.height() as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                if let Ok(mut w) = enc.write_header() {
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                    let _ = w.write_image_data(&bytes);
                }
            }
            shot.step += 1;
            shot.waited = 0;
            shot.requested = false;
        }
        if shot.step >= SHOT_PLAN.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let (name, dark) = SHOT_PLAN[shot.step];
        if shot.waited == 0 {
            ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
            // Never let the review run write to the real config.
            self.cfg.general.setup_complete = true;
            self.saved_cfg = self.cfg.clone();
            self.wizard.step = match name {
                "wizard1" => 0,
                "wizard2" => 1,
                "wizard3" => 2,
                _ => self.wizard.step,
            };
            self.screen = match name {
                "home" => Screen::Dashboard,
                "clips" => Screen::Gallery,
                "settings" => Screen::Settings,
                "trim" => {
                    let first = self.gallery.first_video();
                    first.map(Screen::Trim).unwrap_or(Screen::Gallery)
                }
                _ => Screen::Wizard,
            };
        }
        shot.waited += 1;
        // Wait for previews to settle (including retries) before capturing.
        let settled = !self.thumbs.pending() || shot.waited > 1500;
        if shot.waited >= 45 && settled && !shot.requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            shot.requested = true;
        }
        self.shot = Some(shot);
    }
}
