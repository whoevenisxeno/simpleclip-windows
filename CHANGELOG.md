# Changelog

Versions: a big update raises the first number, a minor update the middle one, and a bug fix the last one.

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
