# Changelog

Versions: a big update raises the first number, a minor update the middle one, and a bug fix the last one.

## 6.9.0

### Added
- Home has a stats section at the bottom: clips saved, screenshots, space used, your day streak and best streak, a chart of the last 14 days, your top games, clips this week, your busiest day, the time of day you usually clip, and how many clips you trimmed or compressed. Its heading peeks in at the bottom of the window, so scroll down to see it. It is all counted from the files in your clips folder.

### Fixed
- In Settings > Storage a long clips folder path was drawn over the "Clips folder" label. The path now stops before the label.
- In a short window, the Remove duplicates dialog lost its title and its buttons off the edges, and the Settings tab list cut off System and Setup guide. The dialog's list now shrinks to fit and the tab list scrolls.
- In the light theme the dots for the setup guide steps still to come were nearly invisible. They now stand out.
- In the setup guide, the screen picker made the "What should it record?" step taller than the window and pushed Continue off the bottom edge. The step now scrolls and Back and Continue stay at the bottom.
- On the Add-ons page the Game events description ran underneath the Remove button and the switch. Descriptions in every Settings row now stop before the controls (and show in full when you hover them), so long ones can no longer overlap.

### Changed
- Game events in Rainbow Six Siege no longer counts team kills as kills. It tells them apart by the colour of the victim's name tag in the kill feed (teammates share your purple, enemies are olive), and when the colour is not clear, for example while a line fades in, it waits instead of guessing. In test mode a team kill shows its own notification, "Team kill in Rainbow Six Siege. Not counted as a kill, so no clip would be saved."
- The Add-ons page is tidier: the Game events switch is on its own, the games sit under Games, and Save a clip at, Clip length and Test mode sit under Saving. Those two groups grey out while Game events is off. A game that is not installed now reads "Not found in your Steam folders".

## 6.8.5

### Added
- Game events now works in Rainbow Six Siege and Dota 2, as well as Counter-Strike 2 and League of Legends, each with its own switch.
- Rainbow Six Siege has no official feed, so the add-on reads the kill feed in the top right of the screen with Windows' built-in text recognition and counts the lines where you are the killer (the game labels your own name YOU). It only looks at that small area, only while the game is running, and keeps nothing. It needs an English language pack for Windows text recognition, which most PCs have. Two kills close together count as a double kill and so on, using the same Save a clip at and Clip length choices.
- Dota 2 uses the game's own state feed like Counter-Strike 2, with one small file added to its cfg folder. Kills within 18 seconds of each other count as a streak.
- Test mode, a switch on the Add-ons page. With it on, game events shows a notification ("Double kill in Rainbow Six Siege. A clip would have been saved now.") instead of saving a clip, so you can see what it picks up in a real match without filling your folder. It follows the same games, streak and timing rules as saving.

### Changed
- Add-ons now have their own page in Settings, above System, instead of a card inside System. Game events is the first one on it.

## 6.8.0

### Added
- Game events, an optional experimental add-on. It is not part of SimpleClip until you turn it on in Settings > Add-ons, which asks first, then downloads it from the SimpleClip releases on GitHub, checks its checksum and installs it. Once on, it saves a clip by itself when you get a multi-kill in Counter-Strike 2 or League of Legends. You choose which games, the smallest streak that counts (any kill, double, triple, quadra or penta) and how much to save (10 seconds to 1 minute). A streak becomes one clip, saved a few seconds after the last kill so the moment has played out. Counter-Strike 2 uses the game's own state feed (SimpleClip adds one small file to the game's cfg folder, and you restart the game once), League of Legends uses its local live client API, and both only talk to your own computer. The add-on runs as its own small program, started and stopped by SimpleClip, and Remove deletes it again, including the file it added to the game.
- `[events]` in the config file (`enabled`, `cs2`, `league`, `min-kills`, `save-secs`).

## 6.7.3

### Added
- Your own shortcuts. In Settings > Hotkeys, under More shortcuts, press Add a shortcut to bind any key to an action you choose: save the last 10, 15, 30 seconds, 1, 2 or 5 minutes, take a screenshot, pause or resume recording, open SimpleClip, or open the clips folder. Add up to 12, for example one key for a quick 15 second clip and another for the last 2 minutes. A shortcut that repeats another one is marked and left off, and a length longer than your replay length says that it will save at most that much.

## 6.7.2

### Added
- A small refresh button next to the version in the bottom left corner checks for updates right away. It pulses while it looks, then opens the update card if there is one or tells you that you are on the latest version. The Check now button in Settings does the same.

### Changed
- While an update is waiting, the popup card now shows at every check (every 6 minutes) instead of once per version, so it cannot be missed. The chime still plays once per version, so it does not repeat. Turning off notifications in Settings still turns the card off.

## 6.7.1

### Changed
- SimpleClip now looks for a new version every 6 minutes while it runs, instead of every 30.

### Added
- A 20 MB size in Compress for Discord, in the dialog and as Fit 20 MB in the right-click menu.

