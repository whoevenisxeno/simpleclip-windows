use crate::app::{ScApp, ToastKind};
use crate::screens::{gallery, sheet};
use crate::theme::{self, icon, pal};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{FontId, RichText, Ui};
use sc_core::compress::{CompressOptions, FRAME_RATES, HEIGHTS, MAX_MB, MIN_MB, SIZES_MB};
use std::path::PathBuf;
use std::time::Duration;

type Job = Receiver<Result<PathBuf, String>>;

#[cfg(windows)]
type Facts = Option<sc_ffmpeg::Source>;
#[cfg(not(windows))]
type Facts = Option<()>;

enum Stage {
    Ask { choosing_size: bool },
    Working(Job),
    Done(PathBuf),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum SizePick {
    Mb(u32),
    Custom,
}

pub struct ShareState {
    path: PathBuf,
    stage: Stage,
    opts: CompressOptions,
    pick: SizePick,
    facts: Facts,
}

fn read_facts(path: &std::path::Path) -> Facts {
    #[cfg(windows)]
    {
        sc_ffmpeg::source_facts(path).ok()
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

impl ShareState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            stage: Stage::Ask {
                choosing_size: false,
            },
            opts: CompressOptions::default(),
            pick: SizePick::Mb(10),
            facts: None,
        }
    }

    pub fn choosing(path: PathBuf) -> Self {
        let facts = read_facts(&path);
        Self {
            stage: Stage::Ask {
                choosing_size: true,
            },
            facts,
            ..Self::new(path)
        }
    }

    pub fn compressing(path: PathBuf, opts: CompressOptions) -> Self {
        let pick = pick_for(opts.target_mb);
        let stage = Stage::Working(start_job(&path, &opts));
        Self {
            path,
            stage,
            opts,
            pick,
            facts: None,
        }
    }

    pub fn demo(path: PathBuf) -> Self {
        Self::choosing(path)
    }
}

fn pick_for(mb: u32) -> SizePick {
    if SIZES_MB.contains(&mb) {
        SizePick::Mb(mb)
    } else {
        SizePick::Custom
    }
}

fn request_for(path: &std::path::Path, opts: &CompressOptions) -> sc_core::ipc::Request {
    sc_core::ipc::Request::Compress {
        path: path.to_path_buf(),
        target_mb: opts.target_mb,
        max_height: opts.max_height,
        max_fps: opts.max_fps,
        mute: opts.mute,
    }
}

fn start_job(path: &std::path::Path, opts: &CompressOptions) -> Job {
    let (tx, rx) = crossbeam_channel::bounded(1);
    let req = request_for(path, opts);
    std::thread::spawn(move || {
        let result = match crate::daemon::request(req) {
            Ok(sc_core::ipc::Response::Saved { path, .. }) => Ok(path),
            Ok(sc_core::ipc::Response::Error { message }) => Err(message),
            Ok(_) => Err("unexpected reply from SimpleClip".to_string()),
            Err(e) => Err(e.to_string()),
        };
        let _ = tx.send(result);
    });
    rx
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn copy(app: &mut ScApp, path: &std::path::Path) {
    match gallery::copy_file(path) {
        Ok(()) => app.toast(
            "Copied. Paste it in Discord or any chat with Ctrl+V",
            ToastKind::Ok,
        ),
        Err(e) => app.toast(e, ToastKind::Warn),
    }
}

struct Preview {
    text: String,
    fits_already: bool,
}

fn preview(facts: &Facts, opts: &CompressOptions) -> Option<Preview> {
    #[cfg(windows)]
    {
        let src = facts.as_ref()?;
        if src.bytes <= opts.target_bytes() && !opts.changes_picture_or_sound() {
            return Some(Preview {
                text: format!(
                    "This clip is {}, already under {} MB.",
                    gallery::size_label(src.bytes),
                    opts.target_mb
                ),
                fits_already: true,
            });
        }
        let plan = sc_ffmpeg::plan_compress(src, opts);
        let sound = if !src.has_audio {
            ""
        } else if opts.mute {
            ", no sound"
        } else {
            ", with sound"
        };
        Some(Preview {
            text: format!(
                "Will be saved at {} \u{d7} {}, {} fps{}, up to {} MB.",
                plan.width, plan.height, plan.fps, sound, opts.target_mb
            ),
            fits_already: false,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (facts, opts);
        None
    }
}

fn option_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    let p = theme::pal_ui(ui);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(92.0, 32.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::new(13.0, egui::FontFamily::Proportional),
            p.text_2,
        );
        add(ui);
    });
}

