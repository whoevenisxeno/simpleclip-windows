# Windows packaging

## Install

Run **`SimpleClip-<version>-Setup.exe`**. It installs for the current user (no admin
prompt) to `%LOCALAPPDATA%\Programs\SimpleClip`, adds a Start Menu entry and an
Apps & features entry, and offers to start the daemon at sign-in, put `sc` on your
PATH and add a desktop shortcut. Default save hotkey is
**Alt+F10** (screenshot: **Alt+F9**); change both in Settings. Clips land in
`Videos\SimpleClip`.

Uninstall from Settings > Apps (your clips and settings are kept). Silent install
for scripts: `Setup.exe /VERYSILENT /TASKS="autostart,addpath"`.

The portable zip (`SimpleClip-<version>-win64.zip`) has the same files plus
`install.ps1` / `uninstall.ps1`, a no-installer alternative that does the same job
from PowerShell (`powershell -ExecutionPolicy Bypass -File .\install.ps1`).

## Build from source

```
git clone https://github.com/whoevenisxeno/simpleclip
cd simpleclip
.\packaging\windows\build.ps1 -InstallDeps
```

`-InstallDeps` installs whatever is missing (Rust, Visual Studio Build Tools with
the C++ workload, LLVM) through winget and downloads a shared FFmpeg dev build.
Without it the script tells you what is missing. The result is `dist\SimpleClip\`,
`dist\SimpleClip-<version>-win64.zip` and (with Inno Setup, which `-InstallDeps`
also installs) `dist\SimpleClip-<version>-Setup.exe`. `-NoInstaller` skips the last.

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
  `sc trim`, `sc monitors`, `sc audio-devices`, `sc quit`.
- `sc-gui.exe` is the window: dashboard, gallery, settings. It starts the daemon
  if it isn't running.

Settings: `%APPDATA%\SimpleClip\config\config.toml` (hot-reloaded; capture, audio,
buffer and encoder changes restart the capture pipeline automatically). Setting
`SC_ENCODER=h264_mf` (or another encoder name) in the daemon's environment pins the
video encoder.

## Microphone

Turn on "Capture microphone" in Settings and pick a device. "Mixed" sums it into the
desktop track; "Separate" saves desktop and microphone as two audio tracks (editors
such as DaVinci Resolve and Premiere show both; many players only play the first).
If the mic records silence, check Windows Settings > Privacy & security > Microphone
and allow desktop apps.

## Trim

Gallery > Trim, or `sc trim clip.mp4 --start 5 --end 20`. The cut is lossless (no
re-encode), so it starts at the keyframe at or before the start you pick; with the
default 2-second keyframe interval that can be up to 2 s early.

## Save notification

When a clip or screenshot is saved, a small card drops down in the top-right corner
(it never takes focus, so it won't interrupt a game) showing the file name, game,
length, size, resolution, fps, encoder, average bitrate and how long the save took.
A failed save shows a red card with the reason. Turn it off with "Show a notification
on save" in Settings.

## Gallery

Clips show a preview frame and are grouped by game (or by date), with a search box and
a chip per game to filter. The game is read from the file name / per-app folder.
Deleting sends a clip to the Recycle Bin.

## Lower CPU use

Settings > Quality > "Convert colours on the GPU" hands raw frames to the GPU encoder
(AMF, NVENC, QuickSync) instead of converting them on the CPU first. On an RX 6650 XT
at 1080p60 it dropped the daemon from about 6.5% to 5% of total CPU. It falls back to
the normal path automatically if the encoder doesn't accept it. Off by default.
