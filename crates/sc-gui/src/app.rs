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
    Edit(PathBuf),
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

#[derive(Default)]
pub struct AutoSave {
    dirty_since: Option<Instant>,
    pub saved_at: Option<Instant>,
    pub error: Option<String>,
    attempted: Option<Config>,
}

pub struct ScApp {
    pub cfg: Config,
    saved_cfg: Config,
    pub autosave: AutoSave,
    pub link: Link,
    pub link_rx: crossbeam_channel::Receiver<Link>,
    pub screen: Screen,
    pub wizard: screens::wizard::WizardState,
    pub toast: Option<Toast>,
    pub devices: Devices,
    pub gallery: screens::gallery::Cache,
    pub editor: screens::editor::EditorState,
    pub settings: screens::settings::SettingsState,
    shown_opened: Option<PathBuf>,
    pub thumbs: crate::thumbs::Thumbs,
    pub library: crate::library::Library,
    pub sheet: screens::sheet::SheetState,
    pub star: crate::star::StarPrompt,
    logo: Option<egui::TextureHandle>,
    applied_look: (String, Option<String>),
    pub presets: Vec<crate::presets::Preset>,
    pub share: Option<crate::screens::share::ShareState>,
    #[cfg(windows)]
    pub updater: crate::update::Updater,
    shot: Option<Shot>,
}

#[derive(Default)]
pub struct Devices {
    pub monitors: Vec<sc_core::capture::MonitorInfo>,
    pub audio: Vec<sc_core::audio::AudioDevice>,
    pub loaded: bool,
    previews: std::collections::HashMap<String, egui::TextureHandle>,
    previewed_at: Option<Instant>,
}

