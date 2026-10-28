use crate::{Error, Options, Result};
use sc_core::audio::{AudioBuffer, AudioDevice, AudioRole};
use sc_core::capture::{FramePool, MonitorInfo, PixelFormat, VideoFrame};
use sc_core::time::Timestamp;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

#[derive(Default)]
struct Slot {
    data: Vec<u8>,
    width: u32,
    height: u32,
    stride: u32,
    has_frame: bool,
}

struct Grabber {
    slot: Arc<Mutex<Slot>>,
    scratch: Vec<u8>,
}

impl GraphicsCaptureApiHandler for Grabber {
    type Flags = Arc<Mutex<Slot>>;
    type Error = BoxErr;

    fn new(ctx: Context<Self::Flags>) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            slot: ctx.flags,
            scratch: Vec::new(),
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        _control: InternalCaptureControl,
    ) -> std::result::Result<(), Self::Error> {
        let fb = frame.buffer()?;
        let (w, h) = (fb.width(), fb.height());
        let bytes = fb.as_nopadding_buffer(&mut self.scratch);
        let mut s = self.slot.lock().unwrap();
        s.data.clear();
        s.data.extend_from_slice(bytes);
        s.width = w;
        s.height = h;
        s.stride = w * 4;
        s.has_frame = true;
        Ok(())
    }
}

type Control = CaptureControl<Grabber, BoxErr>;

pub struct CaptureHandle {
    pub width: u32,
    pub height: u32,
    stop: Arc<AtomicBool>,
    slot: Arc<Mutex<Slot>>,
    control: Option<Control>,
    pacer: Option<JoinHandle<()>>,
    audio: Vec<JoinHandle<()>>,
}

impl CaptureHandle {
    pub fn latest_frame(&self) -> Option<VideoFrame> {
        let s = self.slot.lock().unwrap();
        if !s.has_frame {
            return None;
        }
        let (width, height) = (s.width & !1, s.height & !1);
        Some(VideoFrame {
            width,
            height,
            stride: s.stride,
            format: PixelFormat::Bgra8,
            timestamp: Timestamp::from_nanos(0),
            data: s.data[..(s.stride * height) as usize].to_vec().into(),
        })
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(c) = self.control.take() {
            if let Err(e) = c.stop() {
                tracing::warn!(error = %e, "stopping graphics capture");
            }
        }
        if let Some(t) = self.pacer.take() {
            let _ = t.join();
        }
        for t in self.audio.drain(..) {
            let _ = t.join();
        }
    }
}

fn werr(e: impl std::fmt::Display) -> Error {
    Error::Windows(e.to_string())
}

fn pick_monitor(id: &str) -> Result<Monitor> {
    if let Ok(idx) = id.trim().parse::<usize>() {
        match Monitor::from_index(idx) {
            Ok(m) => return Ok(m),
            Err(e) => tracing::warn!(monitor = idx, error = %e, "monitor not found; using primary"),
        }
    }
    Monitor::primary().map_err(werr)
}

