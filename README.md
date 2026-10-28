<p align="center"><img src="assets/icon-256.png" alt="SimpleClip" width="112"></p>

# SimpleClip for Windows

SimpleClip keeps the last 30 seconds (or 15 s, 45 s, 1 min, 2 min, 5 min) of your screen in memory. When
something good happens you press a hotkey and those seconds are written to an MP4. There is no
record button and nothing to start before you play.

I started this as a Linux tool and then wanted the same thing on Windows, so this repo is
the Windows version: a background service, a small app for browsing and trimming clips, and an
installer. I'm not trying to replace ShadowPlay or Medal. I wanted something small that does one
job and does not need an account.


## What it does

- **Always-on replay buffer.** Windows Graphics Capture for video, WASAPI for sound, encoded on the
  GPU the whole time and kept in RAM.
- **Hotkeys.** `Alt+F10` saves a clip, `Alt+F9` saves a screenshot, `Alt+F8` brings the SimpleClip window up (press it again to minimise it). All three can be changed in Settings, and any key can be used, the backtick included.
- **Hardware encoding.** It tries NVENC (NVIDIA), AMF (AMD), QuickSync (Intel), then software x264,
  then Media Foundation, and uses the first one that works.
- **Audio.** Game and app sound, plus an optional microphone. The mic can be mixed into one track
  or kept on its own track so you can edit your voice separately.
- **Save card.** A small card slides in when a clip is saved, with its size, resolution, frame rate
  and encoder, or a minimal one-line version if you prefer. You choose the corner (or the center),
  whether it slides, and the sound (a camera shutter by default, or your own WAV file, or none).
- **Discord status.** While SimpleClip is recording and you are in a game, your Discord profile shows
  the game and how long you have been playing and clipping. Only games: a browser, the desktop or any
  other program shows nothing. A game is recognised by its store folder (Steam, Epic, GOG, Riot,
  Ubisoft, Xbox, EA, Rockstar), by being a well-known title, or by running in exclusive full screen.
  It goes through the Discord app on your PC, not over the network from SimpleClip, and it is on by
  default. Turn it off in Settings > System.
- **Clip gallery.** Previews, search, and grouping by game, date, folder or bookmark.
- **Rename, folders, bookmarks, notes.** All inside the app. Bookmarks are labels you create and
  colour yourself. Nothing is ever bookmarked automatically.
- **Trim with a live player.** The player is built into the app. It plays and loops only the part
  you selected, and follows the handles while you drag them, so you see the cut as you make it.
  Then either keep the original and make a trimmed copy, or replace the original (it goes to the Recycle
  Bin, not deleted for good). Trimming copies the stream without re-encoding, so it is instant and
  lossless, but cuts land on keyframes.
- **Trimmed clips stay separate.** They get their own view and a small badge, and they are left out
  of the normal clip list and the Recent row on Home. You can also send them to a folder of your
  choice.
- **Share a clip.** "Copy file" puts the clip on the clipboard, so pasting into Discord or a
  chat attaches it. "Compress for Discord" makes a smaller copy that fits 10, 25 or 50 MB, scaling
  the picture down only as far as it must. The original stays. Right after you save a trim, a
  prompt offers both, and once a clip is compressed it offers to copy that file. Compressed copies
  get their own Compressed section next to Trimmed, so they never clutter your clips.
- **Presets.** Three built in (Balanced, Light, Sharp) and your own. A preset holds the frame rate,
  quality, sound and replay length, and nothing about your displays, devices, folders or theme.
- **Themes.** Ten colour themes plus the default, which follows Windows light and dark. You pick one
  in the setup guide and can change it, and add an accent colour, in Settings.
- **Frame rates** of 30, 60, 120, 144, 165 and 240.
- **Settings in tabs.** Capture, Sound, Quality, Storage, Hotkeys, Save card, Look and System, with a search box that finds any setting and a badge that shows when your changes are saved.
- **Screen picker.** Choose which display to record from live pictures of your screens, in Settings and in the setup guide.
- **Last opened ring.** The clip you opened last gets a soft ring, so you do not lose your place in a long list.
- **Tray icon, start with Windows, `sc` on your PATH.** All optional at install time.
- **Updates from inside the app.** SimpleClip checks GitHub for a newer release. When there is one,
  an "Update available" button appears at the top right, shows the changelog, and updates in place
  when you click Update now: it downloads a small package, checks it, and swaps only the changed
  files, with no reinstall. You can turn the check off in Settings. See
  [CHANGELOG.md](CHANGELOG.md) for what changed in each version.

