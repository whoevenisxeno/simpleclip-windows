# SimpleClip

Instant replay clipping that doesn't make you install OBS, run a webview, or pray your compositor supports some portal API from three months ago.

You never hit record. SimpleClip is always buffering the last N seconds of your screen (plus audio) in RAM. Something worth keeping happens, you smash a hotkey, and it dumps the buffer to a file. Buffering never stops. That's the whole app.

Built for gamers who want ShadowPlay / Instant Replay / Medal, minus the bloat, minus the Windows-only lock-in, and minus the account you never asked to create.

## Why this exists

Every "instant replay" tool worth using is either Windows-only, tied to a specific GPU vendor, or a whole streaming suite (OBS) bolted onto a feature that should be a small daemon and a hotkey. Nothing decent exists for Linux desktops running Hyprland or niri. This is an attempt to fix that without dragging the Windows side down to match — both platforms are first-class, Linux just gets built first because it's what I actually use every day.

## Status

Early. Nothing here is production-ready yet — check the phase checklist below for where things actually stand before assuming any of this works.

- [ ] Phase 0 — workspace skeleton, capture/encoder/audio traits, IPC schema, CI
- [ ] Phase 1 — Linux capture → encode → file (manual record, no ring buffer yet)
- [ ] Phase 2 — the actual ring buffer + `sc save`
- [ ] Phase 3 — hotkeys via compositor bind, tray icon, notifications
- [ ] Phase 4 — Windows backend (WGC/DXGI, WASAPI)
- [ ] Phase 5 — GUI wizard + settings
- [ ] Phase 6 — clip gallery + trimming
- [ ] Phase 7 — post-save hooks, packaging (AUR/AppImage/Flatpak/winget)

## How it's built

- **Rust.** A background process that outlives your session needs to not fall over, and single-binary distribution matters when you're targeting AUR + winget.
- **Daemon + thin client** — `scd` owns capture and the buffer and keeps running even if the tray icon or hotkey layer dies. `sc` is the CLI that talks to it over a local socket. There's no "GUI required" path; the CLI alone can drive everything.
- **FFmpeg/libav** for encode and mux, hitting hardware encoders (NVENC / VA-API / QSV / AMF) directly instead of shipping a software x264 fallback by default.
- **Linux capture** goes through `xdg-desktop-portal` + PipeWire — the only sane route on Wayland, with restore tokens so you're not re-approving screen access every boot.
- **Windows capture** uses Windows Graphics Capture, falling back to DXGI Desktop Duplication. Windows 11 only — dropping Win10 support keeps the capture path simple.

No process injection, no exclusive-fullscreen hooking, no game-specific patches. If a game runs exclusive-fullscreen and capture goes dark, SimpleClip tells you to switch to borderless instead of trying to out-hack an anti-cheat.

## Platform support

| | Windows | Linux |
|---|---|---|
| Minimum | Windows 11 | Any distro with PipeWire |
| Priority targets | — | Hyprland, niri |
| Also works | — | KDE/KWin, GNOME/Mutter, X11 |
| Audio | WASAPI loopback + mic | PipeWire (no PulseAudio-only path) |

macOS is not implemented. The capture layer is written behind a trait so it could be added later, but nobody's building it right now.

## Not doing (on purpose)

- Cloud upload, accounts, auto-highlight detection — clip locally, do whatever you want with the file after
- In-game overlay — you get a tray notification and an optional sound, that's it
- HDR, multi-monitor simultaneous capture, after-the-press capture windows — all real features, all cut from v1 to ship something solid first

## License

MIT OR Apache-2.0. Anything GPL-licensed (looking at you, x264) stays behind an opt-in build feature so the default build stays permissive.

## Contributing

Not really set up for outside contributions yet — no CI, no issue templates, nothing. Once Phase 0 lands there'll be an actual `CONTRIBUTING.md`. Until then, open an issue if you want to talk about it.
