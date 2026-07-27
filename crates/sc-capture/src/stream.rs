use crate::{Error, Result};
use pipewire as pw;
use pw::{properties::properties, spa};
use sc_core::capture::{PixelFormat, VideoFrame};
use sc_core::time::Timestamp;
use spa::pod::Pod;
use std::os::fd::OwnedFd;
use std::time::Instant;

struct Cap {
    frames: crossbeam_channel::Sender<VideoFrame>,
    dims: Option<crossbeam_channel::Sender<(u32, u32)>>,
    epoch: Instant,
    format: spa::param::video::VideoInfoRaw,
}

pub fn run(
    fd: OwnedFd,
    node_id: u32,
    frames: crossbeam_channel::Sender<VideoFrame>,
    dims: crossbeam_channel::Sender<(u32, u32)>,
    stop: pipewire::channel::Receiver<()>,
    epoch: Instant,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pwerr)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pwerr)?;
    let core = context.connect_fd_rc(fd, None).map_err(pwerr)?;

    let quit = mainloop.clone();
    let _stop = stop.attach(mainloop.loop_(), move |_| quit.quit());

    let data = Cap {
        frames,
        dims: Some(dims),
        epoch,
        format: Default::default(),
    };
    let stream = pw::stream::StreamBox::new(
        &core,
        "sc-capture",
        properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(pwerr)?;

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, cap, id, param| on_param(cap, id, param))
        .process(on_process)
        .register()
        .map_err(pwerr)?;

    connect(&stream, node_id)?;
    mainloop.run();
    Ok(())
}

fn pwerr(e: pw::Error) -> Error {
    Error::Pipewire(e.to_string())
}

fn on_param(cap: &mut Cap, id: u32, param: Option<&Pod>) {
    let Some(param) = param else { return };
    if id != spa::param::ParamType::Format.as_raw() {
        return;
    }
    match spa::param::format_utils::parse_format(param) {
        Ok((m, s))
            if m == spa::param::format::MediaType::Video
                && s == spa::param::format::MediaSubtype::Raw => {}
        _ => return,
    }
    if cap.format.parse(param).is_err() {
        return;
    }
    let (w, h) = (cap.format.size().width, cap.format.size().height);
    if let Some(tx) = cap.dims.take() {
        let _ = tx.send((w, h));
    }
    tracing::info!(width = w, height = h, "capture format negotiated");
}

fn on_process(stream: &pw::stream::Stream, cap: &mut Cap) {
    let Some(mut buf) = stream.dequeue_buffer() else {
        return;
    };
    let datas = buf.datas_mut();
    let Some(d) = datas.first_mut() else { return };
    let chunk = d.chunk();
    let (size, stride, offset) = (
        chunk.size() as usize,
        chunk.stride() as u32,
        chunk.offset() as usize,
    );
    let Some(bytes) = d.data() else { return };
    if bytes.len() < offset + size {
        return;
    }
    let frame = VideoFrame {
        width: cap.format.size().width,
        height: cap.format.size().height,
        stride,
        format: PixelFormat::Bgra8,
        timestamp: Timestamp::from_nanos(cap.epoch.elapsed().as_nanos() as i64),
        data: bytes[offset..offset + size].to_vec(),
    };
    // Never block the PipeWire thread; drop the frame if the consumer is behind.
    let _ = cap.frames.try_send(frame);
}

fn connect(stream: &pw::stream::StreamBox, node_id: u32) -> Result<()> {
    let values = format_pod();
    let mut params = [Pod::from_bytes(&values).ok_or_else(|| Error::Pipewire("bad pod".into()))?];
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(pwerr)?;
    Ok(())
}

/// Accept the common Wayland-portal packed RGB formats. We convert to NV12 for
/// the encoder downstream (see sc-ffmpeg::Bgra2Nv12).
fn format_pod() -> Vec<u8> {
    let obj = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBA
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 7680,
                height: 4320
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner()
}
