# Changelog

Versions: a big update raises the first number, a minor update the middle one, and a bug fix the last one.

## 5.0.0

A polish of the whole look. The same layout, colours and feel, with the details tightened up.

### Changed
- One set of corner radii everywhere: cards and lists 16, dialogs 22, tiles 14, buttons and fields 10, with inner shapes (like the segmented control's pill) curved to run parallel to the shape around them.
- Controls match native proportions: the switch is 51 by 31 with a soft knob shadow, buttons are 32 high, the segmented control's pill glides to the new choice, and buttons, icon buttons and sidebar items ease in on hover instead of flicking.
- The filled (accent) button is semibold, and there is only one filled button in a view: the inline Compress buttons in the details window and the share prompt are now plain buttons.
- A proper volume slider for system sound and the microphone: a thin accent track, a white knob, a notch at 100% and the value beside it.
- One type scale: 30 for page titles, 20 for headings, 14 for body, 12 for captions; section labels are 12 and the quieter grey text is a little lighter so it reads clearly.
- Proper typography in the words: real apostrophes (You’re covered) and ellipses (Choose…), and shortcuts written the way people say them (Alt+F10).
- Shadows are soft and wide under dialogs and menus, and a hair of shadow lifts white cards in the light theme.
- The empty Clips page has a quiet icon above its title.

## 4.13.1

### Added
- The Discord status now has a "Get SimpleClip" button that opens the project page on GitHub, for people looking at your profile. Discord only shows it to others, not to you.

## 4.13.0

### Added
- Discord status (Rich Presence). While SimpleClip is recording, your Discord profile shows "Playing" the program you are in, "Clipping with SimpleClip", and how long you have been clipping; it says "Replay buffer paused" while paused. It is on by default and can be turned off in Settings > General > Discord status. It talks only to the Discord app on your PC (nothing is sent over the network by SimpleClip) and does nothing if Discord is not running. The name shown is the program in front, the same name the Clips page uses for a game, so it can be a non-game program such as a browser if that is what you have open.

## 4.12.7

### Changed
- The open-window shortcut (`Alt+F8` by default) now works both ways: pressing it while SimpleClip is the window in front minimises it. If the window is open but behind something else, or minimised, it still comes to the front.

## 4.12.6

### Added
- Right-click a clip (its picture or its title) for a menu with everything the three dots and the details window offer: Play, Trim, Copy file, Compress for Discord (10, 25 or 50 MB), Show in folder, rename and notes, bookmarks (tick or untick them, or edit them), Move to folder (or a new one), and Delete. Compressing from the menu runs in the background and reports when the smaller copy is saved.

## 4.12.5

### Added
- Any key can now be a shortcut, on its own or with Ctrl, Alt, Shift and Win: the backtick (`) and the other punctuation keys, arrows, Tab, Enter, Backspace, Caps Lock, the numpad, the media and volume keys, F1 to F24, and keys with no name (saved as `VK_xx`). The recorder in Settings picks them up, and a shortcut with a plain typing key such as ` stops that key typing in other programs while SimpleClip runs. On its own Esc still cancels the recording, but Ctrl+Esc and the like can be recorded. The recorder also waits until every key is let go before it listens, so pressing Enter or Space to click Change does not become the shortcut.

## 4.12.0

### Added
- A shortcut that brings SimpleClip up straight away (`Alt+F8` by default, changeable or switchable off in Settings > Shortcuts). If the window is already open, even minimised, it comes to the front; otherwise it opens.
- Volume sliders in Settings > Audio for system sound and for the microphone, from 0 to 200%. They apply to a running recorder at once, with no restart, and only to sound recorded after the change (what is already in the buffer keeps its level). Double-click a slider to go back to 100%.

### Fixed
- Shortcuts with Alt, such as Alt+F10, could fail where plain ones like F10 worked. The shortcut recorder in Settings now reads the keyboard directly, because the window it listened to loses Alt on keys Windows treats as menu keys. After an Alt shortcut fires, SimpleClip also cancels the lone Alt press that Windows would otherwise turn into a menu activation in the program in front. I could not reproduce the failure on a real keyboard, so this is checked only through the Windows hotkey API and the recorder's logic; the service now logs each hotkey press, so a shortcut that still does nothing can be told apart from one that never arrives.

## 4.11.0

### Added
- After you save a trim, a prompt asks if you want to copy the clip or compress it for Discord. Choosing Compress lets you pick 10, 25 or 50 MB, makes the smaller copy, then offers to copy that file. "Don't ask again" in the prompt, or the switch in Settings, turns it off.
- A Compressed section next to Trimmed in Clips. Compressed copies (made in the app or with `sc compress`) are listed there with a "Compressed" badge, kept out of your main clip list and the Recent row on Home, and grouped under the game they came from.

## 4.10.0

### Added
- Themes: ten colour themes besides the default (Midnight, Graphite, Ember, Forest, Violet, Rose, Ocean, Daylight, Sand, Mint), plus an optional accent colour over any of them. The default is the original look and still follows Windows light and dark. The setup guide has a new "Choose a look" step, and Settings has an Appearance section.
- Compress for Discord: in a clip's details, make a smaller copy that fits 10, 25 or 50 MB. The picture is scaled down only as far as needed, and all sound tracks are mixed into one. The original stays. `sc compress <clip> --mb 10` does the same from the command line.
- Copy file: puts a clip on the clipboard, so you can paste it straight into Discord or a chat.
- Presets: Balanced, Light and Sharp are built in, and you can save your own. A preset holds the frame rate, quality, sound and replay length, and never your displays, devices, folders, shortcuts or theme.
- Frame rates of 144, 165 and 240 fps. The keyframe interval is now shown in seconds and keeps its length when you change the frame rate.

## 4.9.3

### Changed
- Logo: the ring is now white instead of blue, and made of five longer dashes instead of many short ones, so it stays clear at small sizes (tray icon, taskbar, file icon).

## 4.9.2

### Fixed
- After an in-place update, SimpleClip could fail to start again when it was running from a folder without an uninstaller (a portable copy). The update steps now run from a small script, and the restart always happens.
- The leftover update script is cleaned up on the next start.

## 4.9.1

### Fixed
- An update could leave SimpleClip closed for about 20 seconds before it came back. The service is now stopped straight away, so the restart takes a few seconds.
- Updating a copy that is not installed no longer touches the version shown in Apps and features.

## 4.9.0

### Changed
- Updating no longer runs the installer. SimpleClip downloads a small package with just the programs (about 15 MB), checks it, and after the window closes it copies only the changed files over the old ones and starts again. Your settings, shortcuts and the uninstaller are untouched. The big FFmpeg libraries are only downloaded when they actually changed. A log of the copy is written to `update.log` in the logs folder.

## 4.8.0

### New
- New logo: the app, installer, tray icon and sidebar all use the new replay-buffer icon. It is built from `assets/icon.svg` by `assets/make-icon.ps1`.
- After an update, SimpleClip says "SimpleClip updated to ..." once.

### Fixed
- Updating could start the installer more than once, which opened two SimpleClip windows and two command windows. It now starts it once and runs it without a console window.
- Only one SimpleClip window can be open at a time.
- Saving several clips in the same second no longer overwrites earlier ones; each gets its own name.
- A damaged `config.toml` no longer stops the background service from starting. It is moved aside as `config.toml.bad` and the defaults are used.
- Error messages for missing or unreadable video files are now plain, not codes.

### Security and robustness
- Updates are checked against the SHA-256 that GitHub publishes for the installer, and are only downloaded from this project's own releases. A stalled download now gives up instead of waiting for ever.
- Old logs (keeping 7 days), old preview images (30 days, 200 MB) and old downloaded installers are cleaned up automatically.

## 4.7.1

### Fixed
- Clip previews could stay on "Preview unavailable" for good if the background service was briefly unreachable (while it restarted, for example). Previews now retry every 20 seconds and again as soon as the service is back.

### Changed
- The background service reuses its picture buffers instead of allocating new ones 60 times a second, which lowers its CPU use by about 15 percent. (Memory is about the same: most of the service's memory is FFmpeg, the video driver and the screen-capture system, not SimpleClip's own buffers.)

## 4.7.0

### Added
- More quality settings: a Low preset (4 Mbps), a Bitrate box for exact control (1 to 100 Mbps), and a Resolution option (Native, 1440p, 1080p, 720p, 480p) that scales the picture down before encoding for smaller files and less work.

### Changed
- "Lower CPU use" is now on by default, and is switched on once for existing installs (turn it off again in Settings, Quality, if you prefer). It falls back by itself if your encoder does not support it. It is skipped when a lower resolution is chosen, because the GPU path cannot resize.
- Smaller installer and download: FFmpeg's avfilter and avdevice libraries (about 40 MB unpacked) are no longer shipped, because SimpleClip never used them.
- The background service queues fewer pictures between capture and encode.
- The app window uses less memory: previews are smaller and only loaded when scrolled into view, the least recently shown are dropped, everything is released while the window is minimized, and the built-in player decodes at a smaller size.

## 4.6.0

### Added
- An occasional "Enjoying SimpleClip?" card in the app window, asking for a star on GitHub. It appears at most once a week, only after the window has been open for 5 minutes, and never during setup or while an update is on screen. Opening the GitHub page from it (or choosing "Don't ask again") stops it for good. "Maybe later" waits another week.

## 4.5.0

### Added
- The installer now offers "Start with fresh settings" when it finds settings from an earlier install. It backs the old file up as config.toml.bak, so the setup guide runs on first start and every setting begins at its default. It is off by default, so updates keep your settings. For silent installs use `/TASKS=freshsettings`.
- Settings has a Reset section at the bottom: "Reset all settings" puts everything back to the defaults, saves the old settings as config.toml.bak and opens the setup guide. Clips and bookmarks are kept.
- Updating now shows a progress bar with the megabytes downloaded, then "Installing the update" before SimpleClip restarts, so it no longer looks stuck.
- Required updates: a copy that is 5 or more major versions behind (still on 4.x while 9.x is out) shows an "Update required" screen when you open it. It downloads and installs the new version straight away, and the only other option is Quit. This needs "Check for updates" to be on.

## 4.4.1

### Fixed
- The round icon on the cards (the green tick, the red cross and the blue play icon) had jagged edges. It is now drawn smoothly.

## 4.4.0

### Added
- Save card styles: Detailed (the full card, as before) and Minimal (one short line, much smaller). Pick one in Settings, Notifications.
- Choose where the cards appear: top right (default), top left, bottom right, bottom left or center.
- Cards slide in and out, with a switch in Settings to turn the slide off and just fade.
- A new default save sound (a camera shutter). Turn it off or choose your own WAV file in Settings.
- A Preview button in Settings, and `sc test-notification`, that show a sample card and play the sound with your current settings.

### Changed
- Redesigned cards: a round icon badge, a hairline border and tidier text, with the same information.
- The start-up card is blue with a play icon, so it no longer looks like a saved clip.
- Notification settings moved to their own section in Settings.

## 4.3.0

### Changed
- Uninstalling now asks "Are you sure?" first. The dialog has a switch, off by default, to also move your clips to the Recycle Bin, and a reminder to star the project on GitHub if you enjoyed it. After you confirm, the uninstaller runs by itself without asking again.

## 4.2.0

### Added
- An Uninstall button at the bottom of Settings. It asks first, then closes the app and runs the uninstaller. Your clips are kept. It only shows for an installed copy, not a portable folder.
- A card when SimpleClip starts (for example when you sign in to Windows): it says SimpleClip is running in the background, shows the save hotkey, and checks for updates, telling you if a newer version is out or that you are up to date. It follows the "Show a card when a clip is saved" setting, and the update line follows "Check for updates".

## 4.1.0

### Added
- Update check: SimpleClip looks on GitHub for a newer version and shows an "Update available" button at the top right. It opens the changelog and an "Update now" button that downloads the installer and restarts the app. You can turn the automatic check off, or check by hand, in Settings.
- The trim screen now has a live player. It plays and loops exactly the part you selected, and follows the handles while you drag them: the start handle shows the first picture of the selection, the end handle the last. This replaces the separate start and end frame view and the "Preview trimmed clip" button.
- Keyboard controls in the player: Space plays or pauses, the arrow keys skip 5 seconds.

### Fixed
- Dragging the player's seek bar jumped to the wrong time.
- Clips saved with separate audio tracks (game and microphone) only played the first track. All tracks are now mixed in the player.
- Picture and sound could drift apart. The picture now follows the sound with the output delay taken into account, and keeps going if the sound device stalls.
- A clip that cannot be decoded now says so instead of showing a blank picture.

## 4.0.0

First Windows release: replay buffer with hardware encoding, tray icon, hotkeys, save popup, clip gallery with bookmarks, notes and folders, lossless trim, microphone capture and an installer.
