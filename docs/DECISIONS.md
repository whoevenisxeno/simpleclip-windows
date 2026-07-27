# Decisions

Lightweight ADRs. Each entry records a resolved choice and why, so we don't
relitigate it. Newest first.

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
  — load-bearing for the <50ms A/V sync target.
- **Versions matter:** pipewire/libspa **0.10** build against system PipeWire
  1.6.8; 0.8 does not (`spa_pod_builder` layout changed). Do not depend on ashpd's
  bundled `pipewire` feature — it collides on the `pipewire-0.3` links key.

## D-0008 · FFmpeg binding: `ffmpeg-next`, dynamically linked to system FFmpeg
Phase 1. A spike confirmed `ffmpeg-next` 8.1.0 builds and links against the
system's FFmpeg 8.1 (libavcodec 62) — its major version tracks FFmpeg's, which
sidesteps the usual "binding lags the system lib" trap. It finds `h264_vaapi` /
`hevc_vaapi` and re-exports raw FFI (`ffmpeg_next::ffi`) for the hardware-context
work the safe API doesn't cover. VA-API `av_hwdevice_ctx_create` opened cleanly on
the AMD RX 6650 XT. Linux links the distro FFmpeg (LGPL); Windows bundling is
decided at Phase 4. Note: system FFmpeg ships `libx264` but not `libopenh264`,
which feeds the Q3 software-fallback choice — irrelevant where hardware works.

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
Phase 0. Raw 1080p60 is ~370 MB/s — untenable in RAM. We buffer encoded packets
with timestamps instead. Eviction removes whole leading GOPs so the front is always
a keyframe and a saved clip starts cleanly. Trade: needs a short GOP (1–2s) for tight
"last N seconds" boundaries; documented in the encoder config.

## D-0004 · Daemon + thin clients
Phase 0. Reliability pillar. `scd` owns capture and the buffer; `sc`, the tray, and
`sc-gui` are all just IPC clients. A UI or hotkey crash cannot take down capture.

## D-0003 · egui for the GUI, no webview
Phase 0 (implementation Phase 5). Pure-Rust, single-binary, trivial cross-compile,
keeps the lightweight promise. Tauri/webview rejected — the runtime weight
contradicts the footprint goal. Native-look is the accepted trade.

## D-0002 · FFmpeg/libav for encode + mux, LGPL link
Phase 0 (implementation Phase 1). One dependency covers every hardware encoder
(NVENC/VA-API/QSV/AMF), software fallback, and muxing on both OSes. Link LGPL
FFmpeg; any GPL piece (x264) stays behind an opt-in Cargo feature so the default
build is permissive. `libobs` rejected: GPL-3.0 and requires shipping OBS.

## D-0001 · Rust, workspace of four crates
Phase 0. Memory safety for a 24/7 background process, mature capture/encode crates,
single-binary distribution. Layout: `sc-core` (contracts), `scd` (daemon),
`sc` (CLI), `sc-gui` (egui client). MSRV = latest stable − 2.

## D-0000 · Bootstrap Linux-first
Phase 0. The maintainer's daily driver is Hyprland/Wayland on an AMD RX 6650 XT
(VA-API), so Linux gives the fastest dogfooding loop. The platform abstraction is
designed up front so Windows slots in at Phase 4 without rework. This sequences the
work; it does not demote Windows.

---

### Dev-machine facts (reference implementation target)
- Compositor: Hyprland (Wayland), priority tier-1 target
- GPU: AMD Radeon RX 6650 XT → VA-API encode (H.264 High/Main + HEVC confirmed via `vainfo`)
- PipeWire 1.6.8, FFmpeg n8.1.2 (libavcodec 62), Rust 1.97.1
