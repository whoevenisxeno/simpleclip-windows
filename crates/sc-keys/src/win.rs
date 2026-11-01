use crate::parent;
use ab_glyph::FontVec;
use sc_core::store::{self, Manifest};
use sc_keys::draw::{self, Lit};
use sc_keys::layout::Board;
use sc_keys::place::{self, Front, Rect};
use sc_keys::preview;
use sc_keys::settings::{self, Settings, Shape};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromPoint, MonitorFromWindow, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    HMONITOR, MONITORINFO, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowLongW, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsZoomed, KillTimer, PostMessageW, RegisterClassExW,
    SetTimer, SetWindowPos, SetWindowsHookExW, ShowWindow, TranslateMessage, UnhookWindowsHookEx,
    UpdateLayeredWindow, GWL_STYLE, HC_ACTION, HHOOK, HWND_TOPMOST, KBDLLHOOKSTRUCT, MSG,
    MSLLHOOKSTRUCT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, ULW_ALPHA,
    WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_CAPTION, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const FADE: Duration = Duration::from_millis(170);
const FLASH: Duration = Duration::from_millis(320);
const POKE: u32 = WM_APP + 1;
const MOVED: u32 = WM_APP + 2;
const TICK: usize = 1;
const ANIM: usize = 2;

struct Held {
    hwnd: Option<HWND>,
    posted: bool,
    want: [bool; 256],
    mouse: bool,
    down: [bool; 256],
    gone: [Option<Instant>; 256],
    buttons: [bool; 3],
    let_go: [Option<Instant>; 3],
    scroll: Option<(Instant, bool)>,
}

impl Held {
    const fn new() -> Self {
        Held {
            hwnd: None,
            posted: false,
            want: [false; 256],
            mouse: false,
            down: [false; 256],
            gone: [None; 256],
            buttons: [false; 3],
            let_go: [None; 3],
            scroll: None,
        }
    }

    fn poke(&mut self) {
        if self.posted {
            return;
        }
        if let Some(h) = self.hwnd {
            self.posted = unsafe { PostMessageW(Some(h), POKE, WPARAM(0), LPARAM(0)).is_ok() };
        }
    }

    fn key(&mut self, vk: usize, down: bool) {
        if vk >= 256 || !self.want[vk] || self.down[vk] == down {
            return;
        }
        self.down[vk] = down;
        self.gone[vk] = (!down).then(Instant::now);
        self.poke();
    }

    fn button(&mut self, i: usize, down: bool) {
        if !self.mouse || self.buttons[i] == down {
            return;
        }
        self.buttons[i] = down;
        self.let_go[i] = (!down).then(Instant::now);
        self.poke();
    }

    fn wheel(&mut self, up: bool) {
        if self.mouse {
            self.scroll = Some((Instant::now(), up));
            self.poke();
        }
    }

    fn clear(&mut self) {
        self.down = [false; 256];
        self.gone = [None; 256];
        self.buttons = [false; 3];
        self.let_go = [None; 3];
        self.scroll = None;
    }

    fn lit(&self, b: &Board, now: Instant) -> (Lit, bool) {
        let mut busy = false;
        let mut fade = |down: bool, gone: Option<Instant>, span: Duration| -> f32 {
            if down {
                return 1.0;
            }
            let Some(at) = gone else { return 0.0 };
            let p = now.duration_since(at).as_secs_f32() / span.as_secs_f32();
            if p >= 1.0 {
                return 0.0;
            }
            busy = true;
            let q = 1.0 - p;
            q * q * (3.0 - 2.0 * q)
        };
        let keys = b
            .caps
            .iter()
            .map(|c| {
                c.vks
                    .iter()
                    .map(|&v| fade(self.down[v as usize], self.gone[v as usize], FADE))
                    .fold(0.0, f32::max)
            })
            .collect();
        let mouse = [0, 1, 2].map(|i| fade(self.buttons[i], self.let_go[i], FADE));
        let (scroll, up) = match self.scroll {
            Some((at, up)) => (fade(false, Some(at), FLASH), up),
            None => (0.0, false),
        };
        (
            Lit {
                keys,
                mouse,
                scroll,
                up,
            },
            busy,
        )
    }

