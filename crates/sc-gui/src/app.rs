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
    /// Bookmarks and notes for your clips.
    pub library: crate::library::Library,
    /// The open details / bookmark / new-folder sheet, if any.
    pub sheet: screens::sheet::SheetState,
    /// The occasional "star the project" card.
    pub star: crate::star::StarPrompt,
    /// The logo shown in the sidebar, loaded on first use.
    logo: Option<egui::TextureHandle>,
    /// The theme and accent last applied, to notice a change in Settings or the setup guide.
    applied_look: (String, Option<String>),
    /// The user's own saved presets (the built-in ones live in `presets`).
    pub presets: Vec<crate::presets::Preset>,
    /// The "share this clip?" prompt after a trim, when open.
    pub share: Option<crate::screens::share::ShareState>,
    /// Looks for newer releases and installs them.
    #[cfg(windows)]
    pub updater: crate::update::Updater,
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
        let shot = Shot::from_env();
        if let Some(s) = &shot {
            // The design-review run uses its own throwaway library.
            std::env::set_var("SC_LIBRARY", s.dir.join("library.json"));
        }
        let mut cfg = config_io::load();
        if let Some(s) = &shot {
            // ...and its own clips folder, so review never touches yours.
            cfg.save.directory = Some(s.dir.join("clips"));
            // Show the defaults rather than this PC's particular devices.
            cfg.capture.monitor_id.clear();
            cfg.audio.desktop_device.clear();
            cfg.audio.mic_device = None;
            cfg.hotkeys = Default::default();
            cfg.audio.mic_enabled = true;
            cfg.general.setup_complete = true;
            // SC_GUI_THEME lets a review run render any theme.
            if let Ok(t) = std::env::var("SC_GUI_THEME") {
                cfg.general.theme = t;
            }
        }
        // The chosen colour theme (the default follows Windows light and dark).
        crate::themes::set_theme(
            &cc.egui_ctx,
            &cfg.general.theme,
            cfg.general.accent.as_deref(),
        );
        let applied_look = (cfg.general.theme.clone(), cfg.general.accent.clone());
        // After an update, say so once. (Not during the design-review run.)
        let mut updated_from = None;
        #[cfg(windows)]
        if shot.is_none() {
            std::thread::spawn(crate::update::clean_old_downloads);
        }
        if shot.is_none() && cfg.general.last_version.as_deref() != Some(env!("CARGO_PKG_VERSION"))
        {
            updated_from = cfg
                .general
                .last_version
                .clone()
                .or_else(|| cfg.general.setup_complete.then(String::new));
            cfg.general.last_version = Some(env!("CARGO_PKG_VERSION").to_string());
            let _ = config_io::save(&cfg);
        }
        // Show the wizard only until it has been completed once.
        let screen = if cfg.general.setup_complete {
            Screen::Dashboard
        } else {
            Screen::Wizard
        };
        let mut app = Self {
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
            library: crate::library::Library::load(),
            sheet: Default::default(),
            star: Default::default(),
            logo: None,
            applied_look,
            presets: crate::presets::load(),
            share: None,
            #[cfg(windows)]
            updater: Default::default(),
            shot,
        };
        if updated_from.is_some() {
            app.toast(
                format!("SimpleClip updated to {}", env!("CARGO_PKG_VERSION")),
                ToastKind::Ok,
            );
        }
        app
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
        if !matches!(screen, Screen::Trim(_)) {
            self.trim.stop_player();
        }
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
        // Apply a theme or accent change as soon as it is made.
        let look = (
            self.cfg.general.theme.clone(),
            self.cfg.general.accent.clone(),
        );
        if look != self.applied_look {
            crate::themes::set_theme(ctx, &look.0, look.1.as_deref());
            self.applied_look = look;
        }
        while let Ok(link) = self.link_rx.try_recv() {
            // The daemon came back (it restarted, or was updating): retry failed previews now.
            if matches!(self.link, Link::Disconnected) && matches!(link, Link::Connected(_)) {
                self.thumbs.retry_failed();
            }
            self.link = link;
        }
        self.thumbs.poll();
        // A minimized window shows nothing, so give the preview memory back.
        if ctx.input(|i| i.viewport().minimized) == Some(true) {
            self.thumbs.trim_to(0);
        }
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
        #[cfg(windows)]
        self.updater.show(
            ctx,
            self.cfg.general.update_check && self.shot.is_none(),
            self.screen == Screen::Wizard,
        );
        screens::sheet::show(self, ctx);
        screens::share::show(self, ctx);
        #[cfg(windows)]
        let update_open = self.updater.open;
        #[cfg(not(windows))]
        let update_open = false;
        let star_busy = self.screen == Screen::Wizard || update_open;
        // The design-review run never touches the settings, so it only draws a forced card.
        if self.shot.is_none() || self.star.is_showing() {
            crate::star::show(self, ctx, star_busy);
        }
        // Bookmarks and notes save a moment after the last edit.
        self.library.flush_if_due();
        if self.library.is_dirty() {
            ctx.request_repaint_after(Duration::from_millis(600));
        }
        self.show_toast(ctx);
        self.drive_shot(ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let _ = self.library.save();
        self.flush_config();
    }
}