## 6.7.0

### Fixed
- Sound could play later than the picture in new clips, by a different amount in each recording session. SimpleClip timed the picture from its first frame and the sound from the moment capture started, so the two clocks disagreed by however long Windows took to deliver that first frame (a fraction of a second, sometimes a few seconds). Both now use the same clock, and a test that rebuilds a clip from a start delay of up to 2.3 seconds measures picture and sound landing within a few hundredths of a second of each other.
- Over a long session the sound could slowly slip when the sound device's clock ran a little faster than Windows' clock, because nothing pulled it back. The extra is now trimmed off, with a short fade so it does not click, the same way gaps were already filled with silence.
- Two groups of choices in the same window that shared an option name could steal each other's clicks, so clicking the first one did nothing. Each group now keeps its own.
- Sound is now placed on the same clock as the picture when you save an edit, instead of up to one frame late.

### Added
- Settings > Sound > Sound timing moves the sound of new clips earlier or later, up to a second either way, in case your headset, capture card or setup adds a delay of its own.
- Edit > Sound > Sound timing does the same for a clip you already have, up to three seconds either way, with a live preview and Earlier and Later buttons that move it 10 ms at a time. This is how to repair clips that were saved out of sync.
- A bigger Compress for Discord dialog, the same one from the clip details, the right-click menu and the prompt after saving. Sizes of 8, 10, 25, 50, 100 and 500 MB, or any size from 1 to 500 MB. You can cap the resolution (1080p, 720p, 480p, 360p), cap the frame rate (60, 30, 24) and leave the sound out, and a line under the choices says what the result will be, such as "Will be saved at 1280 x 720, 30 fps, with sound, up to 10 MB", or that the clip is already small enough. The right-click menu has quick Fit 8, 10, 25, 50 and 100 MB entries and More options.
- `sc compress` takes `--height`, `--fps` and `--mute` as well as `--mb`.

### Changed
- Compressing never gives the video more bits than the original had, so a clip that is already small does not grow when you pick a big size with a cap.

## 6.6.0

### Changed
- Speed is now set per piece instead of for the whole clip. Split the clip, pick a piece, and give just that part its own speed, for example slow motion on the highlight and normal speed around it. Each piece shows its speed on the timeline, the line under the timeline says which piece is selected, and Apply to all gives every piece the same speed.
- The preview now plays at the real speed of each piece, with the sound speeding up or slowing down with it, and a badge on the picture shows the speed you are watching. In the saved clip the sound keeps its pitch.
- Saving moved into a dialog. The Save button is at the top right next to Undo and Redo (or press Ctrl+S). It shows the length, size and what you changed, lets you pick Save as a copy or Replace the original, and shows progress with a Cancel button. The page no longer scrolls to reach it, and the result stays shown after you save.

### Fixed
- Fades did nothing you could see in the editor. The saved clip was faded correctly, but the preview ignored them, so moving the Fade in and Fade out sliders seemed to have no effect. The preview now darkens the picture and lowers the sound along the fade as you scrub or play, so you can check it before saving.
- The values beside the Volume and Fade sliders ("100%", "2 s") were drawn outside their box, into the edge of the card, and the sliders sat at slightly different positions. They are now aligned.
- Replacing the original could fail to move it to the Recycle Bin when the player had only just let go of the file. It now retries for a moment first.

## 6.5.1

### Fixed
- Undo in the editor only went back one step. Pressing Ctrl+Z again undid the undo, so it flipped between the last two states. It now goes back one edit at a time, as far as you made them, and Redo (Ctrl+Y or Ctrl+Shift+Z) walks forward again. Reset is a single step you can undo too.

## 6.5.0

### Added
- A real clip editor. The Trim screen is now Edit, with a timeline that shows the whole clip as a strip of pictures. Click to move the playhead, press Split here (or S) to cut the clip into pieces, select a piece and drag its edges, and press Delete piece (or Delete) to remove it. Removing a piece from the middle is just a cut. The player skips the parts you removed, and Undo, Redo and Reset (Ctrl+Z, Ctrl+Y) step through your changes.
- Speed from 0.25x to 4x. The sound keeps its pitch instead of going squeaky or growly.
- Sound: volume from 0 to 200%, and a mute switch.
- Fade in and fade out of up to 5 seconds, for the picture and the sound.
- Crop with handles on the picture, free or locked to 16:9, 9:16, 1:1, 4:5 or 21:9, with a thirds grid.
- Turn left or right and flip sideways or upside down, with the preview following along.
- A progress bar with Cancel while an edit is saved. A summary line shows the length, size and what you changed before you save.
- Frame accurate cuts. Edits are re-encoded, so a cut starts exactly where you put it instead of up to about 2 seconds early. For a plain trim there is a Frame accurate switch; turn it off for the old instant lossless cut.

### Changed
- Editing no longer needs the SimpleClip service to be running.
- Open, Trim and the Trimmed tab are now Edit and Edited, and the saved clips are called edited clips.
- The editor preview is a little smaller so the timeline and tools fit on screen.

