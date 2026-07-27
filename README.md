# SimpleClip

Instant replay for Linux. SimpleClip always keeps the last N seconds of your
screen buffered in RAM. When something worth keeping happens, you hit a hotkey
and it writes those seconds to a file. There is no record button, and the buffer
never stops.

Think ShadowPlay / Medal instant replay, but a small daemon and a hotkey instead
of a launcher and an account.

## Status

Usable. The core loop works today: the daemon buffers continuously and a hotkey
saves the last N seconds to a valid, shareable MP4. Rough edges remain (see
Roadmap). Linux only for now.

- Rolling replay buffer, hardware encoded
- Hotkey to save the last N seconds (set from the app, no root, no manual config)
- GUI: setup wizard, live dashboard, clip gallery, and a lot of settings
- Desktop notification and sound on save
- Everything runs unprivileged

## Requirements

- A modern Linux desktop on Wayland with PipeWire and `xdg-desktop-portal`
- A GPU with VA-API H.264 encode (AMD or Intel)
- Hyprland for the fully automatic hotkey setup. Other compositors work too, you
  just bind the key to `sc save` yourself.

## Install

```
git clone https://github.com/whoevenisxeno/SimpleClip
cd SimpleClip
./packaging/install.sh
```

This builds release binaries and installs `scd`, `sc`, and `sc-gui` to
`~/.local/bin` with a desktop launcher. On Hyprland, the daemon manages its own
hotkey config, so once it runs there is nothing else to set up.

## Usage

- The daemon `scd` runs in the background and buffers continuously.
- Press the save hotkey (default SUPER+F10) to write the last N seconds.
- Run `sc-gui` for the wizard, live dashboard, gallery, and settings.
- Or drive it from the CLI:

```
sc save --last 30   # save the last 30 seconds
sc status           # capture state, buffer fill, encoder
sc pause / sc resume
```

## How it works

A long-lived daemon (`scd`) owns capture and the buffer; `sc` and `sc-gui` are
thin clients that talk to it over a local socket. Only the daemon has to stay
alive, so the GUI or a hotkey can crash without losing the buffer.

Capture goes through `xdg-desktop-portal` and PipeWire. Frames are converted and
encoded on the GPU (VA-API H.264) into a rolling ring buffer of encoded packets.
Saving snapshots the buffer and muxes a keyframe-aligned faststart MP4 without
ever stalling capture.

## Configuration

Settings live in `~/.config/simpleclip/config.toml` (hand-editable, hot-reloaded)
and are also exposed in the GUI: buffer length, RAM cap, bitrate, codec, capture
FPS, cursor, save location and naming, notifications, and the hotkey.

## Roadmap

- Audio (desktop + mic) in the clips
- Screenshot hotkey
- System tray icon
- Restore token so the screen-share prompt only appears once
- NVIDIA (NVENC) encode path
- Packaging: AUR, AppImage

## License

MIT OR Apache-2.0.