## Screenshots

<details>
<summary><b>Show screenshots</b></summary>

![Home screen](docs/screenshots/home-dark.png)

These screenshots come from the app's own test mode, using a throwaway library of sample footage
that I renamed to look like different games. The footage is from one game, the names are made up.

### Clips

Grouped by game (the default):

![Clips grouped by game](docs/screenshots/clips-dark.png)

Grouped by bookmark. The bookmark chip is small on purpose:

![Clips grouped by bookmark](docs/screenshots/clips-bookmarks-dark.png)

Grouped by folder:

![Clips grouped by folder](docs/screenshots/clips-folders-dark.png)

Trimmed clips only:

![Trimmed clips view](docs/screenshots/clips-trimmed-dark.png)

### Sharing

After saving a trim, SimpleClip asks what you want to do with it. You can turn the prompt off in
Settings.

![Share prompt after a trim](docs/screenshots/share-prompt.png)

Compressed copies are listed apart from your clips, next to the Trimmed ones:

![Compressed clips](docs/screenshots/clips-compressed.png)

### Themes

The default follows Windows. Pick another in the setup guide or in Settings:

![Choose a look in the setup guide](docs/screenshots/setup-look.png)

![Appearance settings](docs/screenshots/settings-appearance.png)

![Ember theme](docs/screenshots/home-ember.png)

![Sand theme](docs/screenshots/clips-sand.png)

### Clip details

Rename, move to a folder, add bookmarks, write a note:

![Clip details](docs/screenshots/details-actions.png)

The buttons on the left include Copy file and Compress for Discord.

Create and recolour bookmarks:

![Bookmark manager](docs/screenshots/bookmarks-dark.png)

### Trim

![Trim screen](docs/screenshots/trim-dark.png)

### Settings

![Settings](docs/screenshots/settings-dark.png)

### Save card

The detailed card (the default), and the minimal one:

![Save card](docs/screenshots/save-popup.png)

![Minimal save card](docs/screenshots/save-popup-minimal.png)

Text on the detailed card from a real save:

```
Clip saved
sc-rainbowsix-20261019-182103.mp4
rainbowsix • 12s • 13.9 MB
1920x1080 @ 60 fps • AMD AMF • 9.8 Mbps
Saved in 0.1s • \rainbowsix
```

### First run

A short setup guide appears the first time you open the app.

![Setup guide](docs/screenshots/wizard1-dark.png)

There is also a light theme. It follows your Windows setting.

![Home, light theme](docs/screenshots/home-light.png)


</details>

## Install

Download `SimpleClip-<version>-Setup.exe` from the Releases page and run it. It installs for your
user only, so it does not ask for administrator rights. The installer offers two optional tasks:

- start SimpleClip when you sign in to Windows
- add the `sc` command to your PATH

Uninstalling is done from Windows Settings, Apps. Your clips are never touched by the uninstaller.

There is also a plain zip if you would rather not install anything: unpack it and run `scd.exe`
(the background service), then `sc-gui.exe` (the app).

Clips go to `Videos\SimpleClip`, in a folder per game and then per day by default. Settings are in
`%APPDATA%\SimpleClip\config\config.toml`, the clip library (bookmarks, notes) is in
`%APPDATA%\SimpleClip\data\library.json`, and logs are in `%LOCALAPPDATA%\SimpleClip\data\logs`.

## The command line

The `sc` command talks to the running service. Example output:

```
> sc status
state:     Active
recording: false
buffer:    30s (98% full)
monitor:   captured display (1920x1080)
encoder:   Amf
drift:     0.0 ms
daemon:    v4.0.0

> sc save
saved C:\Users\you\Videos\SimpleClip\rainbowsix\sc-rainbowsix-20261019-182103.mp4 (11.9s)

> sc screenshot
saved C:\Users\you\Videos\SimpleClip\rainbowsix\sc-rainbowsix-20261019.png (0.0s)

> sc trim <clip> --start 2 --end 6
saved ...\sc-rainbowsix-20261019-182103-trim.mp4 (4.0s)

> sc compress <clip> --mb 4
saved ...\sample-4mb.mp4 (15.9s)
```

A 15.5 MB clip compressed with `--mb 4` came out at 3.3 MB, scaled from 1080p to 540p, with its
sound and length unchanged.

