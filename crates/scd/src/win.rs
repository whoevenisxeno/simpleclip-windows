//! Windows shell integration for the daemon: single-instance guard, global save
//! hotkey, system tray, start-with-Windows, toast notifications, and the little
//! Win32 helpers (foreground app, local time) the portable code calls.

use crate::state::Daemon;
use sc_core::capture::CaptureState;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use windows::core::{w, BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM};
use windows::Win32::Media::Audio::{
    PlaySoundW, SND_ASYNC, SND_FILENAME, SND_MEMORY, SND_NODEFAULT,
};
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, SendInput, UnregisterHotKey, HOT_KEY_MODIFIERS, INPUT,
    INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VIRTUAL_KEY, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PeekMessageW, PostQuitMessage,
    SetForegroundWindow, ShowWindow, TranslateMessage, MSG, PM_REMOVE, SW_MINIMIZE, SW_RESTORE,
    WM_HOTKEY,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const AUMID: &str = "SimpleClip"; // also the Run-key value name

/// Route console output to the parent terminal when started with `--foreground`.
/// The release binary is a GUI-subsystem app (no console flash at login), so a
/// terminal has to be attached explicitly. Returns a writer for the log output.
pub fn attach_parent_console() -> Option<std::fs::File> {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS).ok()? };
    std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()
}

/// `false` if another daemon already holds the instance mutex.
pub fn acquire_single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, true, w!("Local\\SimpleClipDaemon")) {
            // The handle is deliberately never closed: the mutex must live as
            // long as the process (HANDLE has no Drop, so nothing releases it).
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

/// Local wall-clock time as (year, month, day, hour, minute, second).
pub fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let t = unsafe { GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

/// Executable path of the window currently in the foreground.
pub fn foreground_exe() -> Option<PathBuf> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok =
            QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(proc);
        ok.ok()?;
        Some(PathBuf::from(String::from_utf16_lossy(
            &buf[..len as usize],
        )))
    }
}

// ---------------------------------------------------------------- hotkey --

/// Parse "CTRL+ALT+F10" style strings into RegisterHotKey modifiers + virtual key.
pub fn parse_hotkey(spec: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut key = None;
    for part in spec.split('+').map(|p| p.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "ALT" => mods |= MOD_ALT,
            "SHIFT" => mods |= MOD_SHIFT,
            "SUPER" | "WIN" | "META" | "CMD" => mods |= MOD_WIN,
            "" => return None,
            k => {
                if key.replace(parse_key(k)?).is_some() {
                    return None; // two non-modifier keys
                }
            }
        }
    }
    key.map(|k| (mods, k))
}

fn parse_key(k: &str) -> Option<u32> {
    sc_core::keys::vk_from_name(k)
}

/// Own the global hotkeys on a dedicated message-loop thread. Bindings follow
/// the config: edit `hotkeys.save` / `hotkeys.screenshot` and they re-register
/// without a restart.
pub fn spawn_hotkey(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-hotkey".into())
        .spawn(move || hotkey_loop(daemon))
        .expect("hotkey thread");
}

const SAVE_ID: i32 = 0x5C01;
const SHOT_ID: i32 = 0x5C02;
const OPEN_ID: i32 = 0x5C03;

/// One configurable global hotkey: what is registered with Windows right now,
/// and the last value that failed (so a bad binding is reported once, not every poll).
struct Binding {
    id: i32,
    what: &'static str,
    registered: Option<String>,
    failed: Option<String>,
    /// The registered combination includes Alt (see `mask_alt`).
    uses_alt: bool,
}

impl Binding {
    fn new(id: i32, what: &'static str) -> Self {
        Self {
            id,
            what,
            registered: None,
            failed: None,
            uses_alt: false,
        }
    }