pub fn start(
    frames: crossbeam_channel::Sender<VideoFrame>,
    audio: Option<crossbeam_channel::Sender<AudioBuffer>>,
    opts: &Options,
) -> Result<CaptureHandle> {
    let epoch = Instant::now();
    let monitor = pick_monitor(&opts.monitor_id)?;
    let slot = Arc::new(Mutex::new(Slot::default()));

    let cursor = if opts.show_cursor {
        CursorCaptureSettings::WithCursor
    } else {
        CursorCaptureSettings::WithoutCursor
    };
    let make_settings = |border| {
        Settings::new(
            monitor,
            cursor,
            border,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            slot.clone(),
        )
    };
    let control =
        match Grabber::start_free_threaded(make_settings(DrawBorderSettings::WithoutBorder)) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "capture without border failed; retrying with default");
                Grabber::start_free_threaded(make_settings(DrawBorderSettings::Default))
                    .map_err(werr)?
            }
        };

    let deadline = Instant::now() + Duration::from_secs(5);
    let (width, height) = loop {
        {
            let s = slot.lock().unwrap();
            if s.has_frame {
                break (s.width & !1, s.height & !1);
            }
        }
        if Instant::now() > deadline {
            let _ = control.stop();
            return Err(werr("no frame received from Graphics Capture"));
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let stop = Arc::new(AtomicBool::new(false));
    let fps = opts.fps.clamp(1, 240);

    let pacer = {
        let (slot, stop) = (slot.clone(), stop.clone());
        std::thread::Builder::new()
            .name("sc-capture-pacer".into())
            .spawn(move || pace(slot, frames, stop, epoch, fps, width, height))
            .map_err(werr)?
    };

    let mut audio_threads = Vec::new();
    if let Some(tx) = audio {
        let mut sources = Vec::new();
        if opts.desktop_audio {
            sources.push(Source {
                role: AudioRole::Desktop,
                device: opts.audio_device.clone(),
            });
        }
        if opts.mic_enabled {
            sources.push(Source {
                role: AudioRole::Microphone,
                device: opts.mic_device.clone(),
            });
        }
        for src in sources {
            let (tx, stop) = (tx.clone(), stop.clone());
            audio_threads.push(
                std::thread::Builder::new()
                    .name(format!("sc-capture-{}", src.label().replace(' ', "-")))
                    .spawn(move || capture_audio(tx, epoch, stop, src))
                    .map_err(werr)?,
            );
        }
    }

    Ok(CaptureHandle {
        width,
        height,
        stop,
        slot,
        control: Some(control),
        pacer: Some(pacer),
        audio: audio_threads,
    })
}

fn pace(
    slot: Arc<Mutex<Slot>>,
    tx: crossbeam_channel::Sender<VideoFrame>,
    stop: Arc<AtomicBool>,
    epoch: Instant,
    fps: u32,
    width: u32,
    height: u32,
) {
    let period = Duration::from_nanos(1_000_000_000 / fps as u64);
    let mut next = Instant::now();
    let pool = FramePool::new(1);
    let mut warned = false;
    let (mut sent, mut dropped, mut report) = (0u32, 0u32, Instant::now());
    while !stop.load(Ordering::Relaxed) {
        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else if now - next > period * 4 {
            next = now;
        }
        let frame = {
            let s = slot.lock().unwrap();
            if s.width & !1 != width || s.height & !1 != height {
                if !warned {
                    tracing::warn!(
                        "capture size changed ({}x{} -> {}x{}); frames paused until restart",
                        width,
                        height,
                        s.width,
                        s.height
                    );
                    warned = true;
                }
                continue;
            }
            VideoFrame {
                width,
                height,
                stride: s.stride,
                format: PixelFormat::Bgra8,
                timestamp: Timestamp::from_nanos(epoch.elapsed().as_nanos() as i64),
                data: pool.copy_of(&s.data[..(s.stride * height) as usize]),
            }
        };
        match tx.try_send(frame) {
            Ok(()) => sent += 1,
            Err(_) => dropped += 1,
        }
        if report.elapsed() >= Duration::from_secs(5) {
            tracing::debug!(sent, dropped, "capture pacer, last 5s");
            (sent, dropped, report) = (0, 0, Instant::now());
        }
    }
}

const RATE: u32 = 48_000;
const CHANNELS: u16 = 2;

#[derive(Clone)]
struct Source {
    role: AudioRole,
    device: String,
}

impl Source {
    fn is_loopback(&self) -> bool {
        self.role == AudioRole::Desktop
    }

    fn label(&self) -> &'static str {
        match self.role {
            AudioRole::Desktop => "desktop audio",
            AudioRole::Microphone => "microphone",
        }
    }
}

fn capture_audio(
    tx: crossbeam_channel::Sender<AudioBuffer>,
    epoch: Instant,
    stop: Arc<AtomicBool>,
    src: Source,
) {
    if let Err(e) = wasapi::initialize_mta().ok() {
        tracing::warn!(error = %e, "COM init for audio failed");
        return;
    }
    let mut produced_frames: u64 = 0;
    let t0 = epoch.elapsed();
    let mut failures = 0u32;
    while !stop.load(Ordering::Relaxed) {
        if let Err(e) = audio_session(&tx, epoch, t0, &stop, &src, &mut produced_frames) {
            failures += 1;
            if failures == 1 {
                tracing::warn!(error = %e, source = src.label(), "audio capture interrupted; retrying");
            }
            for _ in 0..10 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            pad_silence(&tx, src.role, epoch, t0, &mut produced_frames, 0);
        }
    }
}

fn pad_silence(
    tx: &crossbeam_channel::Sender<AudioBuffer>,
    role: AudioRole,
    epoch: Instant,
    t0: Duration,
    produced_frames: &mut u64,
    incoming: u64,
) {
    let elapsed = epoch.elapsed().saturating_sub(t0);
    let expected = elapsed.as_nanos() as u64 * RATE as u64 / 1_000_000_000;
    let have = *produced_frames + incoming;
    if expected > have + (RATE as u64 / 20) {
        let gap = expected - have;
        send(
            tx,
            role,
            t0,
            produced_frames,
            vec![0.0; gap as usize * CHANNELS as usize],
        );
    }
}

