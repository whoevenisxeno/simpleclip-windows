# SimpleClip for Windows: handoff

Everything someone needs to pick this project up: what it is, how it fits together, how to build,
test and release it, how updates work, what is unfinished, and the traps that cost time.

## What it is

An instant-replay recorder for Windows. A background service (`scd`) keeps the last 5 s to 5 min of
the screen and sound in RAM, hardware encoded. A hotkey saves the buffer to an MP4. A window
(`sc-gui`) browses, trims, organises, shares and configures clips. A CLI (`sc`) talks to the
service. Current version and history: `CHANGELOG.md`. User-facing description: `README.md`.

Repository: https://github.com/whoevenisxeno/simpleclip-windows (MIT). Branch `main` on GitHub; the
local working branch may be named differently, push with `git push origin <local>:main`.

## Layout

| Crate | Role |
|---|---|
| `sc-core` | Config (`config.rs`), IPC messages (`ipc.rs`), ring buffer, update check (`update.rs`: release parsing, version compare) |
| `sc-capture` | Windows Graphics Capture video, WASAPI audio (`win.rs`) |
| `sc-ffmpeg` | Encoders, muxer, lossless trim, thumbnails, player decoding (`player.rs`), size-limited compress (`compress.rs`) |
| `scd` | The service: pipeline, tray, hotkeys, save card (`popup.rs`), IPC server (`server.rs`), naming |
| `sc` | Command line client |
| `sc-gui` | The app (egui): screens in `screens/`, themes in `themes.rs`, presets, updater (`update.rs`), player (`player.rs`) |

The service and clients talk over a named pipe with one JSON message per line. The service owns
FFmpeg, so the app asks it for previews, trims and compression.

Settings are in `%APPDATA%\SimpleClip\config\config.toml` (plus `presets.json` beside it). The clip
library (bookmarks, notes) is `%APPDATA%\SimpleClip\data\library.json`. Logs and the preview cache
are under `%LOCALAPPDATA%\SimpleClip\data`.

## Build, test, run

Needs Rust, the Visual Studio C++ Build Tools, LLVM (for bindgen) and a shared FFmpeg 8.1 dev build.
Set `FFMPEG_DIR` to the FFmpeg folder (`bin`, `lib`, `include`) and `LIBCLANG_PATH` to LLVM's `bin`.
Only `avcodec`, `avformat`, `avutil`, `swresample` and `swscale` are linked.

```powershell
cargo clippy --workspace --all-targets     # must stay at zero warnings
cargo test --workspace
.\packaging\windows\build.ps1 -FfmpegDir <ffmpeg folder>   # builds dist\ and the installer
```

Running a debug build directly needs FFmpeg's `bin` folder on `PATH`.

Icons come from `assets/icon.svg`: run `assets\make-icon.ps1` (uses Edge headless) to rebuild the
`.ico`, PNGs and tray bitmap.

## Review mode (screenshots without touching real data)

`SC_GUI_SHOTS=<dir>` makes the app walk its screens and write PNGs, using `<dir>\clips` as the clips
folder and `<dir>\library.json`. `SC_GUI_THEME=<id>` renders any theme and `SC_GUI_SHOT_HEIGHT` sets
the window height. The plan is `SHOT_PLAN` in `sc-gui/src/app.rs` (steps that leave a dialog open
come last).

**Trap:** setting these in a shell leaves them set for everything that shell starts later, including
an app you meant to run normally (it came up in review mode). Clear them in a `finally` block.

## Releasing

Rules the maintainer set:
- Version in the workspace `Cargo.toml`. A big update raises the first number, a minor update the
  middle one, a bug fix the last one.
- Every change gets an entry in `CHANGELOG.md`. The release notes are that entry, because the
  in-app updater shows them as the changelog.
- Many small commits with clear messages; a release only per meaningful batch.
- No co-author trailers on commits. README has no em dashes. Nothing machine-specific (hardware
  names, user names, local paths) in the repo or README.

Steps: bump version, changelog entry, `build.ps1`, commit, push, then
`gh release create vX.Y.Z` with these four files from `dist\`: `SimpleClip-X.Y.Z-Setup.exe`,
`-win64.zip`, `-app.zip` and `-dlls.sha256`. Updates need `-app.zip` or `-win64.zip`.

## How updates work

The app checks GitHub's latest release at start and every 6 hours (switch in Settings). "Update
available" opens the changelog. Update now never runs the installer:
1. It reads `-dlls.sha256`. If the installed FFmpeg libraries match, it downloads the small
   `-app.zip` (about 5 MB); otherwise the full zip.
2. It checks the SHA-256 GitHub publishes for the asset, and only accepts downloads from this repo.
3. It unpacks to a staging folder, closes, and a small script stops the service, copies only the
   changed files over the old ones, updates the version in Apps and features, and restarts both.
4. A copy 5 or more major versions behind must update (`FORCE_AFTER_MAJORS`).

Test switches (never set by users): `SC_UPDATE_AUTO=1` installs an available update without a click;
`SC_UPDATE_PRETEND_VERSION=x.y.z` compares against that version instead of the real one. To test
end to end, build a copy with a lower version, put it in its own folder with the FFmpeg DLLs, and let
it update from the latest release (a portable folder works; it is updated in place).

Facts learned: Windows scans the freshly copied programs the first time they run, so the restart
takes about 15 s. `sc quit` hangs because the service closes the pipe without replying, so the
update script kills the service instead. In `cmd`, `if exist ... a & b` skips `b` when false, so the
script is a file with one command per line.

## Things that bite

- Only one app window and one service can run. The window holds an exclusive lock file; a second
  launch exits quietly with code 0.
- `windows`-only code is gated with `#[cfg(windows)]`. The Linux code paths are unmaintained.
- Frame buffers are pooled (`FramePool`); keep the pool small, a bigger one raises memory.
- Memory is mostly FFmpeg, the video driver and capture, not our buffers. Replay length barely
  changes it.
- A damaged `config.toml` is moved aside as `config.toml.bad` and defaults are used.

## State and gaps

Done recently: themes (10 plus default, chosen in the setup guide), presets, Compress for Discord,
Copy file, share prompt after a trim, Compressed section, 144/165/240 fps, in-place updates, new
logo, start-up card, star reminder, uninstall dialog.

Config fields that exist but do nothing: `save.container` (MKV), `save.warn-at-gb`, `post_save_hook`,
codec choice (only H.264). `sc record` and `sc stop` only flip a flag. Only the AMD AMF encoder has
been tested on real hardware (NVENC and QuickSync are written, not run). The installer is not signed,
so SmartScreen may warn.

Ideas not built yet (from comparing with FTHR Clips; their code is GPL, ours is MIT, so use ideas
only): per-app audio (the `wasapi` crate has application loopback), HEVC and AV1, manual recording
with crash-safe fragmented MP4, atomic saves (write to a temp name, then rename), window capture,
export with re-encode (resolution, crop, speed), importing other apps' clip folders, theme
import/export, watermark, a first-run note that the only network use is the update check, a
`KNOWN_ISSUES.md`.
