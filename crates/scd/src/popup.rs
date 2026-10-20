//! The small cards that slide in when a clip or screenshot is saved (or fails),
//! and when SimpleClip starts. A card is a click-through, never-focused,
//! always-on-top layered window, so it can't steal focus from a game, and it
//! fades itself out. Windows-only; drawn with plain GDI to keep the daemon light.

use sc_core::config::{GeneralSettings, PopupPosition};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontW, CreatePen,
    CreateRoundRectRgn, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, Ellipse, EndPaint,
    FillRect, GetStockObject, RoundRect, SelectObject, SetBkMode, SetBrushOrgEx, SetStretchBltMode,
    SetTextColor, SetWindowRgn, StretchBlt, CLEARTYPE_QUALITY, DT_CALCRECT, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
    FW_SEMIBOLD, HALFTONE, HGDIOBJ, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, SystemParametersInfoW,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE,
    SPI_GETWORKAREA, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WM_PAINT, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

/// What the card is about; picks its colour and icon.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    /// Something was saved (green check).
    Saved,
    /// Something went wrong (red cross).
    Error,
    /// SimpleClip itself has news, such as starting up (blue, so it never looks like a save).
    Info,
}

/// What to show.
#[derive(Clone)]
pub struct Popup {
    pub kind: Kind,
    pub title: String,
    /// Detail lines under the title (kept short; long ones are ellipsised).
    pub lines: Vec<String>,
    /// One short line instead of the full card. The first detail line is shown
    /// beside the title and the rest are dropped.
    pub compact: bool,
}

impl Popup {
    pub fn simple(kind: Kind, title: &str, line: &str) -> Self {
        Self {
            kind,
            title: title.to_string(),
            lines: vec![line.to_string()],
            compact: false,
        }
    }
}

/// Where cards appear and whether they slide; set from the config.
#[derive(Clone, Copy)]
struct Placement {
    position: PopupPosition,
    animate: bool,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    position: PopupPosition::TopRight,
    animate: true,
});

/// Follow the user's position and animation settings from now on.
pub fn configure(g: &GeneralSettings) {
    if let Ok(mut p) = PLACEMENT.lock() {
        *p = Placement {
            position: g.popup_position,
            animate: g.popup_animation,
        };
    }
}

static CURRENT: Mutex<Option<Popup>> = Mutex::new(None);
static TX: OnceLock<Sender<Popup>> = OnceLock::new();

/// Queue a popup. Cheap and non-blocking; the first call starts the UI thread.
pub fn show(p: Popup) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel::<Popup>();
        std::thread::Builder::new()
            .name("sc-popup".into())
            .spawn(move || ui_thread(rx))
            .expect("popup thread");
        tx
    });
    let _ = tx.send(p);
}

const WIDTH: i32 = 392;
const COMPACT_WIDTH: i32 = 268;
const MARGIN: i32 = 18;
const BADGE: f32 = 28.0;

fn scale() -> f32 {
    (unsafe { GetDpiForSystem() }) as f32 / 96.0
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

fn accent(kind: Kind) -> COLORREF {
    match kind {
        Kind::Saved => rgb(67, 200, 117),
        Kind::Error => rgb(235, 87, 87),
        Kind::Info => rgb(86, 156, 255),
    }
}

fn ui_thread(rx: std::sync::mpsc::Receiver<Popup>) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("SimpleClipPopup");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);

        while let Ok(mut popup) = rx.recv() {
            // If more arrived while we were busy, show only the newest.
            while let Ok(newer) = rx.try_recv() {
                popup = newer;
            }
            run_one(&popup, hinst.into(), class, &rx);
        }
    }
}

