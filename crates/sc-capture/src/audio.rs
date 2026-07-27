use crate::{Error, Result};
use pipewire as pw;
use pw::{properties::properties, spa};
use sc_core::audio::{AudioBuffer, AudioRole};
use sc_core::time::Timestamp;
use spa::pod::Pod;
use std::time::Instant;

struct Cap {
    tx: crossbeam_channel::Sender<AudioBuffer>,
    epoch: Instant,
    format: spa::param::audio::AudioInfoRaw,
}

/// Capture desktop audio (the default sink's monitor) as interleaved f32,
/// timestamped on the shared capture clock so it stays aligned with video.
/// Blocks in the PipeWire mainloop until stopped.
pub fn run(
    tx: crossbeam_channel::Sender<AudioBuffer>,
    epoch: Instant,
    stop: pipewire::channel::Receiver<()>,
) -> Result<()> {
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(pwerr)?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(pwerr)?;
    let core = context.connect_rc(None).map_err(pwerr)?;

    let quit = mainloop.clone();
    let _stop = stop.attach(mainloop.loop_(), move |_| quit.quit());

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
    };
    props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");

    let stream = pw::stream::StreamBox::new(&core, "sc-audio", props).map_err(pwerr)?;
    let data = Cap {
        tx,
        epoch,
        format: Default::default(),
    };
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, cap, id, param| on_param(cap, id, param))
        .process(on_process)
        .register()
        .map_err(pwerr)?;

    connect(&stream)?;
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
            if m == spa::param::format::MediaType::Audio
                && s == spa::param::format::MediaSubtype::Raw => {}
        _ => return,
    }
    if cap.format.parse(param).is_err() {
        return;
    }
    tracing::info!(
        rate = cap.format.rate(),
        channels = cap.format.channels(),
        "audio format negotiated"
    );
}

fn on_process(stream: &pw::stream::Stream, cap: &mut Cap) {
    let Some(mut buf) = stream.dequeue_buffer() else {
        return;
    };
    let datas = buf.datas_mut();
    let Some(d) = datas.first_mut() else { return };
    let chunk = d.chunk();
    let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
    // bytes per full audio frame (all channels); at least one f32 sample.
    let stride = (chunk.stride() as usize).max(4);
    let Some(bytes) = d.data() else { return };
    // Honor the buffer offset; a non-zero offset would otherwise read garbage
    // (which sounds like static).
    let end = (offset + size).min(bytes.len());
    let start = offset.min(end);
    let valid = (end - start) / stride * stride;
    let samples: Vec<f32> = bytes[start..start + valid]
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if samples.is_empty() {
        return;
    }
    let frame = AudioBuffer {
        role: AudioRole::Desktop,
        sample_rate: cap.format.rate(),
        channels: cap.format.channels() as u16,
        timestamp: Timestamp::from_nanos(cap.epoch.elapsed().as_nanos() as i64),
        samples,
    };
    let _ = cap.tx.try_send(frame);
}

fn connect(stream: &pw::stream::StreamBox) -> Result<()> {
    let mut info = spa::param::audio::AudioInfoRaw::new();
    info.set_format(spa::param::audio::AudioFormat::F32LE);
    let obj = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    let values: Vec<u8> = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();
    let mut params =
        [Pod::from_bytes(&values).ok_or_else(|| Error::Pipewire("bad audio pod".into()))?];
    stream
        .connect(
            spa::utils::Direction::Input,
            None,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(pwerr)?;
    Ok(())
}
