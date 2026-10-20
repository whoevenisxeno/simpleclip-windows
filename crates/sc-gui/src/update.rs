//! Checks GitHub for a newer release, shows its changelog, and installs it on
//! request. Network access goes through `curl.exe`, which ships with Windows 10
//! and 11, so the app needs no HTTP library of its own.
//!
//! The changelog shown is the release's notes, so every release must carry its
//! `CHANGELOG.md` entry as its notes.

use crate::theme::{self, pal};
use crate::widgets;
use egui::{Align2, Color32, FontId, RichText, Rounding, Stroke};
pub use sc_core::update::Release;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The version this build is.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const PAGE: &str = "https://github.com/whoevenisxeno/simpleclip-windows/releases/latest";
/// How often a running app looks again.
const RECHECK: Duration = Duration::from_secs(6 * 3600);
/// A copy this many major versions behind (4.x while 9.x is out) must update before it can be used.
pub const FORCE_AFTER_MAJORS: u64 = 5;

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
    /// The forced update has been started (so it is only started once).
    forced_started: bool,
    /// The installer has been launched (so it is only launched once, however many
    /// frames pass before the window closes).
    handed_over: bool,
    /// Bytes of the installer downloaded so far.
    progress: Arc<AtomicU64>,
    /// The release being installed, kept for the progress view.
    installing: Option<Release>,
    /// When the download finished (the app waits a moment so you can see it).
    ready_since: Option<Instant>,
}

