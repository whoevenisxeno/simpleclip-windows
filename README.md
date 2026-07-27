# SimpleClip

SimpleClip is a lightweight instant-replay clipping application for Windows and Linux. It continuously buffers your screen and audio in memory and writes the last N seconds to disk on a hotkey press. There is no record button and no interruption to the buffer while a clip is saved.

It runs as a background daemon (`scd`) with a CLI (`sc`) and a small tray/GUI client on top. The daemon is the only part that has to stay alive; the CLI, tray icon, and hotkey layer can all fail or restart without losing the buffer.

## Features

- Rolling replay buffer with configurable duration (15s / 30s / 1min / custom), capped by a RAM budget you set
- Global hotkey saves the buffer to a file without pausing or dropping capture
- Manual start/stop recording, separate from the replay path
- Screenshot hotkey, pulled from the live capture
- Clip trimming (stream-copy on keyframes, re-encode only when a frame-accurate cut is requested)
- Clip gallery grouped by date, with playback and reveal-in-folder
- First-launch setup wizard: monitor, microphone, replay duration, save location, hotkeys, quality
- System tray with capture state (active / paused / needs re-authorization)
- Post-save hook: runs a user-defined script with a JSON event after every save
- Full CLI + local IPC surface, usable standalone without the GUI

## Capture

- **Windows 11**: Windows Graphics Capture, falling back to DXGI Desktop Duplication. Hardware encode via NVENC, AMF, or QSV depending on GPU.
- **Linux**: `xdg-desktop-portal` + PipeWire. Tested on Hyprland and niri; also works on KWin, Mutter, and X11. Hardware encode via NVENC or VA-API. PipeWire is required; there is no PulseAudio-only path.
- macOS is not supported.

Encoding defaults to H.264 for compatibility. HEVC and AV1 are available as opt-in settings. Software encoding is only used if no hardware encoder is detected, and is off by default in release builds.

## Installation

**Windows**
```
winget install SimpleClip
```
or download the portable build from the Releases page.

**Linux**
```
# Arch / AUR
yay -S simpleclip

# AppImage
chmod +x SimpleClip-*.AppImage && ./SimpleClip-*.AppImage
```
A Flatpak build is also available; some capture paths and the evdev hotkey fallback are constrained under sandboxing (documented in `docs/`).

## Usage

On first launch, the setup wizard walks through monitor, microphone, buffer duration, save location, and hotkeys.

On Linux, hotkeys are bound in your compositor config and mapped to CLI commands, for example on Hyprland:
```
bind = SUPER, F10, exec, sc save
bind = SUPER, F11, exec, sc screenshot
```
On Windows, hotkeys are registered directly by the app during setup.

CLI reference:

| Command | Description |
|---|---|
| `sc save [--last SECONDS]` | Save the last N seconds from the buffer (defaults to configured duration) |
| `sc screenshot` | Save a still frame from the live capture |
| `sc record` / `sc stop` | Start/stop a manual recording |
| `sc status` | Show capture state, buffer fill, selected monitor, encoder, and A/V drift (`--json` for machine-readable output) |
| `sc pause` / `sc resume` | Pause or resume capture |
| `scd` | The daemon itself; run with `--foreground` or `--verbose` for debugging |

## Configuration

Settings are stored as hand-editable TOML and hot-reloaded on change:

- Linux: `~/.config/simpleclip/config.toml`
- Windows: `%APPDATA%\SimpleClip\config.toml`

An invalid config keeps the daemon running on the last known-good settings and raises a tray warning instead of crashing.

## Documentation

- `ARCHITECTURE.md` — daemon/client split, capture/encoder/audio trait boundaries, buffer design
- `docs/DECISIONS.md` — resolved design decisions and their rationale
- `docs/OPEN-QUESTIONS.md` — unresolved technical questions being tracked

## Privacy

No telemetry, no accounts, no network calls beyond an optional, disableable update check. All logs are local.

## License

Core project is dual-licensed MIT OR Apache-2.0. Any GPL-licensed component (e.g. an optional x264 software encoder) is isolated behind an opt-in build feature and does not affect the default build's license.

## Contributing

Contributions are handled through pull requests against `main`. See `CONTRIBUTING.md` for coding standards, commit conventions, and the CI requirements (fmt, clippy, license/advisory checks) that PRs must pass.