    fn unstick(&mut self) {
        let held = |vk: i32| unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 };
        let mut changed = false;
        for vk in 0..256 {
            if self.down[vk] && !held(vk as i32) {
                self.down[vk] = false;
                self.gone[vk] = Some(Instant::now());
                changed = true;
            }
        }
        let either = held(0x01) || held(0x02);
        for (i, now) in [either, held(0x04), either].into_iter().enumerate() {
            if self.buttons[i] && !now {
                self.buttons[i] = false;
                self.let_go[i] = Some(Instant::now());
                changed = true;
            }
        }
        if changed {
            self.poke();
        }
    }
}

thread_local! {
    static HELD: RefCell<Held> = const { RefCell::new(Held::new()) };
}

fn with<R>(f: impl FnOnce(&mut Held) -> R) -> Option<R> {
    HELD.with(|h| h.try_borrow_mut().ok().map(|mut h| f(&mut h)))
}

unsafe extern "system" fn kb(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        let k = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        with(|h| h.key(k.vkCode as usize, down));
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe extern "system" fn ms(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && lp.0 != 0 {
        let m = wp.0 as u32;
        match m {
            WM_LBUTTONDOWN | WM_LBUTTONUP => {
                with(|h| h.button(0, m == WM_LBUTTONDOWN));
            }
            WM_MBUTTONDOWN | WM_MBUTTONUP => {
                with(|h| h.button(1, m == WM_MBUTTONDOWN));
            }
            WM_RBUTTONDOWN | WM_RBUTTONUP => {
                with(|h| h.button(2, m == WM_RBUTTONDOWN));
            }
            WM_MOUSEWHEEL => {
                let s = &*(lp.0 as *const MSLLHOOKSTRUCT);
                let delta = (s.mouseData >> 16) as u16 as i16;
                with(|h| h.wheel(delta > 0));
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wp, lp)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_DPICHANGED || msg == WM_DISPLAYCHANGE {
        let _ = PostMessageW(Some(hwnd), MOVED, WPARAM(0), LPARAM(0));
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

struct Surface {
    screen: HDC,
    mem: HDC,
    dib: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
}

impl Surface {
    unsafe fn new(w: i32, h: i32) -> Option<Self> {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(dib) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
            let _ = DeleteDC(mem);
            ReleaseDC(None, screen);
            return None;
        };
        let old = SelectObject(mem, HGDIOBJ(dib.0));
        Some(Surface {
            screen,
            mem,
            dib,
            old,
            bits: bits as *mut u8,
            w,
            h,
        })
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(HGDIOBJ(self.dib.0));
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Target {
    work: Rect,
    dpi: f32,
}

fn rect(r: RECT) -> Rect {
    Rect::new(r.left, r.top, r.right, r.bottom)
}

unsafe fn monitor(m: HMONITOR) -> Option<(Rect, Rect, f32)> {
    let mut mi = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !GetMonitorInfoW(m, &mut mi).as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    Some((
        rect(mi.rcMonitor),
        rect(mi.rcWork),
        dx.max(48) as f32 / 96.0,
    ))
}

fn wide(buf: &[u16], n: i32) -> String {
    String::from_utf16_lossy(&buf[..n.clamp(0, buf.len() as i32) as usize])
}

unsafe fn game_target(parent: Option<u32>) -> Option<Target> {
    let fg = GetForegroundWindow();
    if fg.is_invalid() {
        return None;
    }
    let mut r = RECT::default();
    GetWindowRect(fg, &mut r).ok()?;
    let m = MonitorFromWindow(fg, MONITOR_DEFAULTTONULL);
    if m.is_invalid() {
        return None;
    }
    let (full, work, dpi) = monitor(m)?;
    let mut class = [0u16; 128];
    let c = GetClassNameW(fg, &mut class);
    let mut title = [0u16; 64];
    let t = GetWindowTextW(fg, &mut title);
    let mut pid = 0u32;
    GetWindowThreadProcessId(fg, Some(&mut pid));
    let (class, title) = (wide(&class, c), wide(&title, t));
    let front = Front {
        rect: rect(r),
        monitor: full,
        class: &class,
        title: &title,
        ours: pid == std::process::id() || Some(pid) == parent,
        minimized: IsIconic(fg).as_bool(),
        framed: IsZoomed(fg).as_bool()
            && GetWindowLongW(fg, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0,
    };
    place::is_game(&front).then_some(Target { work, dpi })
}

unsafe fn primary_target() -> Option<Target> {
    let m = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
    monitor(m).map(|(_, work, dpi)| Target { work, dpi })
}

struct App {
    dir: PathBuf,
    manifest: Manifest,
    parent: Option<u32>,
    settings: Settings,
    board: Board,
    font: Option<FontVec>,
    hwnd: HWND,
    hooks: Option<(HHOOK, HHOOK)>,
    target: Option<Target>,
    shown: Option<(Target, Settings)>,
    surface: Option<Surface>,
    anim: bool,
    reloaded: Instant,
    checked: Instant,
    told: Instant,
    looked: Instant,
}

impl App {
    fn arm(&self) {
        let mut want = [false; 256];
        for c in &self.board.caps {
            for &v in c.vks {
                want[v as usize] = true;
            }
        }
        let mouse = self.board.mouse.is_some();
        with(|h| {
            h.want = want;
            h.mouse = mouse;
            h.clear();
        });
    }

    fn write_preview(&self) {
        match preview::write(&self.dir, &self.settings.shape(), self.font.as_ref()) {
            Ok(()) => tracing::info!("preview written"),
            Err(e) => tracing::warn!(error = %e, "could not write the preview"),
        }
    }

    fn reload(&mut self) {
        let next = Settings::read(&self.manifest, &store::read_settings(&self.dir));
        if next == self.settings {
            return;
        }
        let old: Shape = self.settings.shape();
        self.settings = next;
        if next.shape() != old {
            self.board = Board::new(next.layout, next.mouse);
            self.arm();
            self.write_preview();
        }
        self.check();
    }

    fn check(&mut self) {
        let s = self.settings;
        let target = unsafe {
            if !s.enabled {
                None
            } else if s.games {
                game_target(self.parent)
            } else {
                primary_target()
            }
        };
        self.target = target;
        match target {
            Some(t) => {
                self.hook();
                with(|h| h.unstick());
                let fresh = self.shown.is_none();
                if self.shown != Some((t, s)) {
                    self.shown = Some((t, s));
                    self.draw();
                }
                unsafe {
                    let _ = SetWindowPos(
                        self.hwnd,
                        Some(HWND_TOPMOST),
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                    );
                    if fresh {
                        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                    }
                }
            }
            None => {
                if self.shown.take().is_some() {
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_HIDE);
                    }
                }
                self.unhook();
                self.animate(false);
            }
        }
    }

    fn hook(&mut self) {
        if self.hooks.is_some() {
            return;
        }
        unsafe {
            let inst = GetModuleHandleW(None).ok().map(|m| m.into());
            let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(kb), inst, 0);
            let m = SetWindowsHookExW(WH_MOUSE_LL, Some(ms), inst, 0);
            match (k, m) {
                (Ok(k), Ok(m)) => {
                    self.hooks = Some((k, m));
                    tracing::info!("hooks installed");
                }
                (k, m) => {
                    if let Ok(k) = k {
                        let _ = UnhookWindowsHookEx(k);
                    }
                    if let Ok(m) = m {
                        let _ = UnhookWindowsHookEx(m);
                    }
                    tracing::warn!("could not install the hooks");
                }
            }
        }
    }

    fn unhook(&mut self) {
        if let Some((k, m)) = self.hooks.take() {
            unsafe {
                let _ = UnhookWindowsHookEx(k);
                let _ = UnhookWindowsHookEx(m);
            }
            tracing::info!("hooks removed");
        }
        with(|h| h.clear());
    }

    fn animate(&mut self, on: bool) {
        if on == self.anim {
            return;
        }
        self.anim = on;
        unsafe {
            if on {
                SetTimer(Some(self.hwnd), ANIM, 15, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), ANIM);
            }
        }
    }

    fn draw(&mut self) {
        let now = Instant::now();
        let got = with(|h| {
            h.posted = false;
            h.lit(&self.board, now)
        });
        let Some(t) = self.target else { return };
        let Some((lit, busy)) = got else { return };
        self.animate(busy);
        let s = self.settings;
        let unit = draw::unit_for(s.layout, s.size, t.dpi);
        let img = draw::render(
            &self.board,
            s.theme,
            unit,
            s.opacity as f32 / 100.0,
            self.font.as_ref(),
            &lit,
        );
        let fits = self
            .surface
            .as_ref()
            .is_some_and(|f| f.w == img.w && f.h == img.h);
        if !fits {
            self.surface = unsafe { Surface::new(img.w, img.h) };
        }
        let Some(sf) = &self.surface else { return };
        let (x, y) = place::place(t.work, (img.w, img.h), s.at);
        let px = img.bgra();
        unsafe {
            std::ptr::copy_nonoverlapping(px.as_ptr(), sf.bits, px.len());
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                self.hwnd,
                Some(sf.screen),
                Some(&POINT { x, y }),
                Some(&SIZE { cx: sf.w, cy: sf.h }),
                Some(sf.mem),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    fn status(&self) {
        let line = if !self.settings.enabled {
            "Off"
        } else if self.target.is_some() {
            "Showing your keys"
        } else {
            "Hidden until a game is open"
        };
        let _ = store::write_status(&self.dir, line);
    }

    fn tick(&mut self) -> bool {
        let now = Instant::now();
        if now.duration_since(self.looked) >= Duration::from_secs(1) {
            self.looked = now;
            if let Some(pid) = self.parent {
                if !parent::alive(pid) {
                    return false;
                }
            }
        }
        if now.duration_since(self.reloaded) >= Duration::from_secs(1) {
            self.reloaded = now;
            self.reload();
        }
        if now.duration_since(self.checked) >= Duration::from_millis(500) {
            self.checked = now;
            self.check();
        }
        if now.duration_since(self.told) >= Duration::from_secs(2) {
            self.told = now;
            self.status();
        }
        true
    }
}

pub fn run(dir: &Path, parent: Option<u32>) {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(module) = GetModuleHandleW(None) else {
            return;
        };
        let class = w!("SimpleClipKeys");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            w!("SimpleClip keys"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            tracing::warn!("could not create the overlay window");
            return;
        };
        with(|h| h.hwnd = Some(hwnd));
        let manifest = settings::manifest();
        let s = Settings::read(&manifest, &store::read_settings(dir));
        let long_ago = Instant::now() - Duration::from_secs(3600);
        let mut app = App {
            dir: dir.to_path_buf(),
            manifest,
            parent,
            settings: s,
            board: Board::new(s.layout, s.mouse),
            font: draw::font(),
            hwnd,
            hooks: None,
            target: None,
            shown: None,
            surface: None,
            anim: false,
            reloaded: Instant::now(),
            checked: long_ago,
            told: long_ago,
            looked: long_ago,
        };
        if app.font.is_none() {
            tracing::warn!("no font found, keys will have no labels");
        }
        app.arm();
        app.write_preview();
        SetTimer(Some(hwnd), TICK, 100, None);
        let mut msg = MSG::default();
        let mut alive = app.tick();
        while alive {
            let got = GetMessageW(&mut msg, None, 0, 0);
            if got.0 <= 0 {
                break;
            }
            match (msg.hwnd == hwnd, msg.message) {
                (true, POKE) => app.draw(),
                (true, MOVED) => app.check(),
                (true, WM_TIMER) if msg.wParam.0 == ANIM => app.draw(),
                (true, WM_TIMER) if msg.wParam.0 == TICK => alive = app.tick(),
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        app.unhook();
        app.surface = None;
        let _ = store::write_status(dir, "Off");
        let _ = DestroyWindow(hwnd);
    }
}
