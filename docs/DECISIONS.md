# Decisions

Lightweight ADRs. Each entry records a resolved choice and why, so we don't
relitigate it. Newest first.

## D-0010 · Windows backend: windows-capture + WASAPI loopback + FFmpeg hw encoders
Phase 4. Resolves Q2.
- **Video:** `windows-capture` (Windows Graphics Capture), BGRA CPU readback. WGC
  only delivers on change, so a pacer thread re-emits the latest frame at the
  target fps: constant frame rate, keyframes on a static desktop, and a cap on
  high-refresh monitors. Frame size from the first frame is authoritative (DPI).
- **Audio:** `wasapi` shared-mode loopback, 48 kHz stereo f32 with autoconvert.
  Loopback sends nothing while the system is silent, so gaps are filled with
  silence against the shared clock to keep A/V aligned.
- **Encode:** the existing FFmpeg path. NVENC, AMF and QSV take system-memory NV12
  (no hw frame pool), tried in that order, then libx264 (GPL builds only), then
  Media Foundation `h264_mf`. Each candidate is probed by really opening it. AMF
  ignores `gop_size`, which would leave one unbounded GOP the ring can't evict, so
  an IDR is forced every `gop` frames via `pict_type`. BGRA->NV12 uses BT.709
  coefficients and the stream is tagged BT.709/limited (swscale defaults to 601).
- **Pipeline:** colour conversion and encoding run on separate threads so each
  gets a full frame interval at 60 fps.
- **Shell:** `RegisterHotKey` on its own thread (re-registers on config change),
  `tray-icon`, toast via PowerShell/WinRT with its own AUMID, HKCU Run key for
  autostart, a named mutex for single instance, a file log (the daemon is a
  GUI-subsystem exe).
- **Licensing:** packages ship an LGPL FFmpeg by default (D-0002). `-Variant gpl`
  in `packaging/windows/build.ps1` opts into libx264.

## D-0009 · Linux capture: ashpd (portal ScreenCast) + pipewire-rs 0.10
Phase 1. A spike proved the full path on Hyprland: `ashpd` drives the
xdg-desktop-portal ScreenCast (create session, select monitor, start, open the
PipeWire remote fd), and `pipewire` 0.10 consumes the node. Findings that shape
the real backend:
- **Runtime:** ashpd with the `async-io` backend + `pollster::block_on` avoids
  pulling tokio into the daemon. The blocking PipeWire mainloop runs on its own
  thread while the portal session is held alive in scope.
- **Pixel format:** the compositor delivers **BGRA** (4 bpp, stride = width*4).
  VA-API wants NV12, so a **swscale BGRA->NV12** step sits between capture and the
  encoder (this is the Q7 CPU-download cost, made concrete).
- **Framerate is dynamic (`0/1`):** frames arrive on damage, not a fixed cadence.
  Frames must be timestamped from the PipeWire buffer clock, never an assumed fps
  - load-bearing for the <50ms A/V sync target.
- **Versions matter:** pipewire/libspa **0.10** build against system PipeWire
  1.6.8; 0.8 does not (`spa_pod_builder` layout changed). Do not depend on ashpd's
  bundled `pipewire` feature - it collides on the `pipewire-0.3` links key.

## D-0008 · FFmpeg binding: `ffmpeg-next`, dynamically linked to system FFmpeg
Phase 1. A spike confirmed `ffmpeg-next` 8.1.0 builds and links against the
system's FFmpeg 8.1 (libavcodec 62) - its major version tracks FFmpeg's, which
sidesteps the usual "binding lags the system lib" trap. It finds `h264_vaapi` /
`hevc_vaapi` and re-exports raw FFI (`ffmpeg_next::ffi`) for the hardware-context
work the safe API doesn't cover. VA-API `av_hwdevice_ctx_create` opened cleanly on
an AMD GPU. Linux links the distro FFmpeg (LGPL); Windows bundling is
decided at Phase 4. Note: system FFmpeg ships `libx264` but not `libopenh264`,
which feeds the Q3 software-fallback choice - irrelevant where hardware works.

## D-0007 · IPC framing: newline-delimited JSON
Phase 0. Debuggable by eye and with `nc`, trivially versioned via an envelope,
zero schema-compiler step. The brief allows revisiting only if profiling shows it
matters; capture/encode dominate cost, not control-plane messages. Envelope carries
a `version` field, checked on every read.

## D-0006 · Local socket via `interprocess`
Phase 0. One API for Unix domain sockets and Windows named pipes, so the daemon and
clients share a single IPC code path. Namespaced names (`GenericNamespaced`) avoid
stale socket files on Linux (abstract namespace) and map cleanly to named pipes on
Windows. Socket name overridable via `SC_SOCKET` for tests / side-by-side daemons.

## D-0005 · Ring stores encoded packets, evicts whole GOPs
Phase 0. Raw 1080p60 is ~370 MB/s - untenable in RAM. We buffer encoded packets
with timestamps instead. Eviction removes whole leading GOPs so the front is always
a keyframe and a saved clip starts cleanly. Trade: needs a short GOP (1-2s) for tight
"last N seconds" boundaries; documented in the encoder config.

## D-0004 · Daemon + thin clients
Phase 0. Reliability pillar. `scd` owns capture and the buffer; `sc`, the tray, and
`sc-gui` are all just IPC clients. A UI or hotkey crash cannot take down capture.

## D-0003 · egui for the GUI, no webview
Phase 0 (implementation Phase 5). Pure-Rust, single-binary, trivial cross-compile,
keeps the lightweight promise. Tauri/webview rejected - the runtime weight
contradicts the footprint goal. Native-look is the accepted trade.

## D-0002 · FFmpeg/libav for encode + mux, LGPL link
Phase 0 (implementation Phase 1). One dependency covers every hardware encoder
(NVENC/VA-API/QSV/AMF), software fallback, and muxing on both OSes. Link LGPL
FFmpeg; any GPL piece (x264) stays behind an opt-in Cargo feature so the default
build is permissive. `libobs` rejected: GPL-3.0 and requires shipping OBS.

## D-0001 · Rust, workspace of four crates
Phase 0. Memory safety for a 24/7 background process, mature capture/encode crates,
single-binary distribution. Layout: `sc-core` (contracts), `scd` (daemon),
`sc` (CLI), `sc-gui` (egui client). MSRV = latest stable - 2.

## D-0000 · Bootstrap Linux-first
Phase 0. The original development setup was Hyprland/Wayland with an AMD GPU
(VA-API), so Linux gives the fastest dogfooding loop. The platform abstraction is
designed up front so Windows slots in at Phase 4 without rework. This sequences the
work; it does not demote Windows.

---

### Dev-machine facts (reference implementation target)
- Compositor: Hyprland (Wayland), priority tier-1 target
- GPU: AMD → VA-API encode (H.264 High/Main + HEVC confirmed via `vainfo`)
- PipeWire 1.6.8, FFmpeg n8.1.2 (libavcodec 62), Rust 1.97.1
