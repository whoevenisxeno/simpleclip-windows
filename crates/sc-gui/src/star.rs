use crate::app::ScApp;
use crate::theme::{self, pal};
use crate::widgets;
use egui::{Align2, Color32, FontId, RichText, Rounding, Stroke};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const REPO_URL: &str = "https://github.com/whoevenisxeno/simpleclip-windows";
const OPEN_FOR: Duration = Duration::from_secs(5 * 60);
const FIRST_AFTER: u64 = 2 * 86_400;
const AGAIN_AFTER: u64 = 7 * 86_400;

pub struct StarPrompt {
    opened: Instant,
    showing: bool,
}

impl Default for StarPrompt {
    fn default() -> Self {
        Self {
            opened: Instant::now(),
            showing: false,
        }
    }
}

impl StarPrompt {
    pub fn is_showing(&self) -> bool {
        self.showing
    }

    pub fn force(&mut self) {
        self.showing = true;
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

pub fn show(app: &mut ScApp, ctx: &egui::Context, busy: bool) {
    if app.cfg.general.star_prompt_done {
        return;
    }
    let now = now_secs();
    if !app.star.showing {
        let next = *app
            .cfg
            .general
            .star_prompt_next
            .get_or_insert(now + FIRST_AFTER);
        if busy || app.star.opened.elapsed() < OPEN_FOR || now < next {
            ctx.request_repaint_after(Duration::from_secs(30));
            return;
        }
        app.star.showing = true;
    }

    let p = pal(ctx);
    let (mut star, mut later, mut never) = (false, false, false);
    egui::Area::new(egui::Id::new("star-prompt"))
        .anchor(Align2::RIGHT_BOTTOM, [-20.0, -20.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(p.card)
                .rounding(Rounding::same(theme::radius::CARD))
                .stroke(Stroke::new(1.0_f32, p.line))
                .inner_margin(egui::Margin::same(16.0))
                .shadow(theme::float_shadow(&p))
                .show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("\u{2605}")
                                .size(18.0)
                                .color(Color32::from_rgb(0xff, 0xc8, 0x3d)),
                        );
                        ui.label(
                            RichText::new("Enjoying SimpleClip?")
                                .font(FontId::new(15.0, theme::semibold()))
                                .color(p.text),
                        );
                    });
                    ui.add_space(4.0);
                    widgets::secondary(
                        ui,
                        "It is free and open source. A star on GitHub helps other people find it. If you open the page, this will not show again.",
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if widgets::primary_button(ui, None, "Star on GitHub").clicked() {
                            star = true;
                        }
                        if widgets::quiet_button(ui, None, "Maybe later").clicked() {
                            later = true;
                        }
                    });
                    ui.add_space(8.0);
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new("Don’t ask again").size(12.0).color(p.text_2),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        never = true;
                    }
                });
        });

    if star {
        open_url(REPO_URL);
        app.cfg.general.star_prompt_done = true;
        app.star.showing = false;
    } else if never {
        app.cfg.general.star_prompt_done = true;
        app.star.showing = false;
    } else if later {
        app.cfg.general.star_prompt_next = Some(now + AGAIN_AFTER);
        app.star.showing = false;
    }
}
