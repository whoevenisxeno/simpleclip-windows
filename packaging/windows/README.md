# Windows packaging

## Install (from a release zip)

Unzip, then in PowerShell:

```
.\install.ps1
```

This installs for the current user only (no admin prompt) to
`%LOCALAPPDATA%\Programs\SimpleClip`, adds a Start Menu shortcut, puts `sc` on your
PATH, starts the daemon at sign-in and launches it. Default save hotkey is
**Alt+F10** (screenshot: **Alt+F9**); change both in Settings. Clips land in
`Videos\SimpleClip`.

`uninstall.ps1` removes everything but your clips (`-RemoveData` also removes
settings and logs).

If PowerShell refuses to run the script, use
`powershell -ExecutionPolicy Bypass -File .\install.ps1`.

## Build from source

```
git clone https://github.com/whoevenisxeno/simpleclip
cd simpleclip
.\packaging\windows\build.ps1 -InstallDeps
```

`-InstallDeps` installs whatever is missing (Rust, Visual Studio Build Tools with
the C++ workload, LLVM) through winget and downloads a shared FFmpeg dev build.
Without it the script tells you what is missing. The result is
`dist\SimpleClip\` and `dist\SimpleClip-<version>-win64.zip`.

To build by hand instead:

| Need | Why |
|------|-----|
| Rust (MSVC toolchain) | the compiler |
| VS Build Tools, "Desktop development with C++" | the MSVC linker |
| LLVM, with `LIBCLANG_PATH` pointing at its `bin` | `ffmpeg-sys-next` runs bindgen |
| a **shared** FFmpeg 8.1 dev build, with `FFMPEG_DIR` pointing at it (`bin`, `lib`, `include`) | encode and mux |

Then `cargo build --release --workspace`. To run from `target\release`, copy
FFmpeg's `bin\*.dll` next to the executables (or put that folder on `PATH`).

### FFmpeg flavour

`-Variant lgpl` (the default) uses an LGPL FFmpeg build: GPU encoders
(NVENC, AMF, QuickSync) plus Windows' built-in `h264_mf` as a software fallback.
`-Variant gpl` also gets you `libx264`, a better software fallback, but the bundled
DLLs are then GPL. See `docs/DECISIONS.md` (D-0002, D-0010).

## What runs where

- `scd.exe` is the daemon. It is a GUI-subsystem app (no console window), lives in
  the system tray, and logs to `%LOCALAPPDATA%\SimpleClip\data\logs\scd.log`.
  `scd --foreground -v` prints logs to the terminal you start it from.
- `sc.exe` is the CLI: `sc save`, `sc status`, `sc screenshot`, `sc pause`,
  `sc monitors`, `sc audio-devices`, `sc quit`.
- `sc-gui.exe` is the window: dashboard, gallery, settings. It starts the daemon
  if it isn't running.

Settings: `%APPDATA%\SimpleClip\config.toml` (hot-reloaded). Setting
`SC_ENCODER=h264_mf` (or another encoder name) in the daemon's environment pins the
video encoder.