fn options_panel(ui: &mut Ui, st: &mut ShareState) {
    let mut pick = st.pick;
    option_row(ui, "Size", |ui| {
        let mut options: Vec<(SizePick, String)> = SIZES_MB
            .iter()
            .map(|mb| (SizePick::Mb(*mb), format!("{mb}")))
            .collect();
        options.push((SizePick::Custom, "Custom".to_string()));
        let labelled: Vec<(SizePick, &str)> =
            options.iter().map(|(v, l)| (*v, l.as_str())).collect();
        widgets::segmented(ui, &mut pick, &labelled);
        if !matches!(pick, SizePick::Custom) {
            widgets::secondary(ui, "MB");
        }
    });
    if pick != st.pick {
        st.pick = pick;
        if let SizePick::Mb(mb) = pick {
            st.opts.target_mb = mb;
        }
    }
    if st.pick == SizePick::Custom {
        option_row(ui, "", |ui| {
            let mut mb = st.opts.target_mb.clamp(MIN_MB, MAX_MB);
            ui.add(
                egui::DragValue::new(&mut mb)
                    .range(MIN_MB..=MAX_MB)
                    .suffix(" MB")
                    .speed(0.5),
            );
            st.opts.target_mb = mb;
            widgets::secondary(ui, format!("Any size from {MIN_MB} to {MAX_MB} MB."));
        });
    }
    option_row(ui, "Resolution", |ui| {
        let mut options: Vec<(Option<u32>, String)> = vec![(None, "Auto".to_string())];
        options.extend(HEIGHTS.iter().map(|h| (Some(*h), format!("{h}p"))));
        let labelled: Vec<(Option<u32>, &str)> =
            options.iter().map(|(v, l)| (*v, l.as_str())).collect();
        widgets::segmented(ui, &mut st.opts.max_height, &labelled);
    });
    option_row(ui, "Frame rate", |ui| {
        let mut options: Vec<(Option<u32>, String)> = vec![(None, "Keep".to_string())];
        options.extend(FRAME_RATES.iter().map(|f| (Some(*f), format!("{f}"))));
        let labelled: Vec<(Option<u32>, &str)> =
            options.iter().map(|(v, l)| (*v, l.as_str())).collect();
        widgets::segmented(ui, &mut st.opts.max_fps, &labelled);
    });
    option_row(ui, "Sound", |ui| {
        widgets::segmented(ui, &mut st.opts.mute, &[(false, "Keep"), (true, "Off")]);
    });
}

#[derive(Default)]
struct Actions {
    copy_path: Option<PathBuf>,
    compress_now: bool,
    turn_off: bool,
    set_choosing: Option<bool>,
    want_close: bool,
    retry: bool,
    scrim: bool,
}