### Fixed
- Trimming could cut up to about 2 seconds early because it snapped to a keyframe. Edits are now exact unless you switch Frame accurate off.

## 6.4.0

### Added
- SimpleClip now looks for a new version every 30 minutes while it runs (it used to look when it started, and the window every 6 hours). When it finds one it pops up a themed card, "SimpleClip X is available", and plays a short two-note chime that is different from the clip saved sound. You are told once per version, not every half hour. The check at start-up plays the chime too.
- Settings > System > Advanced > Sound when an update is found turns the chime off, with a Preview button to hear it. The card shows only if popups are on, and none of it happens if Check for updates is off.

## 6.3.0

### Changed
- The small cards that pop up when a clip is saved, and when SimpleClip starts (with the news that an update is available), are redrawn. They are now fully solid with smooth rounded corners and a soft shadow instead of slightly see-through with jagged corners, and they follow your SimpleClip theme and accent colour (or Windows light and dark with the default theme). The update card now says "SimpleClip X is available".
- Removing duplicate clips now opens a proper confirmation. It shows how many clips go, how much space it frees and how many stay, then lists each clip with its picture, kind and size, marked Keeps or Removes, before anything is moved to the Recycle Bin.

## 6.2.0

### Added
- Settings > Storage > Clean up > Remove duplicate clips. It looks for clips that exist in several versions, such as an original, a trimmed copy and a compressed copy of it, and keeps the best one: the compressed copy if there is one, otherwise the trimmed copy, otherwise the original. It asks first, tells you how many clips and how much space, and moves the rest to the Recycle Bin. Two different trims of the same clip are different moments, so both are kept, and clips that only share a name are never touched.

### Fixed
- The ring around the clip you just opened did not show up until you came back to the window and moved the mouse, because opening a clip hands the screen to your video player. It now draws straight away, so it is already there when you return.

## 6.1.0

### Added
- A soft ring around the clip you opened last, so you can find it again in a long list. It shows on the Clips page and in Home's Recent clips, after you play a clip, trim it, or open it from its menu or details.
- Icons beside each section in Settings (Capture, Sound, Quality, Storage, Hotkeys, Save card, Look, System and Setup guide).

## 6.0.1

### Added
- Pick the screen to record from live pictures. Settings > Capture and the setup guide now show a picture of each screen as it looks right now, with its name, size and refresh rate, and a "Recording this screen" marker on the one that is chosen. Click a picture to choose that screen; the main display stays the default. The service now also reports where each screen sits, so each picture matches its screen. (The pictures update every few seconds while you look at them, and are never saved.)

### Fixed
- The setup guide, and Settings in a small window, drew each row far too tall, with the switch or buttons floating far below their name. Rows are back to their normal height, and the setup guide is a little wider.

## 6.0.0

A new Settings and a more refined finish across the window. Everything works the same; it is laid out and drawn more carefully.

### Changed
- Settings is now a set of tabs: Capture, Sound, Quality, Storage, Hotkeys, Save card, Look and System. A search box at the top finds a setting in any tab, and an "All changes saved" badge shows when your changes are written. On a narrow window the tabs run along the top and each setting puts its control under its name.
- The sidebar marks the page you are on with a card and an accent bar, shows how many clips you have beside Clips, shows the buffer filling in the status block, and shows the version at the bottom.
- Home: tick marks round the buffer ring, small captions on the four info tiles, and a strip at the bottom with your three shortcuts.
- The Trim screen centres the video above its controls instead of leaving it to the left.
- Rows in Settings have more room, and a row's controls no longer run over its name in a small window.
- Setup guide: the theme tiles fit six to a row.
- The settings that used to be under General now live in System, the shortcuts are under Hotkeys, and the save card options are under Save card.

### Fixed
- The theme tiles in Settings and the setup guide ignored the accent colour you had picked, so changing the accent looked like it did nothing there. They now show your accent.
- Text on an accent-coloured button, chip or the Update available button is now dark when the accent is a light colour, so it stays readable.

## 5.0.2

### Added
- A 45 second replay length, between 30 s and 1 min, in Settings and in the first-run setup.

## 5.0.1

### Changed
- The Discord status now shows only actual games. A browser, the desktop, your editor or any other program shows nothing, and SimpleClip does not even connect to Discord while you are not in a game. A program counts as a game if it sits in a game store's folder (Steam, Epic, GOG, Riot, Ubisoft, Xbox, EA, Rockstar; the folder gives the game's proper name), is a well-known game by its program name, or is running in exclusive full screen (and is not a browser or media player).
- The timer now counts this game session rather than the whole time SimpleClip has been running. Alt-Tabbing out to chat or a browser for up to a minute and a half keeps the status; a different game, or being away longer, starts a fresh timer.
- Known limit: a game installed outside the store folders, not on the built-in list and run in a window or borderless, is not recognised. That is deliberate, so that nothing else is ever shown as a game.

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