    /// Bring the registration in line with the configured `want` string.
    fn sync(&mut self, want: &str) {
        if self.registered.as_deref() == Some(want) || self.failed.as_deref() == Some(want) {
            return;
        }
        if self.registered.take().is_some() {
            unsafe {
                let _ = UnregisterHotKey(None, self.id);
            }
        }
        self.uses_alt = false;
        // An empty binding means "off" and is not an error.
        if want.trim().is_empty() {
            self.failed = None;
            return;
        }
        let Some((mods, vk)) = parse_hotkey(want) else {
            tracing::warn!(hotkey = %want, what = self.what, "unrecognized hotkey");
            notify(
                "SimpleClip",
                &format!(
                    "\"{want}\" isn't a valid {} hotkey. Fix it in Settings.",
                    self.what
                ),
            );
            self.failed = Some(want.to_string());
            return;
        };
        match unsafe { RegisterHotKey(None, self.id, mods | MOD_NOREPEAT, vk) } {
            Ok(()) => {
                tracing::info!(hotkey = %want, what = self.what, "hotkey registered");
                self.registered = Some(want.to_string());
                self.failed = None;
                self.uses_alt = mods.0 & MOD_ALT.0 != 0;
            }
            Err(e) => {
                tracing::warn!(hotkey = %want, what = self.what, error = %e, "hotkey in use by another app");
                notify(
                    "SimpleClip",
                    &format!(
                        "Hotkey {want} is already taken by another app. Change it in Settings."
                    ),
                );
                self.failed = Some(want.to_string());
            }
        }
    }
}