fn dialog(st: &mut ShareState, ctx: &egui::Context) -> Actions {
    let p = pal(ctx);
    let mut act = Actions::default();
    let working = matches!(st.stage, Stage::Working(_));
    let width = if matches!(
        st.stage,
        Stage::Ask {
            choosing_size: true
        }
    ) {
        560.0
    } else {
        440.0
    };
    act.scrim = sheet::modal(ctx, "share-dialog", width, |ui| {
        let (heading, name) = match &st.stage {
            Stage::Done(out) => ("Compressed", file_name(out)),
            Stage::Failed(_) => ("Share this clip", file_name(&st.path)),
            Stage::Ask {
                choosing_size: true,
            } => ("Compress for Discord", file_name(&st.path)),
            _ => ("Clip saved", file_name(&st.path)),
        };
        ui.label(
            RichText::new(heading)
                .font(FontId::new(20.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(2.0);
        widgets::secondary(ui, name);
        ui.add_space(14.0);

        match &st.stage {
            Stage::Ask { choosing_size } => {
                let choosing = *choosing_size;
                if !choosing {
                    widgets::secondary(ui, "Do you want to share it?");
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if widgets::primary_button(ui, Some(icon::COPY), "Copy file").clicked() {
                            act.copy_path = Some(st.path.clone());
                        }
                        if widgets::quiet_button(ui, Some(icon::SHARE), "Compress for Discord")
                            .clicked()
                        {
                            act.set_choosing = Some(true);
                        }
                    });
                } else {
                    widgets::secondary(
                        ui,
                        "Pick the largest size you can upload. The picture is only scaled down as far as it must be.",
                    );
                    ui.add_space(10.0);
                    options_panel(ui, st);
                    ui.add_space(8.0);
                    let note = preview(&st.facts, &st.opts);
                    let fits = note.as_ref().is_some_and(|n| n.fits_already);
                    if let Some(n) = &note {
                        widgets::secondary(ui, n.text.clone());
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        ui.add_enabled_ui(!fits, |ui| {
                            if widgets::primary_button(ui, Some(icon::SHARE), "Compress").clicked()
                            {
                                act.compress_now = true;
                            }
                        });
                        if widgets::quiet_button(ui, Some(icon::COPY), "Copy file as it is")
                            .clicked()
                        {
                            act.copy_path = Some(st.path.clone());
                        }
                    });
                }
                ui.add_space(14.0);
                if !choosing
                    && ui
                        .add(
                            egui::Label::new(
                                RichText::new("Don’t ask again").size(12.0).color(p.text_2),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                {
                    act.turn_off = true;
                }
            }
            Stage::Working(_) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().color(p.accent));
                    ui.add_space(6.0);
                    widgets::secondary(
                        ui,
                        format!("Compressing to fit {}\u{2026}", st.opts.describe()),
                    );
                });
                ui.add_space(4.0);
                widgets::secondary(ui, "This takes a few seconds for a short clip.");
            }
            Stage::Done(out) => {
                let size = std::fs::metadata(out).map(|m| m.len()).unwrap_or(0);
                widgets::secondary(
                    ui,
                    format!(
                        "Saved next to the original, {}. Copy it so you can paste it in Discord?",
                        gallery::size_label(size)
                    ),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::primary_button(ui, Some(icon::COPY), "Copy file").clicked() {
                        act.copy_path = Some(out.clone());
                    }
                    if widgets::quiet_button(ui, Some(icon::FOLDER), "Show in folder").clicked() {
                        gallery::reveal(out);
                    }
                });
            }
            Stage::Failed(msg) => {
                widgets::notice(ui, p.orange, msg);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::primary_button(ui, Some(icon::REFRESH), "Change options").clicked()
                    {
                        act.retry = true;
                    }
                    if widgets::quiet_button(ui, Some(icon::COPY), "Copy file as it is").clicked() {
                        act.copy_path = Some(st.path.clone());
                    }
                });
            }
        }
        ui.add_space(10.0);
        if !working
            && ui
                .add(
                    egui::Label::new(RichText::new("Close").size(13.0).color(p.text_2))
                        .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
        {
            act.want_close = true;
        }
    });
    if working {
        act.scrim = false;
    }
    act
}