Other commands: `pause`, `resume`, `quit`, `monitors`, `audio-devices`, `thumbnail`, `test-notification`.

## Default config

This is what a fresh install writes (printed by the code itself, not typed by hand):

```toml
[capture]
monitor-id = ""
show-cursor = true
target-fps = 60

[audio]
desktop-enabled = true
desktop-device = ""
mic-enabled = true
tracks = "mixed"
desktop-volume = 100          # percent, 0 to 200
mic-volume = 100

[buffer]
replay-duration-secs = 30
ram-cap-mb = 2048

[encode]
codec = "h264"
bitrate-kbps = 8000
gop-frames = 120
gpu-convert = false

[save]
filename-template = "sc-{app}-{date}"
folder-policy = "per-day"
container = "mp4"
warn-at-gb = 50

[hotkeys]
save = "ALT+F10"
screenshot = "ALT+F9"
open = "ALT+F8"                # brings the SimpleClip window up; empty turns it off

[general]
autostart = true
setup-complete = false
# Notifications (these are the defaults)
popup-style = "detailed"      # or "minimal"
popup-position = "top-right"  # top-left, bottom-right, bottom-left, center
popup-animation = true
theme = "system"           # or midnight, graphite, ember, forest, violet, rose, ocean, daylight, sand, mint
# accent = "#8b5cf6"           # optional accent colour over any theme
save-sound = true
discord-presence = true    # show the game and clipping time on your Discord profile
# save-sound-file = 'C:\path\to\your.wav'
```

## Typical numbers

At the defaults (1920x1080, 60 fps, 8 Mbps H.264) a 30 second buffer fills in 30 seconds, a save takes about a tenth of a second, and a clip is about 60 MB per minute of video. I have not benchmarked CPU or GPU overhead, so I won't put a number on it.

## Things that are not finished

Being straight about this so you don't hit it by surprise:

- **Only H.264 in MP4.** The config has `container` and `codec` fields, but MKV, HEVC and AV1 are
  not implemented yet.
- **`warn-at-gb` and `post_save_hook` are in the config but do nothing.**
- **`ram-cap-mb` is only used to estimate and validate the buffer size**, it is not a hard limit.
- **Trim cuts on keyframes.** That is the price of not re-encoding. With the default 2 second
  keyframe spacing at 60 fps, cuts can land up to about 2 seconds off.
- **Exclusive-fullscreen games** can capture as black with Windows Graphics Capture. Borderless
  or windowed fullscreen works. This is a Windows limitation, not something I can fix here.
- **Only the AMD (AMF) encoder path has been tested.** The NVENC and QuickSync paths are written
  but have not been run on real hardware.
- **No code signing**, so Windows SmartScreen may warn you the first time you run the installer.

## Build from source

You need Rust (stable), the Visual Studio Build Tools with the C++ workload, LLVM (for bindgen),
and a shared FFmpeg 8.1 development build (`bin`, `lib`, `include`). The build script can fetch
what is missing:

```powershell
git clone https://github.com/whoevenisxeno/simpleclip-windows
cd simpleclip-windows
.\packaging\windows\build.ps1 -InstallDeps
```

That produces `dist\SimpleClip` (portable folder), a zip, and `Setup.exe` if Inno Setup is
installed. The default build links an LGPL FFmpeg. If you already have FFmpeg, set `FFMPEG_DIR` to
it. More detail is in [packaging/windows](packaging/windows/README.md).

Checks:

```powershell
cargo clippy --workspace --all-targets
cargo test --workspace
```

## How it is put together

```
sc-core     config, the IPC messages, the ring buffer
sc-capture  Windows Graphics Capture video and WASAPI audio
sc-ffmpeg   encoders, muxing, trim, thumbnails (FFmpeg through ffmpeg-next)
scd         the background service: pipeline, tray, hotkeys, popup, IPC server
sc          the command line client
sc-gui      the app (egui)
```

The service and the clients talk over a named pipe, `simpleclip.sock`, using one JSON message per
line. The app asks the service for previews and trims, because the service is the one that owns
FFmpeg. Design notes and the reasoning behind choices are in [docs/DECISIONS.md](docs/DECISIONS.md).

## Origin and licence

This began as a fork of my own Linux-first SimpleClip. The Linux code paths are still in the tree
but I have not tested them as part of this work. Licensed under the MIT licence, see
[LICENSE](LICENSE). FFmpeg is a separate project under its own licence, and the installer ships
its licence text next to the binaries.