fn hotkey_loop(daemon: Daemon) {
    let mut save = Binding::new(SAVE_ID, "save");
    let mut shot = Binding::new(SHOT_ID, "screenshot");
    let mut open = Binding::new(OPEN_ID, "open");
    loop {
        let (want_save, want_shot, want_open) = {
            let cfg = daemon.config.read().unwrap();
            (
                cfg.hotkeys.save.clone(),
                cfg.hotkeys.screenshot.clone(),
                cfg.hotkeys.open.clone(),
            )
        };
        save.sync(&want_save);
        shot.sync(&want_shot);
        open.sync(&want_open);
        // WM_HOTKEY lands in this thread's queue; poll it so config changes are
        // also noticed promptly.
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message != WM_HOTKEY {
                continue;
            }
            let id = msg.wParam.0 as i32;
            if let Some(b) = [&save, &shot, &open].into_iter().find(|b| b.id == id) {
                tracing::info!(what = b.what, "hotkey pressed");
                if b.uses_alt {
                    mask_alt();
                }
            }
            let d = daemon.clone();
            match id {
                SAVE_ID => {
                    std::thread::spawn(move || save_from_hotkey(&d));
                }
                SHOT_ID => {
                    std::thread::spawn(move || screenshot_from_hotkey(&d));
                }
                OPEN_ID => {
                    std::thread::spawn(open_gui);
                }
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

/// Windows treats a lone press and release of Alt as "go to the menu bar". A
/// hotkey like Alt+F10 swallows the F10 but not the Alt, so the program in front
/// would still see a lone Alt: its menu lights up, a game can lose focus, a
/// browser eats the next key. Pressing an unassigned key while Alt is still down
/// turns it into a combination, which is what stops that.
fn mask_alt() {
    unsafe {
        if GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000 == 0 {
            return; // Alt is already up, nothing to cancel
        }
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0xE8), // unassigned
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn save_from_hotkey(daemon: &Daemon) {
    if let Err(e) = daemon.save_clip(None) {
        tracing::warn!(error = %e, "hotkey save failed");
        notify("SimpleClip", &format!("Couldn't save clip: {e}"));
    }
}

fn screenshot_from_hotkey(daemon: &Daemon) {
    if let Err(e) = daemon.screenshot() {
        tracing::warn!(error = %e, "hotkey screenshot failed");
        notify("SimpleClip", &format!("Couldn't take screenshot: {e}"));
    }
}

// --------------------------------------------------------- notifications --

/// A card shortly after start-up: SimpleClip is running in the background, and
/// whether a newer version is out. Waits a little for the network at sign-in.
pub fn announce_startup(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-startup-card".into())
        .spawn(move || {
            let (notify_on, check, hotkey, secs) = {
                let c = daemon.config.read().unwrap();
                (
                    c.general.notify,
                    c.general.update_check,
                    c.hotkeys.save.clone(),
                    c.buffer.replay_duration_secs,
                )
            };
            if !notify_on {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
            let mut lines = vec![format!("{hotkey} saves the last {secs}s")];
            if check {
                let current = env!("CARGO_PKG_VERSION");
                let mut result = Err(String::new());
                for attempt in 0..4 {
                    result = sc_core::update::fetch_latest(current);
                    if result.is_ok() {
                        break;
                    }
                    if attempt < 3 {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                    }
                }
                lines.push(match result {
                    Ok(Some(r)) => format!("Update {} is available. Open SimpleClip.", r.version),
                    Ok(None) => format!("You are up to date ({current})"),
                    Err(_) => "Couldn't check for updates".to_string(),
                });
            }
            crate::popup::show(crate::popup::Popup {
                kind: crate::popup::Kind::Info,
                compact: false,
                title: "SimpleClip is running".into(),
                lines,
            });
        })
        .ok();
}

/// Show a one-line popup in the top-right corner (errors, hotkey problems).
pub fn notify(title: &str, body: &str) {
    let kind = if title.to_ascii_lowercase().contains("couldn") {
        crate::popup::Kind::Error
    } else {
        crate::popup::Kind::Info
    };
    crate::popup::show(crate::popup::Popup::simple(kind, title, body));
}

/// Opt the process into per-monitor DPI awareness so the popup is crisp on
/// scaled displays. Must run before any window is created.
pub fn init_dpi() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// The built-in save sound (a camera shutter, see assets/sounds/CREDITS.md).
static SAVE_WAV: &[u8] = include_bytes!("../../../assets/sounds/save.wav");

/// Play the save sound: the user's own file if one is set and exists, else the
/// built-in one. Async; returns immediately.
pub fn play_save_sound(g: &sc_core::config::GeneralSettings) {
    use windows::core::HSTRING;
    unsafe {
        if let Some(file) = g.save_sound_file.as_ref().filter(|f| f.exists()) {
            let name = HSTRING::from(file.as_os_str());
            if PlaySoundW(&name, None, SND_FILENAME | SND_ASYNC | SND_NODEFAULT).as_bool() {
                return;
            }
        }
        let _ = PlaySoundW(
            windows::core::PCWSTR(SAVE_WAV.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

// ------------------------------------------------------------- autostart --

pub fn autostart_enabled() -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ};
    use winreg::RegKey;
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(RUN_KEY, KEY_READ)
        .and_then(|k| k.get_value::<String, _>(AUMID))
        .is_ok()
}

pub fn set_autostart(on: bool) {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_WRITE};
    use winreg::RegKey;
    let Ok(key) = RegKey::predef(HKEY_CURRENT_USER).open_subkey_with_flags(RUN_KEY, KEY_WRITE)
    else {
        return;
    };
    if on {
        if let Ok(exe) = std::env::current_exe() {
            let _ = key.set_value(AUMID, &format!("\"{}\"", exe.display()));
        }
    } else {
        let _ = key.delete_value(AUMID);
    }
}

// ------------------------------------------------------------------ tray --

fn tray_icon_image(active: bool) -> tray_icon::Icon {
    // The app logo at 32x32 (assets/tray-32.rgba, made by assets/make-icon.ps1). While
    // paused it is drawn in grey and slightly faded, so the state shows at a glance.
    const N: u32 = 32;
    static LOGO: &[u8] = include_bytes!("../../../assets/tray-32.rgba");
    let mut px = LOGO.to_vec();
    if !active {
        for i in (0..px.len()).step_by(4) {
            let grey =
                (px[i] as f32 * 0.30 + px[i + 1] as f32 * 0.59 + px[i + 2] as f32 * 0.11) as u8;
            px[i] = grey;
            px[i + 1] = grey;
            px[i + 2] = grey;
            px[i + 3] = (px[i + 3] as f32 * 0.7) as u8;
        }
    }
    tray_icon::Icon::from_rgba(px, N, N).expect("valid icon")
}

/// Handle the open window if there is one: minimise it when it is the window in
/// front, otherwise restore it and bring it up. Returns false when there is no
/// such window.
fn raise_gui() -> bool {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut title = [0u16; 32];
        let n = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut class = [0u16; 64];
        let c = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let is_title = String::from_utf16_lossy(&title[..n]) == "SimpleClip";
        // The save card is also titled "SimpleClip"; it is not the app.
        let is_card = String::from_utf16_lossy(&class[..c]) == "SimpleClipPopup";
        if is_title && !is_card && (IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool()) {
            *(found.0 as *mut Option<HWND>) = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    unsafe {
        let mut found: Option<HWND> = None;
        let _ = EnumWindows(
            Some(visit),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        );
        let Some(hwnd) = found else { return false };
        // Already the window in front: pressing the shortcut again tucks it away.
        if !IsIconic(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            let _ = ShowWindow(hwnd, SW_MINIMIZE);
            return true;
        }
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
        true
    }
}

/// The open-window shortcut: bring SimpleClip up (a new window if none is open),
/// or put it away if it is already in front.
fn open_gui() {
    if raise_gui() {
        return;
    }
    let gui = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("sc-gui.exe")))
        .filter(|p| p.exists());
    match gui {
        Some(p) => {
            let _ = Command::new(p).spawn();
        }
        None => notify("SimpleClip", "sc-gui.exe was not found next to scd.exe."),
    }
}

fn open_folder(path: &std::path::Path) {
    let _ = std::fs::create_dir_all(path);
    let _ = Command::new("explorer.exe").arg(path).spawn();
}

/// Run the tray icon and its message loop on the calling (main) thread. Returns
/// when the user picks Quit.
pub fn run_tray(daemon: Daemon) {
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{TrayIconBuilder, TrayIconEvent};

    let save = MenuItem::new("Save clip now", true, None);
    let shot = MenuItem::new("Take screenshot", true, None);
    let open_gui_item = MenuItem::new("Open SimpleClip", true, None);
    let folder = MenuItem::new("Open clips folder", true, None);
    let pause = MenuItem::new("Pause buffering", true, None);
    let autostart = CheckMenuItem::new("Start with Windows", true, autostart_enabled(), None);
    let quit = MenuItem::new("Quit", true, None);

    let menu = Menu::new();
    let _ = menu.append_items(&[
        &save,
        &shot,
        &PredefinedMenuItem::separator(),
        &open_gui_item,
        &folder,
        &PredefinedMenuItem::separator(),
        &pause,
        &autostart,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("SimpleClip - buffering")
        .with_icon(tray_icon_image(true))
        .build()
        .expect("tray icon");

    let (save_id, shot_id, gui_id, folder_id, pause_id, auto_id, quit_id) = (
        save.id().clone(),
        shot.id().clone(),
        open_gui_item.id().clone(),
        folder.id().clone(),
        pause.id().clone(),
        autostart.id().clone(),
        quit.id().clone(),
    );

    {
        let d = daemon.clone();
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            let id = e.id;
            if id == save_id {
                let d = d.clone();
                std::thread::spawn(move || save_from_hotkey(&d));
            } else if id == shot_id {
                let d = d.clone();
                std::thread::spawn(move || screenshot_from_hotkey(&d));
            } else if id == gui_id {
                open_gui();
            } else if id == folder_id {
                open_folder(&d.config.read().unwrap().save.resolve_dir());
            } else if id == pause_id {
                let paused = d.capture_state() != CaptureState::Paused;
                d.set_paused(paused);
            } else if id == auto_id {
                // Persist through the config so the setting survives restarts
                // (the daemon re-applies it from there).
                let on = !autostart_enabled();
                set_autostart(on);
                if let Ok(path) = sc_core::config::Config::default_path() {
                    if let Ok(mut cfg) = sc_core::config::Config::load(&path) {
                        cfg.general.autostart = on;
                        let _ = cfg.save(&path);
                    }
                }
            } else if id == quit_id {
                unsafe { PostQuitMessage(0) };
            }
        }));
    }
    TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
        if let TrayIconEvent::DoubleClick { .. } = e {
            open_gui();
        }
    }));

    // The standard message loop services the tray's hidden window and menus.
    // Poll rather than block so pause changes can refresh the icon too.
    let mut msg = MSG::default();
    let mut shown_paused = false;
    loop {
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // Reflect pause state however it changed (menu, `sc pause`, or the GUI).
        let paused = daemon.capture_state() == CaptureState::Paused;
        if paused != shown_paused {
            shown_paused = paused;
            let _ = tray.set_icon(Some(tray_icon_image(!paused)));
            let _ = tray.set_tooltip(Some(if paused {
                "SimpleClip - paused"
            } else {
                "SimpleClip - buffering"
            }));
            pause.set_text(if paused {
                "Resume buffering"
            } else {
                "Pause buffering"
            });
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Whether a program is running in exclusive full screen (Direct3D), the mode
/// games use and a browser or document never does.
pub fn exclusive_fullscreen() -> bool {
    use windows::Win32::UI::Shell::{SHQueryUserNotificationState, QUNS_RUNNING_D3D_FULL_SCREEN};
    unsafe {
        SHQueryUserNotificationState()
            .map(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifier_combos() {
        let (m, vk) = parse_hotkey("ctrl+alt+F10").unwrap();
        assert_eq!(m, MOD_CONTROL | MOD_ALT);
        assert_eq!(vk, 0x79); // VK_F10
        let (m, vk) = parse_hotkey("SUPER + shift + c").unwrap();
        assert_eq!(m, MOD_WIN | MOD_SHIFT);
        assert_eq!(vk, b'C' as u32);
    }

    #[test]
    fn parses_bare_and_named_keys() {
        assert_eq!(parse_hotkey("F9"), Some((HOT_KEY_MODIFIERS(0), 0x78)));
        assert_eq!(parse_hotkey("ALT+PRINT").unwrap().1, 0x2C);
        assert_eq!(parse_hotkey("CTRL+7").unwrap().1, b'7' as u32);
    }

    #[test]
    fn parses_any_key() {
        assert_eq!(parse_hotkey("`"), Some((HOT_KEY_MODIFIERS(0), 0xC0)));
        assert_eq!(parse_hotkey("CTRL+ALT+LEFT").unwrap().1, 0x25);
        assert_eq!(parse_hotkey("ALT+VK_FA").unwrap().1, 0xFA);
        assert_eq!(parse_hotkey("SHIFT+-").unwrap().1, 0xBD);
    }

    #[test]
    fn rejects_malformed_hotkeys() {
        assert!(parse_hotkey("").is_none());
        assert!(parse_hotkey("CTRL+").is_none());
        assert!(parse_hotkey("CTRL+ALT").is_none()); // modifiers only
        assert!(parse_hotkey("A+B").is_none()); // two keys
        assert!(parse_hotkey("ALT+F99").is_none());
        assert!(parse_hotkey("ALT+NOPE").is_none());
    }

    #[test]
    fn local_time_is_plausible() {
        let (y, mo, d, h, mi, s) = local_time();
        assert!(y >= 2024 && (1..=12).contains(&mo) && (1..=31).contains(&d));
        assert!(h < 24 && mi < 60 && s < 61);
    }
}
