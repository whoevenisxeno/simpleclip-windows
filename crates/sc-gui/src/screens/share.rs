//! The "share this clip?" prompt shown after a trim is saved: copy the file to the
//! clipboard, or compress it to fit Discord's upload limit and then offer to copy
//! the compressed file.

use crate::app::{ScApp, ToastKind};
use crate::screens::{gallery, sheet};
use crate::theme::{self, icon, pal};
use crate::widgets;
use crossbeam_channel::{Receiver, TryRecvError};
use egui::{FontId, RichText};
use std::path::PathBuf;
use std::time::Duration;

type Job = Receiver<Result<PathBuf, String>>;

enum Stage {
    /// Choose between copying and compressing.
    Ask { choosing_size: bool },
    /// The daemon is compressing.
    Working(Job),
    /// Compression finished; the new file can be copied.
    Done(PathBuf),
    /// Compression did not work (or was not needed); the message says why.
    Failed(String),
}

/// The open prompt, if any.
pub struct ShareState {
    /// The clip that was just saved.
    path: PathBuf,
    stage: Stage,
    /// Size limit chosen for compression, in megabytes.
    mb: u32,
}

impl ShareState {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            stage: Stage::Ask {
                choosing_size: false,
            },
            mb: 10,
        }
    }
}

impl ShareState {
    /// The prompt with the size choice showing (for the design-review run).
    pub fn demo(path: PathBuf) -> Self {
        Self {
            path,
            stage: Stage::Ask {
                choosing_size: true,
            },
            mb: 10,
        }
    }
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Copy `path` to the clipboard and say how it went.
fn copy(app: &mut ScApp, path: &std::path::Path) {
    match gallery::copy_file(path) {
        Ok(()) => app.toast(
            "Copied. Paste it in Discord or any chat with Ctrl+V",
            ToastKind::Ok,
        ),
        Err(e) => app.toast(e, ToastKind::Warn),
    }
}

/// Draw the prompt and handle its buttons. Call once per frame.
pub fn show(app: &mut ScApp, ctx: &egui::Context) {
    let Some(mut st) = app.share.take() else {
        return;
    };
    let p = pal(ctx);

    // Compression running: pick up the result when it arrives.
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

    let mut copy_path: Option<PathBuf> = None;
    let mut compress_now = false;
    let mut turn_off = false;
    let mut set_choosing: Option<bool> = None;
    let mut want_close = false;
    let mut mb = st.mb;

    let working = matches!(st.stage, Stage::Working(_));
    let close = sheet::modal(ctx, "share-dialog", 440.0, |ui| {
        let (heading, name) = match &st.stage {
            Stage::Done(out) => ("Compressed", file_name(out)),
            Stage::Failed(_) => ("Share this clip", file_name(&st.path)),
            _ => ("Clip saved", file_name(&st.path)),
        };
        ui.label(
            RichText::new(heading)
                .font(FontId::new(19.0, theme::semibold()))
                .color(p.text),
        );
        ui.add_space(2.0);
        widgets::secondary(ui, name);
        ui.add_space(14.0);

        match &st.stage {
            Stage::Ask { choosing_size } => {
                widgets::secondary(ui, "Do you want to share it?");
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if widgets::primary_button(ui, Some(icon::COPY), "Copy file").clicked() {
                        copy_path = Some(st.path.clone());
                    }
                    if widgets::quiet_button(ui, Some(icon::SHARE), "Compress for Discord")
                        .clicked()
                    {
                        set_choosing = Some(!choosing_size);
                    }
                });
                if *choosing_size {
                    ui.add_space(12.0);
                    widgets::secondary(
                        ui,
                        "Largest size. The picture is scaled down only as far as needed.",
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        widgets::segmented(
                            ui,
                            &mut mb,
                            &[(10, "10 MB"), (25, "25 MB"), (50, "50 MB")],
                        );
                        if widgets::primary_button(ui, None, "Compress").clicked() {
                            compress_now = true;
                        }
                    });
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new("Don't ask again").size(12.0).color(p.text_2),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        turn_off = true;
                    }
                });
            }
            Stage::Working(_) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().color(p.accent));
                    ui.add_space(6.0);
                    widgets::secondary(ui, format!("Compressing to fit {} MB...", st.mb));
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
                        copy_path = Some(out.clone());
                    }
                    if widgets::quiet_button(ui, Some(icon::FOLDER), "Show in folder").clicked() {
                        gallery::reveal(out);
                    }
                });
            }
            Stage::Failed(msg) => {
                widgets::notice(ui, p.orange, msg);
                ui.add_space(10.0);
                if widgets::primary_button(ui, Some(icon::COPY), "Copy file as it is").clicked() {
                    copy_path = Some(st.path.clone());
                }
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
            want_close = true;
        }
    });
    let closing = (close && !working) || want_close;
    st.mb = mb;

    if let Some(path) = copy_path {
        copy(app, &path);
    }
    if turn_off {
        app.cfg.general.share_prompt = false;
        app.toast(
            "You won't be asked again. You can turn this back on in Settings.",
            ToastKind::Ok,
        );
        return;
    }
    if closing {
        return;
    }
    if let Some(c) = set_choosing {
        st.stage = Stage::Ask { choosing_size: c };
    }
    if compress_now {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let req = sc_core::ipc::Request::Compress {
            path: st.path.clone(),
            target_mb: st.mb.max(1),
        };
        std::thread::spawn(move || {
            let result = match crate::daemon::request(req) {
                Ok(sc_core::ipc::Response::Saved { path, .. }) => Ok(path),
                Ok(sc_core::ipc::Response::Error { message }) => Err(message),
                Ok(_) => Err("unexpected reply from SimpleClip".to_string()),
                Err(e) => Err(e.to_string()),
            };
            let _ = tx.send(result);
        });
        st.stage = Stage::Working(rx);
    }
    app.share = Some(st);
}