/// Size of a card in device pixels.
fn size_of(popup: &Popup, s: f32) -> (i32, i32) {
    if popup.compact {
        ((COMPACT_WIDTH as f32 * s) as i32, (48.0 * s) as i32)
    } else {
        (
            (WIDTH as f32 * s) as i32,
            ((22.0 + 28.0 + 21.0 * popup.lines.len() as f32 + 12.0) * s) as i32,
        )
    }
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

fn ease_in(p: f32) -> f32 {
    p.clamp(0.0, 1.0).powi(2)
}

unsafe fn run_one(
    popup: &Popup,
    hinst: windows::Win32::Foundation::HINSTANCE,
    class: PCWSTR,
    rx: &std::sync::mpsc::Receiver<Popup>,
) {
    *CURRENT.lock().unwrap() = Some(popup.clone());
    let place = *PLACEMENT.lock().unwrap();
    let s = scale();
    let (w, h) = size_of(popup, s);
    let margin = (MARGIN as f32 * s) as i32;

    // The primary monitor's work area (clear of the taskbar).
    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut RECT as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let (x, y) = match place.position {
        PopupPosition::TopRight => (work.right - w - margin, work.top + margin),
        PopupPosition::TopLeft => (work.left + margin, work.top + margin),
        PopupPosition::BottomRight => (work.right - w - margin, work.bottom - h - margin),
        PopupPosition::BottomLeft => (work.left + margin, work.bottom - h - margin),
        PopupPosition::Center => (
            work.left + (work.right - work.left - w) / 2,
            work.top + (work.bottom - work.top - h) / 2,
        ),
    };
    // Slide in from the nearest side edge; the centre card rises instead.
    let travel = (44.0 * s) as i32;
    let (dx, dy) = match place.position {
        PopupPosition::TopRight | PopupPosition::BottomRight => (travel, 0),
        PopupPosition::TopLeft | PopupPosition::BottomLeft => (-travel, 0),
        PopupPosition::Center => (0, travel / 2),
    };

    let hwnd = match CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        class,
        w!("SimpleClip"),
        WS_POPUP,
        x + dx,
        y + dy,
        w,
        h,
        None,
        None,
        Some(hinst),
        None,
    ) {
        Ok(h) => h,
        Err(_) => return,
    };
    let radius = (16.0 * s) as i32;
    let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, radius, radius);
    SetWindowRgn(hwnd, Some(rgn), true);
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    let (fade_in, fade_out) = if place.animate {
        (Duration::from_millis(340), Duration::from_millis(280))
    } else {
        (Duration::from_millis(90), Duration::from_millis(160))
    };
    let hold = Duration::from_millis(if popup.compact {
        3000
    } else if popup.kind == Kind::Info {
        6500
    } else {
        5200
    });
    let top_alpha = 240.0;
    let started = Instant::now();
    let total = fade_in + hold + fade_out;
    let mut msg = MSG::default();
    loop {
        let t = started.elapsed();
        if t >= total || rx.try_recv().map(restash).unwrap_or(false) {
            break;
        }
        // 0 = fully off to the side, 1 = in place.
        let (alpha, k) = if t < fade_in {
            let p = t.as_secs_f32() / fade_in.as_secs_f32();
            (ease_out(p) * top_alpha, ease_out(p))
        } else if t < fade_in + hold {
            (top_alpha, 1.0)
        } else {
            let p = (t - fade_in - hold).as_secs_f32() / fade_out.as_secs_f32();
            ((1.0 - ease_in(p)) * top_alpha, 1.0 - ease_in(p))
        };
        let k = if place.animate { k } else { 1.0 };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha as u8, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x + ((1.0 - k) * dx as f32) as i32,
            y + ((1.0 - k) * dy as f32) as i32,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = DestroyWindow(hwnd);
}

/// A newer popup arrived while one was on screen: stash it as the one to show next.
fn restash(next: Popup) -> bool {
    if let Some(tx) = TX.get() {
        let _ = tx.send(next);
    }
    true
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        paint(hwnd);
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}

