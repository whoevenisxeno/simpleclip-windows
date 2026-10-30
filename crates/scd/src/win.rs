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
const AUMID: &str = "SimpleClip";

pub fn attach_parent_console() -> Option<std::fs::File> {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS).ok()? };
    std::fs::OpenOptions::new().write(true).open("CONOUT$").ok()
}

pub fn acquire_single_instance() -> bool {
    unsafe {
        match CreateMutexW(None, true, w!("Local\\SimpleClipDaemon")) {
            Ok(_) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

pub fn local_time() -> (u16, u16, u16, u16, u16, u16) {
    let t = unsafe { GetLocalTime() };
    (t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
}

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
                    return None;
                }
            }
        }
    }
    key.map(|k| (mods, k))
}

fn parse_key(k: &str) -> Option<u32> {
    sc_core::keys::vk_from_name(k)
}

pub fn spawn_hotkey(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-hotkey".into())
        .spawn(move || hotkey_loop(daemon))
        .expect("hotkey thread");
}

const SAVE_ID: i32 = 0x5C01;
const SHOT_ID: i32 = 0x5C02;
const OPEN_ID: i32 = 0x5C03;

struct Binding {
    id: i32,
    what: &'static str,
    registered: Option<String>,
    failed: Option<String>,
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

fn mask_alt() {
    unsafe {
        if GetAsyncKeyState(VK_MENU.0 as i32) as u16 & 0x8000 == 0 {
            return;
        }
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(0xE8),
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
            let mut title = "SimpleClip is running".to_string();
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
                match result {
                    Ok(Some(r)) => {
                        title = format!("SimpleClip {} is available", r.version);
                        if first_time_for(&mut LAST_ANNOUNCED.lock().unwrap(), &r.version)
                            && daemon.config.read().unwrap().general.update_sound
                        {
                            play_update_sound();
                        }
                        lines.insert(
                            0,
                            "Open SimpleClip to update. It only takes a moment.".to_string(),
                        );
                    }
                    Ok(None) => lines.push(format!("You are up to date ({current})")),
                    Err(_) => lines.push("Couldn't check for updates".to_string()),
                }
            }
            crate::popup::show(crate::popup::Popup {
                kind: crate::popup::Kind::Info,
                compact: false,
                title,
                lines,
            });
        })
        .ok();
}

pub fn notify(title: &str, body: &str) {
    let kind = if title.to_ascii_lowercase().contains("couldn") {
        crate::popup::Kind::Error
    } else {
        crate::popup::Kind::Info
    };
    crate::popup::show(crate::popup::Popup::simple(kind, title, body));
}

pub fn init_dpi() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

static SAVE_WAV: &[u8] = include_bytes!("../../../assets/sounds/save.wav");
static UPDATE_WAV: &[u8] = include_bytes!("../../../assets/sounds/update.wav");

pub fn play_update_sound() {
    unsafe {
        let _ = PlaySoundW(
            windows::core::PCWSTR(UPDATE_WAV.as_ptr() as *const u16),
            None,
            SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
        );
    }
}

const UPDATE_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60);

static LAST_ANNOUNCED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn first_time_for(last: &mut Option<String>, version: &str) -> bool {
    if last.as_deref() == Some(version) {
        return false;
    }
    *last = Some(version.to_string());
    true
}

pub fn announce_update(daemon: &Daemon, version: &str, with_card: bool) {
    let first = first_time_for(&mut LAST_ANNOUNCED.lock().unwrap(), version);
    let cfg = daemon.config.read().unwrap().clone();
    if with_card && cfg.general.notify {
        crate::popup::configure(&cfg.general);
        crate::popup::show(crate::popup::Popup {
            kind: crate::popup::Kind::Info,
            compact: false,
            title: format!("SimpleClip {version} is available"),
            lines: vec![
                "Open SimpleClip to update. It only takes a moment.".to_string(),
                format!("You have {}", env!("CARGO_PKG_VERSION")),
            ],
        });
    }
    if first && cfg.general.update_sound {
        play_update_sound();
    }
}

pub fn watch_updates(daemon: Daemon) {
    std::thread::Builder::new()
        .name("sc-update-watch".into())
        .spawn(move || loop {
            std::thread::sleep(UPDATE_EVERY);
            if !daemon.config.read().unwrap().general.update_check {
                continue;
            }
            if let Ok(Some(release)) = sc_core::update::fetch_latest(env!("CARGO_PKG_VERSION")) {
                announce_update(&daemon, &release.version, true);
            }
        })
        .ok();
}

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

fn tray_icon_image(active: bool) -> tray_icon::Icon {
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

fn raise_gui() -> bool {
    unsafe extern "system" fn visit(hwnd: HWND, found: LPARAM) -> BOOL {
        let mut title = [0u16; 32];
        let n = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut class = [0u16; 64];
        let c = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let is_title = String::from_utf16_lossy(&title[..n]) == "SimpleClip";
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
        assert_eq!(vk, 0x79);
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
        assert!(parse_hotkey("CTRL+ALT").is_none());
        assert!(parse_hotkey("A+B").is_none());
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

#[cfg(test)]
mod update_tests {
    use super::*;

    #[test]
    fn a_version_is_announced_once() {
        let mut last = None;
        assert!(first_time_for(&mut last, "6.4.0"));
        assert!(!first_time_for(&mut last, "6.4.0"));
        assert!(first_time_for(&mut last, "6.5.0"));
        assert!(!first_time_for(&mut last, "6.5.0"));
    }
}
