# Architecture

SimpleClip is split into a long-lived **daemon** and several **thin clients**. The
daemon is the only component that touches capture, so a crash in any client — the
CLI, the tray, the GUI — cannot lose or corrupt the replay buffer.

```
                 ┌────────────────────────────────────────────┐
                 │  scd (daemon)                               │
   compositor    │                                            │
   keybind ──▶ sc │  CaptureBackend ─▶ Encoder ─▶ PacketRing   │
                 │        │                          │         │
   sc-gui ──────▶│  AudioSource ─────────────────────┘         │
   (wizard,      │                          save: snapshot ──▶ muxer ──▶ file
    gallery,     │                                            │
    trim, tray)  │  IPC server (versioned, newline JSON)      │
                 └────────────────────────────────────────────┘
```

## Crates

| Crate | Kind | Responsibility |
|-------|------|----------------|
| `sc-core` | lib | Shared contracts: capture/encoder/audio traits, IPC schema, config, ring buffer, timestamps, errors. No platform code. |
| `scd` | bin | The daemon. Owns capture, encode, the ring buffer, saving, and the IPC server. Survives client failure. |
| `sc` | bin | CLI control client. One-shot request/response. Also the Linux hotkey mechanism (compositor binds a key to `sc save`). |
| `sc-gui` | bin | egui client: setup wizard, gallery, trim, settings, tray. Also just an IPC client — never linked into the daemon. |

## Trait boundaries

Platform-specific code lives behind three traits in `sc-core`, selected at runtime:

- **`CaptureBackend`** → per platform (Linux portal+PipeWire, Windows WGC/DXGI, macOS later). Pushes `VideoFrame`s carrying a monotonic timestamp into a channel; never touches the encoder directly.
- **`Encoder`** → per hardware encoder (NVENC, VA-API, QSV, AMF) plus an opt-in software one. Turns frames into `EncodedPacket`s. Detection probes that the encoder actually initializes before committing.
- **`AudioSource`** → per platform audio system. Desktop and mic are separate sources on the same timebase as video; track layout is decided at save time.

Because everything crosses these traits, the buffer/save logic is testable against mock backends with no display present.

## The replay buffer

`PacketRing` (in `sc-core/src/buffer.rs`) is a byte-capped ring of **encoded**
packets — not raw frames, which at 1080p60 would blow the RAM budget. It stores
compressed video and audio packets with timestamps. Eviction drops **whole leading
GOPs**, so the front is always a keyframe and a saved clip can start cleanly. One
ring per track (video, desktop audio, mic).

Saving is non-blocking: `snapshot(secs)` clones the last N seconds off the capture
thread and hands them to a writer task; capture never pauses and quality never dips
(§6.3 of the brief).

## IPC

Transport is a local socket (`interprocess`): Unix domain socket on Linux, named
pipe on Windows, one code path. Messages are newline-delimited JSON wrapped in an
`Envelope { version, id, payload }`. The `version` field is checked on every read;
once published, only additive changes are allowed within a version. Schema lives in
`sc-core/src/ipc.rs`.

## Config

TOML in the platform config dir (`~/.config/simpleclip/` via XDG, `%APPDATA%\SimpleClip\`
on Windows). Hand-editable, hot-reloaded by a file watcher on its own thread. An
invalid config is logged and ignored, keeping the daemon on the last-good config
rather than crashing.

## What is NOT here yet

Phase 0 ships the skeleton and contracts only. Capture, encode, audio, save, tray,
and GUI are stubbed behind their traits and filled in phase by phase — see the phase
plan in the build brief and `docs/DECISIONS.md` for resolved choices.