impl Default for Updater {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Idle)),
            checked: None,
            started: Instant::now(),
            open: false,
            forced_started: false,
            handed_over: false,
            progress: Arc::new(AtomicU64::new(0)),
            installing: None,
            ready_since: None,
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
    /// The version updates are compared against. `SC_UPDATE_PRETEND_VERSION` lets a test
    /// run pretend to be older, to try the update flow against a real release.
    fn compare_base() -> String {
        std::env::var("SC_UPDATE_PRETEND_VERSION").unwrap_or_else(|_| CURRENT.to_string())
    }

    pub fn check(&mut self, ctx: &egui::Context) {
        if matches!(self.state(), State::Checking | State::Downloading(_)) {
            return;
        }
        self.checked = Some(Instant::now());
        Self::set(&self.state, State::Checking, ctx);
        let (state, ctx) = (self.state.clone(), ctx.clone());
        std::thread::spawn(move || {
            let next = match sc_core::update::fetch_latest(&Self::compare_base()) {
                Ok(Some(r)) => State::Available(r),
                Ok(None) => State::UpToDate,
                Err(e) => State::Failed(e),
            };
            Self::set(&state, next, &ctx);
        });
    }

    /// Download the installer in the background; the state becomes `Ready` when done.
    /// The size on disk is polled so the dialog can show a progress bar.
    fn download(&mut self, r: Release, ctx: &egui::Context) {
        use std::os::windows::process::CommandExt;
        self.progress.store(0, Ordering::Relaxed);
        self.installing = Some(r.clone());
        self.ready_since = None;
        Self::set(&self.state, State::Downloading(r.clone()), ctx);
        let (state, ctx, progress) = (self.state.clone(), ctx.clone(), self.progress.clone());
        std::thread::spawn(move || {
            let dest = std::env::temp_dir().join(format!("SimpleClip-{}-Setup.exe", r.version));
            let _ = std::fs::remove_file(&dest);
            let result = (|| -> Result<(), String> {
                let mut child = std::process::Command::new("curl.exe")
                    .args([
                        "-fsSL",
                        "--retry",
                        "2",
                        // Give up on a stalled connection instead of waiting for ever.
                        "--max-time",
                        "900",
                        "--speed-time",
                        "30",
                        "--speed-limit",
                        "1024",
                        "-H",
                        "User-Agent: SimpleClip",
                        "-o",
                    ])
                    .arg(&dest)
                    .arg(&r.url)
                    .creation_flags(0x0800_0000)
                    .spawn()
                    .map_err(|e| format!("couldn't run curl: {e}"))?;
                loop {
                    progress.store(
                        std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0),
                        Ordering::Relaxed,
                    );
                    ctx.request_repaint();
                    match child.try_wait() {
                        Ok(Some(status)) if status.success() => break,
                        Ok(Some(_)) => return Err("the download failed".to_string()),
                        Ok(None) => std::thread::sleep(Duration::from_millis(120)),
                        Err(e) => return Err(e.to_string()),
                    }
                }
                let got = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                if got == 0 || (r.size > 0 && got != r.size) {
                    return Err("the download was incomplete".to_string());
                }
                // GitHub publishes each file's SHA-256; refuse anything that does not match.
                if let Some(want) = r.sha256.as_deref() {
                    match sha256_of(&dest) {
                        Some(have) if have.eq_ignore_ascii_case(want) => {}
                        _ => {
                            return Err(
                                "the download did not match its checksum, so it was not installed"
                                    .to_string(),
                            );
                        }
                    }
                }
                progress.store(got, Ordering::Relaxed);
                Ok(())
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&dest);
            }
            Self::set(
                &state,
                match result {
                    Ok(()) => State::Ready(dest),
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
            "ping -n 2 127.0.0.1 >nul & \"{}\" /SILENT /NORESTART /SUPPRESSMSGBOXES & start \"\" \"{}\" & start \"\" \"{}\"",
            setup.display(),
            dir.join("scd.exe").display(),
            dir.join("sc-gui.exe").display()
        );
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .raw_arg(script)
            // Test switches must not leak into the relaunched app (it would update again).
            .env_remove("SC_UPDATE_AUTO")
            .env_remove("SC_UPDATE_PRETEND_VERSION")
            .creation_flags(0x0800_0000) // no console window (DETACHED_PROCESS would make cmd open one)
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
        let script = format!("ping -n 3 127.0.0.1 >nul & \"{}\" /SILENT", exe.display());
        let _ = std::process::Command::new("cmd")
            .arg("/C")
            .raw_arg(script)
            .creation_flags(0x0800_0000)
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
                sha256: None,
            });
        }
        self.open = true;
    }

    /// Per-frame work: schedule checks, draw the top-right button and the details window.
    pub fn show(&mut self, ctx: &egui::Context, auto_check: bool, wizard: bool) {
        if auto_check
            && self.started.elapsed() > Duration::from_secs(1)
            && self.checked.is_none_or(|t| t.elapsed() > RECHECK)
        {
            self.check(ctx);
        }
        if let State::Ready(p) = self.state() {
            // Show "installing" for a moment before handing over, so it never looks stuck.
            let since = *self.ready_since.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_millis(1400) {
                if !self.handed_over {
                    self.handed_over = true;
                    Self::hand_over(&p);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            self.open = true;
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let state = self.state();
        let release = match &state {
            State::Available(r) | State::Downloading(r) => Some(r.clone()),
            State::Ready(_) => self.installing.clone(),
            State::Failed(_) => self.installing.clone(),
            _ => None,
        };
        let Some(r) = release else {
            self.open = false;
            return;
        };
        // Far behind (several major versions): the update is not optional.
        let forced =
            sc_core::update::major_gap(&r.version, &Self::compare_base()) >= FORCE_AFTER_MAJORS;
        // Test switch: install a newer version without waiting for a click.
        if std::env::var_os("SC_UPDATE_AUTO").is_some()
            && matches!(state, State::Available(_))
            && !self.forced_started
        {
            self.forced_started = true;
            self.download(r, ctx);
            return;
        }
        if wizard && !forced {
            return;
        }
        if forced {
            self.open = true;
            if matches!(state, State::Available(_)) && !self.forced_started {
                self.forced_started = true;
                self.download(r, ctx);
                return;
            }
            Self::block_ui(ctx);
        }
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
        let downloading = matches!(state, State::Downloading(_) | State::Ready(_));
        let finished = matches!(state, State::Ready(_));
        let got = self.progress.load(Ordering::Relaxed);
        let mut close = false;
        let mut install = false;
        egui::Window::new("update-window")
            .order(egui::Order::Foreground)
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
                    RichText::new(if forced {
                        "Update required".to_string()
                    } else {
                        format!("SimpleClip {} is available", r.version)
                    })
                    .font(FontId::new(19.0, theme::semibold()))
                    .color(p.text),
                );
                ui.add_space(2.0);
                widgets::secondary(
                    ui,
                    if forced {
                        format!(
                            "Version {CURRENT} is too old to keep using. Installing {} now. Here is what changed:",
                            r.version
                        )
                    } else {
                        format!("You have {CURRENT}. Here is what changed:")
                    },
                );
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
                if downloading {
                    let frac = if r.size > 0 {
                        (got as f32 / r.size as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
                    ui.add(
                        egui::ProgressBar::new(if finished { 1.0 } else { frac })
                            .fill(p.accent)
                            .desired_height(10.0)
                            .rounding(Rounding::same(5.0)),
                    );
                    ui.add_space(6.0);
                    widgets::secondary(
                        ui,
                        if finished {
                            "Download finished. Installing the update. SimpleClip reopens by itself in a moment.".to_string()
                        } else if r.size > 0 {
                            format!(
                                "Downloading\u{2026} {:.1} of {:.1} MB ({:.0}%)",
                                mb(got),
                                mb(r.size),
                                frac * 100.0
                            )
                        } else {
                            format!("Downloading\u{2026} {:.1} MB", mb(got))
                        },
                    );
                    ui.add_space(12.0);
                }
                if let State::Failed(e) = &state {
                    widgets::notice(ui, p.red, e);
                }
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let failed = matches!(state, State::Failed(_));
                    let label = if downloading {
                        "Updating\u{2026}"
                    } else if failed {
                        "Try again"
                    } else {
                        "Update now"
                    };
                    if widgets::primary_button(ui, None, label).clicked() && !downloading {
                        install = true;
                    }
                    if forced {
                        // No "Later": the only other way out is closing the app.
                        if !downloading && widgets::quiet_button(ui, None, "Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else if !downloading && widgets::quiet_button(ui, None, "Later").clicked() {
                        close = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.hyperlink_to("Release page", PAGE);
                    });
                });
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

impl Updater {
    /// Dim the whole window and swallow clicks, so nothing else can be used
    /// while a required update is on screen.
    fn block_ui(ctx: &egui::Context) {
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("update-block"))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, _) =
                    ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
                ui.painter()
                    .rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(170));
            });
    }
}

impl Updater {
    /// Pretend a download is under way (for the design-review run).
    pub fn demo_download(&mut self, version: &str) {
        let r = Release {
            version: version.into(),
            notes: "## What changed\n\n### Added\n- A lot of things\n\n### Fixed\n- A lot of other things\n".into(),
            url: String::new(),
            size: 58 * 1024 * 1024,
            sha256: None,
        };
        self.progress.store(24 * 1024 * 1024, Ordering::Relaxed);
        self.installing = Some(r.clone());
        self.forced_started = true;
        if let Ok(mut s) = self.state.lock() {
            *s = State::Downloading(r);
        }
        self.open = true;
    }
}

/// SHA-256 of a file as lowercase hex, using Windows' own `certutil` so the app
/// needs no hashing library.
fn sha256_of(path: &std::path::Path) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("certutil")
        .arg("-hashfile")
        .arg(path)
        .arg("SHA256")
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    // Line 1 is a header, line 2 the hash (possibly with spaces), then a footer.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .nth(1)
        .map(|l| {
            l.split_whitespace()
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|h| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit()))
}

/// Delete installers left in the temp folder by earlier updates.
pub fn clean_old_downloads() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with("SimpleClip-") && name.ends_with("-Setup.exe") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