impl ScApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal_ui(ui);
        // App mark and name.
        ui.horizontal(|ui| {
            // The app logo (assets/icon.svg, rendered by assets/make-icon.ps1).
            let logo = self.logo.get_or_insert_with(|| {
                let img =
                    crate::thumbs::decode_png_bytes(include_bytes!("../../../assets/icon-64.png"))
                        .unwrap_or_else(|| egui::ColorImage::new([1, 1], Color32::TRANSPARENT));
                ui.ctx()
                    .load_texture("logo", img, egui::TextureOptions::LINEAR)
            });
            ui.add(egui::Image::new(egui::load::SizedTexture::new(
                logo.id(),
                vec2(30.0, 30.0),
            )));
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
    ("clips-bookmarks", true),
    ("clips-folders", true),
    ("clips-trimmed", true),
    ("clips-compressed", true),
    ("details", true),
    ("bookmarks", true),
    ("newfolder", true),
    ("trim", true),
    ("player", true),
    ("wizard1", true),
    ("wizard2", true),
    ("wizard3", true),
    ("wizard4", true),
    ("settings", true),
    ("home", false),
    ("clips", false),
    ("details", false),
    ("trim", false),
    // These leave a dialog on screen, so they come last.
    ("update", true),
    ("uninstall", true),
    ("update-download", true),
    ("update-forced", true),
    ("star", true),
    ("share", true),
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
    /// Sample bookmarks and notes (in the throwaway review library) so those
    /// screens have something to show.
    fn shot_demo_data(&mut self) {
        if !self.library.bookmarks.is_empty() {
            return;
        }
        let colors = crate::library::COLORS;
        let clutch = self.library.add_bookmark("Clutch", colors[3]);
        let funny = self.library.add_bookmark("Funny", colors[2]);
        let fail = self.library.add_bookmark("Fail", colors[0]);
        let paths: Vec<_> = self
            .gallery
            .all_paths()
            .into_iter()
            .filter(|p| !self.library.is_trimmed(p))
            .collect();
        if let Some(p) = paths.first() {
            self.library.toggle_bookmark(p, clutch);
            self.library.toggle_bookmark(p, funny);
            self.library
                .set_note(p, "1v3 on B site. Send this one to the group chat.");
        }
        if let Some(p) = paths.get(1) {
            self.library.toggle_bookmark(p, fail);
        }
        if let Some(p) = paths.get(2) {
            self.library.toggle_bookmark(p, funny);
            self.library.set_note(p, "Lag spike right before the flick");
        }
    }

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
            let (ready, failed, loading) = self.thumbs.stats();
            let _ = std::fs::write(
                shot.dir.join("stats.txt"),
                format!(
                    "previews ready={ready} failed={failed} loading={loading}
"
                ),
            );
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
                "wizard4" => 3,
                _ => self.wizard.step,
            };
            self.shot_demo_data();
            self.sheet = Default::default();
            self.gallery.reset_view();
            self.screen = match name {
                "home" => Screen::Dashboard,
                "clips" | "clips-bookmarks" | "clips-folders" | "clips-trimmed"
                | "clips-compressed" | "share" | "details" | "bookmarks" | "newfolder" => {
                    Screen::Gallery
                }
                "update" => Screen::Dashboard,
                "uninstall" => Screen::Settings,
                "update-download" | "update-forced" => Screen::Dashboard,
                "star" => Screen::Dashboard,
                "settings" => Screen::Settings,
                "trim" | "player" => {
                    let first = self.gallery.first_video();
                    first.map(Screen::Trim).unwrap_or(Screen::Gallery)
                }
                _ => Screen::Wizard,
            };
            match name {
                "clips-bookmarks" => self.gallery.show_by(false),
                "clips-trimmed" => self.gallery.show_trimmed(),
                "clips-compressed" => self.gallery.show_compressed(),
                "share" => {
                    if let Some(p) = self.gallery.first_video() {
                        self.share = Some(screens::share::ShareState::demo(p));
                    }
                }
                "clips-folders" => self.gallery.show_by(true),
                "details" => {
                    let first = self
                        .gallery
                        .all_paths()
                        .into_iter()
                        .find(|p| !self.library.is_trimmed(p));
                    if let Some(p) = first {
                        self.sheet.open_clip(p);
                    }
                }
                "bookmarks" => self.sheet.manage = true,
                "uninstall" => {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("uninstall-ask"), true))
                }
                "star" => self.star.force(),
                "update-download" => {
                    #[cfg(windows)]
                    self.updater.demo_download("4.9.0");
                }
                "update-forced" => {
                    #[cfg(windows)]
                    self.updater.demo_download("9.0.0");
                }
                "update" => {
                    #[cfg(windows)]
                    self.updater.demo();
                }
                "newfolder" => {
                    self.sheet.folder_dialog = Some(screens::sheet::FolderDialog {
                        name: "Best of".into(),
                        move_clip: None,
                    })
                }
                _ => {}
            }
        }
        shot.waited += 1;
        // Pick a selection (as if dragging a handle), then let it play.
        if name == "player" && shot.waited == 40 {
            self.trim.demo_select(3.0, 11.0);
        }
        if name == "player" && shot.waited == 70 {
            self.trim.demo_play();
        }
        if shot.step == 0 && shot.waited == 1 {
            // A taller window so long pages (trim, settings) fit in one picture.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                1080.0,
                std::env::var("SC_GUI_SHOT_HEIGHT")
                    .ok()
                    .and_then(|h| h.parse().ok())
                    .unwrap_or(900.0),
            )));
        }
        // Wait for previews to settle (including retries) before capturing.
        let settled = !self.thumbs.pending() || shot.waited > 1500;
        if shot.waited
            >= (if name == "trim" {
                220
            } else if name == "player" {
                140
            } else {
                45
            })
            && settled
            && !shot.requested
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            shot.requested = true;
        }
        self.shot = Some(shot);
    }
}