pub fn show(app: &mut ScApp, ctx: &egui::Context) {
    let Some(mut st) = app.share.take() else {
        return;
    };

    let mut next: Option<Stage> = None;
    if let Stage::Working(rx) = &st.stage {
        match rx.try_recv() {
            Ok(Ok(out)) => {
                app.library.set_compressed(&out, true);
                app.gallery.rescan();
                next = Some(Stage::Done(out));
            }
            Ok(Err(msg)) => next = Some(Stage::Failed(msg)),
            Err(TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
            Err(TryRecvError::Disconnected) => {
                next = Some(Stage::Failed("compression stopped unexpectedly".into()))
            }
        }
    }
    if let Some(n) = next {
        st.stage = n;
    }

    let act = dialog(&mut st, ctx);
    if let Some(path) = act.copy_path {
        copy(app, &path);
    }
    if act.turn_off {
        app.cfg.general.share_prompt = false;
        app.toast(
            "You won’t be asked again. You can turn this back on in Settings.",
            ToastKind::Ok,
        );
        return;
    }
    if act.scrim || act.want_close {
        return;
    }
    if let Some(c) = act.set_choosing {
        if c && st.facts.is_none() {
            st.facts = read_facts(&st.path);
        }
        st.stage = Stage::Ask { choosing_size: c };
    }
    if act.retry {
        if st.facts.is_none() {
            st.facts = read_facts(&st.path);
        }
        st.stage = Stage::Ask {
            choosing_size: true,
        };
    }
    if act.compress_now {
        match st.opts.validate() {
            Ok(()) => st.stage = Stage::Working(start_job(&st.path, &st.opts)),
            Err(message) => st.stage = Stage::Failed(message),
        }
    }
    app.share = Some(st);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::Harness;
    use egui::Modifiers;

    fn choosing() -> ShareState {
        ShareState {
            facts: None,
            ..ShareState::choosing(PathBuf::from("C:/Clips/sc-apex-1.mp4"))
        }
    }

    fn two_frames(h: &mut Harness, st: &mut ShareState) -> Actions {
        let mut act = Actions::default();
        for _ in 0..2 {
            h.frame(Vec::new(), Modifiers::NONE, &mut |ui| {
                act = dialog(st, ui.ctx());
            });
        }
        act
    }

    fn click(h: &mut Harness, st: &mut ShareState, text: &str) -> Actions {
        let mut seen = Actions::default();
        h.click_text(text, &mut |ui| {
            let a = dialog(st, ui.ctx());
            seen.compress_now |= a.compress_now;
            seen.want_close |= a.want_close;
            seen.retry |= a.retry;
            seen.turn_off |= a.turn_off;
            seen.scrim |= a.scrim;
            seen.copy_path = seen.copy_path.take().or(a.copy_path);
            seen.set_choosing = seen.set_choosing.or(a.set_choosing);
        });
        seen
    }

    #[test]
    fn the_request_carries_every_option() {
        let opts = CompressOptions {
            target_mb: 50,
            max_height: Some(720),
            max_fps: Some(30),
            mute: true,
        };
        match request_for(std::path::Path::new("C:/Clips/a.mp4"), &opts) {
            sc_core::ipc::Request::Compress {
                path,
                target_mb,
                max_height,
                max_fps,
                mute,
            } => {
                assert_eq!(path, std::path::PathBuf::from("C:/Clips/a.mp4"));
                assert_eq!(
                    (target_mb, max_height, max_fps, mute),
                    (50, Some(720), Some(30), true)
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_preset_size_is_a_chip_and_any_other_size_is_custom() {
        assert_eq!(pick_for(10), SizePick::Mb(10));
        assert_eq!(pick_for(500), SizePick::Mb(500));
        assert_eq!(pick_for(37), SizePick::Custom);
    }

    #[test]
    fn the_dialog_offers_every_option() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        for text in [
            "Compress for Discord",
            "Size",
            "Resolution",
            "Frame rate",
            "Sound",
            "8",
            "10",
            "25",
            "50",
            "100",
            "500",
            "Custom",
            "Auto",
            "1080p",
            "720p",
            "480p",
            "360p",
            "Keep",
            "60",
            "30",
            "24",
            "Off",
            "Compress",
            "Copy file as it is",
        ] {
            assert!(h.has(text), "the dialog is missing {text}");
        }
    }

    #[test]
    fn the_size_chips_set_the_target() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        assert_eq!(st.opts.target_mb, 10);
        click(&mut h, &mut st, "50");
        assert_eq!((st.opts.target_mb, st.pick), (50, SizePick::Mb(50)));
        click(&mut h, &mut st, "8");
        assert_eq!(st.opts.target_mb, 8);
        click(&mut h, &mut st, "500");
        assert_eq!(st.opts.target_mb, 500);
    }

    #[test]
    fn a_custom_size_can_be_typed_in_and_stays_in_range() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        assert!(!h.has("Any size from 1 to 500 MB."));
        click(&mut h, &mut st, "Custom");
        assert_eq!(st.pick, SizePick::Custom);
        two_frames(&mut h, &mut st);
        assert!(h.has("Any size from 1 to 500 MB."));
        st.opts.target_mb = 37;
        two_frames(&mut h, &mut st);
        assert_eq!(st.opts.target_mb, 37);
        assert!(st.opts.validate().is_ok());
        st.opts.target_mb = 9_999;
        two_frames(&mut h, &mut st);
        assert!(st.opts.target_mb <= 500, "{}", st.opts.target_mb);
    }

    #[test]
    fn resolution_frame_rate_and_sound_choices_are_stored() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        click(&mut h, &mut st, "720p");
        assert_eq!(st.opts.max_height, Some(720));
        click(&mut h, &mut st, "480p");
        assert_eq!(st.opts.max_height, Some(480));
        click(&mut h, &mut st, "Auto");
        assert_eq!(st.opts.max_height, None);

        click(&mut h, &mut st, "30");
        assert_eq!(st.opts.max_fps, Some(30));
        click(&mut h, &mut st, "60");
        assert_eq!(st.opts.max_fps, Some(60));
        click(&mut h, &mut st, "Keep");
        assert!(st.opts.max_fps.is_none() && !st.opts.mute);

        click(&mut h, &mut st, "Off");
        assert!(st.opts.mute);
        click(&mut h, &mut st, "30");
        assert_eq!(st.opts.max_fps, Some(30));
        h.click_nth("Keep", 1, &mut |ui| {
            dialog(&mut st, ui.ctx());
        });
        assert!(!st.opts.mute);
        assert_eq!(st.opts.max_fps, Some(30), "the other row is left alone");
        h.click_nth("Keep", 0, &mut |ui| {
            dialog(&mut st, ui.ctx());
        });
        assert!(st.opts.max_fps.is_none());
    }

    #[test]
    fn the_compress_button_starts_the_job_with_what_was_chosen() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        click(&mut h, &mut st, "25");
        click(&mut h, &mut st, "720p");
        click(&mut h, &mut st, "Off");
        let act = click(&mut h, &mut st, "Compress");
        assert!(act.compress_now);
        assert_eq!(
            st.opts,
            CompressOptions {
                target_mb: 25,
                max_height: Some(720),
                max_fps: None,
                mute: true,
            }
        );
    }

    #[test]
    fn the_dialog_can_copy_the_original_or_be_closed() {
        let mut h = Harness::new();
        let mut st = choosing();
        two_frames(&mut h, &mut st);
        let act = click(&mut h, &mut st, "Copy file as it is");
        assert_eq!(act.copy_path, Some(PathBuf::from("C:/Clips/sc-apex-1.mp4")));
        let act = click(&mut h, &mut st, "Close");
        assert!(act.want_close);
    }

    #[test]
    fn the_first_prompt_after_saving_leads_to_the_options() {
        let mut h = Harness::new();
        let mut st = ShareState::new(PathBuf::from("C:/Clips/sc-apex-1.mp4"));
        two_frames(&mut h, &mut st);
        assert!(h.has("Clip saved") && h.has("Copy file") && h.has("Compress for Discord"));
        assert!(!h.has("Resolution"));
        assert!(h.has("Don’t ask again"));
        let act = click(&mut h, &mut st, "Compress for Discord");
        assert_eq!(act.set_choosing, Some(true));
        let act = click(&mut h, &mut st, "Don’t ask again");
        assert!(act.turn_off);
    }

    #[test]
    fn a_failed_job_offers_to_change_the_options() {
        let mut h = Harness::new();
        let mut st = choosing();
        st.stage = Stage::Failed("couldn't get this clip under that size".into());
        two_frames(&mut h, &mut st);
        assert!(h.has("Change options") && h.has("Copy file as it is"));
        let act = click(&mut h, &mut st, "Change options");
        assert!(act.retry);
    }

    #[cfg(windows)]
    #[test]
    fn the_preview_says_what_will_happen() {
        let facts: Facts = Some(sc_ffmpeg::Source {
            width: 1920,
            height: 1080,
            fps: 60,
            secs: 30.0,
            has_audio: true,
            bytes: 120_000_000,
        });
        let note = preview(&facts, &CompressOptions::with_size(10)).unwrap();
        assert!(!note.fits_already);
        assert!(note.text.contains("60 fps") && note.text.contains("with sound"));
        assert!(note.text.contains("10 MB"));

        let capped = CompressOptions {
            target_mb: 500,
            max_height: Some(720),
            max_fps: Some(30),
            mute: true,
        };
        let note = preview(&facts, &capped).unwrap();
        assert!(note.text.contains("1280 \u{d7} 720"), "{}", note.text);
        assert!(note.text.contains("30 fps") && note.text.contains("no sound"));

        let tiny: Facts = Some(sc_ffmpeg::Source {
            bytes: 4_000_000,
            ..facts.clone().unwrap()
        });
        let note = preview(&tiny, &CompressOptions::with_size(10)).unwrap();
        assert!(note.fits_already);
        assert!(note.text.contains("already under 10 MB"));
        assert!(!preview(&tiny, &capped).unwrap().fits_already);
        assert!(preview(&None, &capped).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn a_clip_that_already_fits_disables_the_compress_button() {
        let mut h = Harness::new();
        let mut st = choosing();
        st.facts = Some(sc_ffmpeg::Source {
            width: 1280,
            height: 720,
            fps: 30,
            secs: 5.0,
            has_audio: true,
            bytes: 2_000_000,
        });
        two_frames(&mut h, &mut st);
        assert!(h
            .texts
            .iter()
            .any(|(t, _)| t.contains("already under 10 MB")));
        let act = click(&mut h, &mut st, "Compress");
        assert!(!act.compress_now);
        click(&mut h, &mut st, "Off");
        two_frames(&mut h, &mut st);
        let act = click(&mut h, &mut st, "Compress");
        assert!(act.compress_now);
    }
}