fn send(
    tx: &crossbeam_channel::Sender<AudioBuffer>,
    role: AudioRole,
    t0: Duration,
    produced_frames: &mut u64,
    samples: Vec<f32>,
) {
    let frames = (samples.len() / CHANNELS as usize) as u64;
    let ts = t0.as_nanos() as i64 + (*produced_frames * 1_000_000_000 / RATE as u64) as i64;
    *produced_frames += frames;
    let _ = tx.send(AudioBuffer {
        role,
        sample_rate: RATE,
        channels: CHANNELS,
        timestamp: Timestamp::from_nanos(ts),
        samples,
    });
}

fn audio_session(
    tx: &crossbeam_channel::Sender<AudioBuffer>,
    epoch: Instant,
    t0: Duration,
    stop: &AtomicBool,
    src: &Source,
    produced_frames: &mut u64,
) -> std::result::Result<(), BoxErr> {
    use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

    let enumerator = DeviceEnumerator::new()?;
    let kind = if src.is_loopback() {
        Direction::Render
    } else {
        Direction::Capture
    };
    let dev = if src.device.is_empty() {
        enumerator.get_default_device(&kind)?
    } else {
        enumerator.get_device(&src.device)?
    };
    let mut client = dev.get_iaudioclient()?;
    let fmt = WaveFormat::new(
        32,
        32,
        &SampleType::Float,
        RATE as usize,
        CHANNELS as usize,
        None,
    );
    let (period, _min) = client.get_device_period()?;
    client.initialize_client(
        &fmt,
        &Direction::Capture,
        &StreamMode::PollingShared {
            autoconvert: true,
            buffer_duration_hns: period.max(1_000_000),
        },
    )?;
    let cap = client.get_audiocaptureclient()?;
    let block = fmt.get_blockalign() as usize;
    client.start_stream()?;
    tracing::info!(device = %dev.get_friendlyname().unwrap_or_default(), source = src.label(), "audio capture started");

    let mut buf = vec![0u8; block * RATE as usize / 2];
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(10));
        let mut samples: Vec<f32> = Vec::new();
        while let Some(n) = cap.get_next_packet_size()? {
            if n == 0 {
                break;
            }
            let (frames, _info) = cap.read_from_device(&mut buf)?;
            samples.extend(
                buf[..frames as usize * block]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|c| f32::from_le_bytes(*c)),
            );
        }
        let incoming = (samples.len() / CHANNELS as usize) as u64;
        pad_silence(tx, src.role, epoch, t0, produced_frames, incoming);
        if !samples.is_empty() {
            send(tx, src.role, t0, produced_frames, samples);
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

pub fn list_monitors() -> Result<Vec<MonitorInfo>> {
    let primary = Monitor::primary().ok();
    Monitor::enumerate()
        .map_err(werr)?
        .into_iter()
        .map(|m| {
            let index = m.index().map_err(werr)?;
            let origin = monitor_origin(&m);
            Ok(MonitorInfo {
                id: index.to_string(),
                name: m.name().unwrap_or_else(|_| format!("Display {index}")),
                width: m.width().unwrap_or(0),
                height: m.height().unwrap_or(0),
                refresh_mhz: m.refresh_rate().unwrap_or(0) * 1000,
                primary: primary == Some(m),
                x: origin.0,
                y: origin.1,
            })
        })
        .collect()
}

pub fn list_audio_devices() -> Result<Vec<AudioDevice>> {
    use wasapi::{DeviceEnumerator, Direction};
    let _ = wasapi::initialize_mta();
    let en = DeviceEnumerator::new().map_err(werr)?;
    let mut out = Vec::new();
    for (dir, is_monitor) in [(Direction::Render, true), (Direction::Capture, false)] {
        let coll = en.get_device_collection(&dir).map_err(werr)?;
        for dev in &coll {
            let dev = dev.map_err(werr)?;
            out.push(AudioDevice {
                id: dev.get_id().map_err(werr)?,
                name: dev.get_friendlyname().unwrap_or_default(),
                is_monitor,
            });
        }
    }
    Ok(out)
}

fn monitor_origin(m: &Monitor) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, HMONITOR, MONITORINFO};
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let ok = unsafe { GetMonitorInfoW(HMONITOR(m.as_raw_hmonitor()), &mut info) }.as_bool();
    if ok {
        (info.rcMonitor.left, info.rcMonitor.top)
    } else {
        (0, 0)
    }
}