impl Devices {
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
        self.previewed_at = None;
        self.loaded = true;
    }

    pub fn refresh_previews(&mut self, ctx: &egui::Context) {
        let stale = self
            .previewed_at
            .is_none_or(|t| t.elapsed() > Duration::from_secs(4));
        if !stale {
            return;
        }
        self.previewed_at = Some(Instant::now());
        let positions_known =
            self.monitors.len() < 2 || self.monitors.iter().any(|m| m.x != 0 || m.y != 0);
        if !positions_known {
            return;
        }
        #[cfg(windows)]
        for m in &self.monitors {
            let Some(img) =
                crate::screenshot::capture_region(m.x, m.y, m.width as i32, m.height as i32, 480)
            else {
                continue;
            };
            match self.previews.get_mut(&m.id) {
                Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                None => {
                    let tex = ctx.load_texture(
                        format!("monitor-{}", m.id),
                        img,
                        egui::TextureOptions::LINEAR,
                    );
                    self.previews.insert(m.id.clone(), tex);
                }
            }
        }
        #[cfg(not(windows))]
        let _ = ctx;
    }

    pub fn preview(&self, id: &str) -> Option<&egui::TextureHandle> {
        self.previews.get(id)
    }

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
        std::thread::spawn(daemon::ensure_running);
        daemon::spawn_status_poller(cc.egui_ctx.clone(), tx);
        let shot = Shot::from_env();
        if let Some(s) = &shot {
            std::env::set_var("SC_LIBRARY", s.dir.join("library.json"));
        }
        let mut cfg = config_io::load();
        if let Some(s) = &shot {
            cfg.save.directory = Some(s.dir.join("clips"));
            cfg.capture.monitor_id.clear();
            cfg.audio.desktop_device.clear();
            cfg.audio.mic_device = None;
            cfg.hotkeys = Default::default();
            cfg.events.enabled = true;
            cfg.hotkeys.extra = vec![
                sc_core::config::ExtraHotkey {
                    keys: "CTRL+F1".into(),
                    action: sc_core::config::HotkeyAction::SaveLast,
                    secs: 15,
                },
                sc_core::config::ExtraHotkey {
                    keys: "CTRL+F2".into(),
                    action: sc_core::config::HotkeyAction::TogglePause,
                    secs: 30,
                },
                sc_core::config::ExtraHotkey {
                    keys: String::new(),
                    action: sc_core::config::HotkeyAction::SaveLast,
                    secs: 120,
                },
            ];
            cfg.audio.mic_enabled = true;
            cfg.general.setup_complete = true;
            cfg.general.theme = "system".into();
            cfg.general.accent = None;
            cfg.general.discord_presence = true;
            cfg.buffer.replay_duration_secs = 60;
            if let Ok(t) = std::env::var("SC_GUI_THEME") {
                cfg.general.theme = t;
            }
        }
        crate::themes::set_theme(
            &cc.egui_ctx,
            &cfg.general.theme,
            cfg.general.accent.as_deref(),
        );
        let applied_look = (cfg.general.theme.clone(), cfg.general.accent.clone());
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
            editor: Default::default(),
            settings: Default::default(),
            shown_opened: None,
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
        #[cfg(windows)]
        if app.shot.is_some() {
            app.settings.addon.demo(true);
        }
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
        if let Screen::Edit(path) = &screen {
            self.gallery.opened = Some(path.clone());
        }
        self.flush_config();
        if screen != Screen::Settings {
            self.settings.query.clear();
            self.settings.dupes = None;
        }
        if !matches!(screen, Screen::Edit(_)) {
            self.editor.stop_player();
        }
        self.screen = screen;
    }

    pub fn settings_dirty(&self) -> bool {
        self.cfg != self.saved_cfg
    }

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

    fn autosave(&mut self, ctx: &egui::Context) {
        if self.screen != Screen::Settings || self.cfg == self.saved_cfg {
            self.autosave.dirty_since = None;
            return;
        }
        if self.autosave.attempted.as_ref() == Some(&self.cfg) {
            return;
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
        if self.screen != Screen::Settings {
            widgets::set_search_query(ctx, "");
        }
        let look = (
            self.cfg.general.theme.clone(),
            self.cfg.general.accent.clone(),
        );
        if look != self.applied_look {
            crate::themes::set_theme(ctx, &look.0, look.1.as_deref());
            self.applied_look = look;
        }
        while let Ok(link) = self.link_rx.try_recv() {
            if matches!(self.link, Link::Disconnected) && matches!(link, Link::Connected(_)) {
                self.thumbs.retry_failed();
            }
            self.link = link;
        }
        self.thumbs.poll();
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
                    if self.screen == Screen::Settings {
                        let avail = ui.available_width();
                        let content = avail.min(1040.0) - 56.0;
                        let side = ((avail - content) / 2.0).max(28.0);
                        ui.horizontal_top(|ui| {
                            ui.add_space(side);
                            ui.vertical(|ui| {
                                ui.set_width(content.max(320.0));
                                ui.add_space(20.0);
                                screens::settings::view(self, ui);
                            });
                        });
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
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
                                        Screen::Edit(path) => {
                                            screens::editor::view(self, ui, &path)
                                        }
                                        Screen::Settings | Screen::Wizard => {}
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
        if let Some((text, ok)) = self.updater.take_notice() {
            self.toast(text, if ok { ToastKind::Ok } else { ToastKind::Warn });
        }
        screens::sheet::show(self, ctx);
        screens::share::show(self, ctx);
        #[cfg(windows)]
        let update_open = self.updater.open;
        #[cfg(not(windows))]
        let update_open = false;
        let star_busy = self.screen == Screen::Wizard || update_open;
        if self.shot.is_none() || self.star.is_showing() {
            crate::star::show(self, ctx, star_busy);
        }
        self.library.flush_if_due();
        if self.library.is_dirty() {
            ctx.request_repaint_after(Duration::from_millis(600));
        }
        if self.gallery.opened != self.shown_opened {
            self.shown_opened = self.gallery.opened.clone();
            ctx.request_repaint();
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
        ui.horizontal(|ui| {
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
        let clip_count = self.gallery.originals_count(&self.library);
        for (screen, glyph, label) in items {
            let selected = self.screen == screen
                || matches!((&self.screen, &screen), (Screen::Edit(_), Screen::Gallery));
            let badge =
                (screen == Screen::Gallery && clip_count > 0).then(|| clip_count.to_string());
            if widgets::nav_item(ui, glyph, label, selected, badge.as_deref()).clicked() {
                self.goto(screen);
            }
        }

        const FOOT: f32 = 116.0;
        ui.add_space((ui.available_height() - FOOT).max(8.0));
        self.status_block(ui);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(12.0)
                        .color(p.text_3),
                );
                let busy = self.updater.checking();
                let tip = if busy {
                    "Checking for updates\u{2026}"
                } else {
                    "Check for updates"
                };
                if widgets::mini_icon_button(ui, icon::REFRESH, "Check", tip, busy).clicked() {
                    self.updater.check_now(ui.ctx());
                }
            });
        });
    }

    fn status_block(&mut self, ui: &mut egui::Ui) {
        let p = theme::pal_ui(ui);
        let (color, word, sub, fill) = match &self.link {
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
                    Some(s.buffer_fill.clamp(0.0, 1.0)),
                )
            }
            Link::Disconnected => (p.text_3, "Not running", "Start from Home".to_string(), None),
        };
        egui::Frame::none()
            .fill(p.card)
            .rounding(egui::Rounding::same(theme::radius::TILE))
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
                    if let Some(f) = fill {
                        ui.add_space(4.0);
                        let (bar, _) = ui.allocate_exact_size(
                            vec2(ui.available_width(), 4.0),
                            egui::Sense::hover(),
                        );
                        ui.painter()
                            .rect_filled(bar, egui::Rounding::same(2.0), p.card_hi);
                        let mut done = bar;
                        done.set_width((bar.width() * f).max(4.0));
                        ui.painter().rect_filled(
                            done,
                            egui::Rounding::same(2.0),
                            if color == p.orange {
                                p.orange
                            } else {
                                p.accent
                            },
                        );
                    }
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
                    .rounding(egui::Rounding::same(theme::radius::CARD))
                    .inner_margin(egui::Margin::symmetric(18.0, 12.0))
                    .shadow(theme::float_shadow(&p))
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
    ("editor", true),
    ("editor-crop", true),
    ("editor-rotate", true),
    ("editor-fades", true),
    ("editor-sound", true),
    ("editor-save", true),
    ("wizard1", true),
    ("wizard2", true),
    ("wizard3", true),
    ("wizard4", true),
    ("settings", true),
    ("settings-sound", true),
    ("settings-quality", true),
    ("settings-storage", true),
    ("settings-hotkeys", true),
    ("settings-card", true),
    ("settings-look", true),
    ("settings-system", true),
    ("settings-addon", true),
    ("settings-addon-on", true),
    ("settings-search", true),
    ("settings-dupes", true),
    ("look-accent", true),
    ("look-sand", true),
    ("settings-light", false),
    ("home", false),
    ("clips", false),
    ("details", false),
    ("editor", false),
    ("share", true),
    ("update", true),
    ("uninstall", true),
    ("update-download", true),
    ("update-forced", true),
    ("star", true),
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
        let all = self.gallery.all_paths();
        if let Some(p) = all.get(5) {
            self.library.set_trimmed(p, true);
        }
        if let Some(p) = all.get(6) {
            self.library.set_compressed(p, true);
        }
    }

    fn drive_shot(&mut self, ctx: &egui::Context) {
        let Some(mut shot) = self.shot.take() else {
            return;
        };
        ctx.request_repaint();
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
            self.cfg.general.setup_complete = true;
            let (shot_theme, shot_accent) = match name {
                "look-accent" => ("ember".to_string(), Some("#e84576".to_string())),
                "look-sand" => ("sand".to_string(), Some("#0b9cbd".to_string())),
                _ => (
                    std::env::var("SC_GUI_THEME").unwrap_or_else(|_| "system".into()),
                    None,
                ),
            };
            self.cfg.general.theme = shot_theme;
            self.cfg.general.accent = shot_accent;
            self.saved_cfg = self.cfg.clone();
            self.settings = Default::default();
            #[cfg(windows)]
            self.settings.addon.demo(name != "settings-addon");
            {
                use screens::settings::Tab;
                match name {
                    "settings-sound" => self.settings.tab = Tab::Sound,
                    "settings-quality" => self.settings.tab = Tab::Quality,
                    "settings-storage" => self.settings.tab = Tab::Storage,
                    "settings-dupes" => {
                        self.settings.tab = Tab::Storage;
                        let demo = |name: &str, mb: u64| crate::dupes::Item {
                            path: std::path::PathBuf::from(format!("{name}.mp4")),
                            bytes: mb * 1_048_576,
                        };
                        let found = crate::dupes::plan(vec![
                            demo("sc-apex-20261026-193000", 52),
                            demo("sc-apex-20261026-193000-trim", 14),
                            demo("sc-apex-20261026-193000-trim-10mb", 9),
                            demo("sc-valorant-20261027-211500", 61),
                            demo("sc-valorant-20261027-211500-25mb", 24),
                            demo("sc-cs2-20261026-180000", 48),
                        ]);
                        self.settings.dupes = Some(found);
                    }
                    "settings-hotkeys" => self.settings.tab = Tab::Hotkeys,
                    "settings-card" => self.settings.tab = Tab::SaveCard,
                    "settings-look" | "look-accent" | "look-sand" => self.settings.tab = Tab::Look,
                    "settings-system" => self.settings.tab = Tab::System,
                    "settings-addon-on" => self.settings.tab = Tab::Addons,
                    "settings-addon" => {
                        self.settings.tab = Tab::Addons;
                        #[cfg(windows)]
                        {
                            self.settings.addon.asking = true;
                        }
                    }
                    "settings-search" => self.settings.query = "sound".into(),
                    _ => {}
                }
            }
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
            self.gallery.opened = self.gallery.first_video();
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
                n if n.starts_with("settings") || n.starts_with("look") => Screen::Settings,
                "editor" | "editor-crop" | "editor-rotate" | "editor-fades" | "editor-sound"
                | "editor-save" => {
                    let first = self.gallery.first_video();
                    first.map(Screen::Edit).unwrap_or(Screen::Gallery)
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
        if name.starts_with("editor") && shot.waited == 60 {
            self.editor.demo(name);
        }
        if shot.step == 0 && shot.waited == 1 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(vec2(
                std::env::var("SC_GUI_SHOT_WIDTH")
                    .ok()
                    .and_then(|w| w.parse().ok())
                    .unwrap_or(1080.0),
                std::env::var("SC_GUI_SHOT_HEIGHT")
                    .ok()
                    .and_then(|h| h.parse().ok())
                    .unwrap_or(900.0),
            )));
        }
        let settled = !self.thumbs.pending() || shot.waited > 1500;
        if shot.waited >= (if name.starts_with("editor") { 220 } else { 45 })
            && settled
            && !shot.requested
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
            shot.requested = true;
        }
        self.shot = Some(shot);
    }
}
