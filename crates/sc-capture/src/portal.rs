use crate::{stream, Error, Result};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use sc_core::capture::VideoFrame;

/// Runs the whole capture lifetime: negotiate the portal ScreenCast session, open
/// the PipeWire remote, then hand off to the (blocking) PipeWire stream loop. The
/// portal session is held in this scope for the entire capture so the compositor
/// keeps streaming; it closes when this function returns.
pub async fn run(
    frames: crossbeam_channel::Sender<VideoFrame>,
    dims: crossbeam_channel::Sender<(u32, u32)>,
    stop: pipewire::channel::Receiver<()>,
    epoch: std::time::Instant,
) -> Result<()> {
    let proxy = Screencast::new()
        .await
        .map_err(|e| Error::Portal(e.to_string()))?;
    let session = proxy
        .create_session(Default::default())
        .await
        .map_err(|e| Error::Portal(e.to_string()))?;
    proxy
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_cursor_mode(CursorMode::Embedded)
                .set_sources(SourceType::Monitor | SourceType::Window)
                .set_multiple(false)
                .set_persist_mode(PersistMode::DoNot),
        )
        .await
        .map_err(|e| Error::Portal(e.to_string()))?;

    let response = proxy
        .start(&session, None, Default::default())
        .await
        .map_err(|e| Error::Portal(e.to_string()))?
        .response()
        .map_err(|e| Error::Portal(e.to_string()))?;
    let node_id = response
        .streams()
        .first()
        .ok_or(Error::NoFormat)?
        .pipe_wire_node_id();

    let fd = proxy
        .open_pipe_wire_remote(&session, Default::default())
        .await
        .map_err(|e| Error::Portal(e.to_string()))?;
    tracing::info!(node_id, "portal ScreenCast started");

    stream::run(fd, node_id, frames, dims, stop, epoch)
}
