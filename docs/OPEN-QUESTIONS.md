# Open questions

Each is resolved with a short spike **before** the phase that needs it, then moved
to `DECISIONS.md`. Status: 🔴 open · 🟡 spiking · 🟢 resolved (see ADR).

| # | Question | Blocks | Status |
|---|----------|--------|--------|
| Q1 | Maintained FFmpeg/libav Rust binding exposing NVENC/VA-API/QSV/AMF on both OSes; system-link vs bundle per platform. | Phase 1 | 🟢 D-0008 |
| Q2 | Windows capture crate (`windows-capture`?) frame format and lowest-copy path into libav. | Phase 4 | 🔴 |
| Q3 | Software-encoder default: openh264 (BSD, patent nuance) vs x264 (GPL, feature-gated). Leaning openh264 default. | Phase 1 | 🔴 |
| Q4 | Confirm newline-JSON IPC isn't a bottleneck under real load. | - | 🟢 D-0007 (revisit if profiled) |
| Q5 | egui video-playback for gallery/trim: embed frames vs delegate to system player. | Phase 6 | 🔴 |
| Q6 | GlobalShortcuts portal status on Hyprland / niri / KWin / Mutter (secondary hotkey path only). | Phase 3 | 🔴 |
| Q7 | Zero-copy GPU-frame → encoder path. v1 uses CPU-download: portal delivers BGRA in mapped memory → swscale BGRA→NV12 → VA-API upload. Proven working; cost = one swscale + one GPU upload per frame. DMA-BUF zero-copy is a later optimization. | Phase 1 | 🟡 CPU path |
| Q8 | Exclusive-fullscreen "no frames" detection heuristics per platform. | Phase 3/4 | 🔴 |

## Spike plan for the next phase (Phase 1, Linux)

- **Q1** - evaluate `ffmpeg-next` vs `ffmpeg-sys-next` against system FFmpeg n8.1.2
  (already installed). Confirm a VA-API H.264 encoder can be opened and fed frames.
  On Linux, dynamically link system FFmpeg (distro-provided, LGPL). Decide bundling
  for the Windows build at Phase 4.
- **Q3** - only matters when no hardware encoder exists; the dev machine has VA-API,
  so defer the software-encoder choice but keep it behind the `software-encode`
  feature flag from the start.
- **Q7** - start with a CPU-download path (portal/PipeWire → mapped buffer → libav)
  for correctness; measure CPU cost against the <5%-of-one-core budget and record it.