unsafe fn paint(hwnd: HWND) {
    let Some(p) = CURRENT.lock().unwrap().clone() else {
        return;
    };
    let s = scale();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);

    // Card with a hairline border.
    let bg = CreateSolidBrush(rgb(27, 29, 39));
    FillRect(hdc, &rc, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));
    let border = CreatePen(PS_SOLID, 1, rgb(58, 62, 80));
    let old_pen = SelectObject(hdc, HGDIOBJ(border.0));
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let r = (16.0 * s) as i32;
    let _ = RoundRect(hdc, 0, 0, rc.right, rc.bottom, r, r);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(border.0));

    SetBkMode(hdc, TRANSPARENT);
    let font = |face: PCWSTR, pt: f32, weight: i32| {
        CreateFontW(
            -((pt * s * 96.0 / 72.0) as i32),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            Default::default(),
            Default::default(),
            Default::default(),
            CLEARTYPE_QUALITY,
            FF_DONTCARE.0 as u32,
            face,
        )
    };
    let title_font = font(w!("Segoe UI"), 12.0, FW_SEMIBOLD.0 as i32);
    let body_font = font(w!("Segoe UI"), 10.0, FW_NORMAL.0 as i32);
    let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX;

    let pad = (16.0 * s) as i32;
    let badge = (BADGE * s) as i32;
    let color = accent(p.kind);
    // Round badge with the icon, vertically centred on the first row.
    let row_h = (28.0 * s) as i32;
    let top = if p.compact {
        (rc.bottom - badge) / 2
    } else {
        (14.0 * s) as i32 + (row_h - badge) / 2
    };
    // The badge is drawn four times larger and scaled down, because plain GDI
    // shapes and glyphs have hard, jagged edges.
    let ss = 4;
    let big = badge * ss;
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, big, big);
    let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));
    let bg2 = CreateSolidBrush(rgb(27, 29, 39));
    FillRect(
        mem,
        &RECT {
            left: 0,
            top: 0,
            right: big,
            bottom: big,
        },
        bg2,
    );
    let _ = DeleteObject(HGDIOBJ(bg2.0));
    let fill = CreateSolidBrush(color);
    let none = CreatePen(PS_SOLID, 0, color);
    let ob = SelectObject(mem, HGDIOBJ(fill.0));
    let op = SelectObject(mem, HGDIOBJ(none.0));
    let _ = Ellipse(mem, 0, 0, big, big);
    SelectObject(mem, ob);
    SelectObject(mem, op);
    let _ = DeleteObject(HGDIOBJ(fill.0));
    let _ = DeleteObject(HGDIOBJ(none.0));

    SetBkMode(mem, TRANSPARENT);
    let big_font = font(
        w!("Segoe UI Symbol"),
        11.0 * ss as f32,
        FW_SEMIBOLD.0 as i32,
    );
    let old_font = SelectObject(mem, HGDIOBJ(big_font.0));
    SetTextColor(mem, rgb(18, 20, 28));
    let glyph = match p.kind {
        Kind::Saved => "\u{2713}",
        Kind::Error => "\u{2715}",
        Kind::Info => "\u{25B6}",
    };
    let mut g: Vec<u16> = glyph.encode_utf16().collect();
    let mut gr = RECT {
        left: 0,
        top: 0,
        right: big,
        bottom: big,
    };
    DrawTextW(
        mem,
        &mut g,
        &mut gr,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
    );
    SelectObject(mem, old_font);
    let _ = DeleteObject(HGDIOBJ(big_font.0));

    SetStretchBltMode(hdc, HALFTONE);
    let _ = SetBrushOrgEx(hdc, 0, 0, None);
    let _ = StretchBlt(
        hdc,
        pad,
        top,
        badge,
        badge,
        Some(mem),
        0,
        0,
        big,
        big,
        SRCCOPY,
    );
    SelectObject(mem, old_bmp);
    let _ = DeleteObject(HGDIOBJ(bmp.0));
    let _ = DeleteDC(mem);

    let old = SelectObject(hdc, HGDIOBJ(title_font.0));

    let text_x = pad + badge + (12.0 * s) as i32;
    if p.compact {
        // "Clip saved   12s" on one line.
        SelectObject(hdc, HGDIOBJ(title_font.0));
        SetTextColor(hdc, rgb(240, 242, 248));
        let mut t: Vec<u16> = p.title.encode_utf16().collect();
        let mut measure = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        DrawTextW(
            hdc,
            &mut t,
            &mut measure,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
        let mut tr = RECT {
            left: text_x,
            top: 0,
            right: rc.right - pad,
            bottom: rc.bottom,
        };
        DrawTextW(hdc, &mut t, &mut tr, flags);
        if let Some(detail) = p.lines.first() {
            SelectObject(hdc, HGDIOBJ(body_font.0));
            SetTextColor(hdc, rgb(150, 156, 172));
            let mut d: Vec<u16> = detail.encode_utf16().collect();
            let mut dr = RECT {
                left: text_x + (measure.right - measure.left) + (10.0 * s) as i32,
                top: 0,
                right: rc.right - pad,
                bottom: rc.bottom,
            };
            DrawTextW(hdc, &mut d, &mut dr, flags);
        }
    } else {
        let mut y = (14.0 * s) as i32;
        let row = |top: i32, h: f32| RECT {
            left: text_x,
            top,
            right: rc.right - pad,
            bottom: top + (h * s) as i32,
        };
        SelectObject(hdc, HGDIOBJ(title_font.0));
        SetTextColor(hdc, rgb(240, 242, 248));
        let mut text: Vec<u16> = p.title.encode_utf16().collect();
        let mut r = row(y, 28.0);
        DrawTextW(hdc, &mut text, &mut r, flags);
        y += (30.0 * s) as i32;

        SelectObject(hdc, HGDIOBJ(body_font.0));
        for (i, line) in p.lines.iter().enumerate() {
            // The first detail line (the file name) is brighter than the stats under it.
            SetTextColor(
                hdc,
                if i == 0 {
                    rgb(214, 218, 230)
                } else {
                    rgb(140, 146, 164)
                },
            );
            let mut text: Vec<u16> = line.encode_utf16().collect();
            let mut r = row(y, 22.0);
            DrawTextW(hdc, &mut text, &mut r, flags);
            y += (21.0 * s) as i32;
        }
    }

    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(title_font.0));
    let _ = DeleteObject(HGDIOBJ(body_font.0));
    let _ = EndPaint(hwnd, &ps);
}
