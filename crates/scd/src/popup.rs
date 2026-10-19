//! A small pop-down card in the top-right corner of the screen, shown when a
//! clip or screenshot is saved (or fails). It is a click-through, never-focused,
//! always-on-top layered window, so it can't steal focus from a game, and it
//! fades itself out. Windows-only; drawn with plain GDI to keep the daemon light.

use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW,
    EndPaint, FillRect, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, CLEARTYPE_QUALITY,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
    FW_SEMIBOLD, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
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

/// What to show. `ok` picks the accent colour (green / red).
#[derive(Clone)]
pub struct Popup {
    pub ok: bool,
    pub title: String,
    /// Detail lines under the title (kept short; long ones are ellipsised).
    pub lines: Vec<String>,
}

impl Popup {
    pub fn simple(ok: bool, title: &str, line: &str) -> Self {
        Self {
            ok,
            title: title.to_string(),
            lines: vec![line.to_string()],
        }
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

const WIDTH: i32 = 380;
const MARGIN: i32 = 18;
const HOLD: Duration = Duration::from_millis(5200);

fn scale() -> f32 {
    (unsafe { GetDpiForSystem() }) as f32 / 96.0
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
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

unsafe fn run_one(
    popup: &Popup,
    hinst: windows::Win32::Foundation::HINSTANCE,
    class: PCWSTR,
    rx: &std::sync::mpsc::Receiver<Popup>,
) {
    *CURRENT.lock().unwrap() = Some(popup.clone());
    let s = scale();
    let (w, h) = (
        (WIDTH as f32 * s) as i32,
        ((54 + 22 * popup.lines.len() as i32 + 16) as f32 * s) as i32,
    );
    let margin = (MARGIN as f32 * s) as i32;

    // Top-right of the primary monitor's work area (clear of the taskbar).
    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut RECT as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let x = work.right - w - margin;
    let y_final = work.top + margin;
    let slide = (28.0 * s) as i32;

    let hwnd = match CreateWindowExW(
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        class,
        w!("SimpleClip"),
        WS_POPUP,
        x,
        y_final - slide,
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
    let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, (14.0 * s) as i32, (14.0 * s) as i32);
    SetWindowRgn(hwnd, Some(rgn), true);
    let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 0, LWA_ALPHA);
    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);

    let started = Instant::now();
    let fade_in = Duration::from_millis(220);
    let fade_out = Duration::from_millis(280);
    let total = fade_in + HOLD + fade_out;
    let mut msg = MSG::default();
    loop {
        let t = started.elapsed();
        if t >= total || rx.try_recv().map(restash).unwrap_or(false) {
            break;
        }
        // ease-out slide + fade in, hold, fade out
        let (alpha, y) = if t < fade_in {
            let p = t.as_secs_f32() / fade_in.as_secs_f32();
            let e = 1.0 - (1.0 - p).powi(3);
            (
                (e * 235.0) as u8,
                y_final - slide + (slide as f32 * e) as i32,
            )
        } else if t < fade_in + HOLD {
            (235, y_final)
        } else {
            let p = (t - fade_in - HOLD).as_secs_f32() / fade_out.as_secs_f32();
            (((1.0 - p) * 235.0) as u8, y_final)
        };
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOACTIVATE,
        );
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(Duration::from_millis(12));
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

    let bg = CreateSolidBrush(rgb(24, 26, 34));
    FillRect(hdc, &rc, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));

    // Accent bar down the left edge.
    let accent = if p.ok {
        rgb(67, 200, 117)
    } else {
        rgb(235, 87, 87)
    };
    let bar = CreateSolidBrush(accent);
    let bar_rc = RECT {
        left: 0,
        top: 0,
        right: (5.0 * s) as i32,
        bottom: rc.bottom,
    };
    FillRect(hdc, &bar_rc, bar);
    let _ = DeleteObject(HGDIOBJ(bar.0));

    SetBkMode(hdc, TRANSPARENT);
    let font = |pt: i32, weight: i32| {
        CreateFontW(
            -((pt as f32 * s * 96.0 / 72.0) as i32),
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
            w!("Segoe UI"),
        )
    };
    let title_font = font(12, FW_SEMIBOLD.0 as i32);
    let body_font = font(10, FW_NORMAL.0 as i32);

    let pad = (18.0 * s) as i32;
    let mut y = (12.0 * s) as i32;
    let row = |top: i32, h: f32| RECT {
        left: pad,
        top,
        right: rc.right - pad,
        bottom: top + (h * s) as i32,
    };
    let flags = DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS | DT_NOPREFIX;

    // Title, with a check / cross glyph in the accent colour.
    let old = SelectObject(hdc, HGDIOBJ(title_font.0));
    SetTextColor(hdc, accent);
    let glyph = if p.ok { "\u{2713}  " } else { "\u{2715}  " };
    let mut text: Vec<u16> = format!("{glyph}{}", p.title).encode_utf16().collect();
    let mut r = row(y, 28.0);
    DrawTextW(hdc, &mut text, &mut r, flags);
    y += (30.0 * s) as i32;

    SelectObject(hdc, HGDIOBJ(body_font.0));
    for (i, line) in p.lines.iter().enumerate() {
        // First detail line (the file name) is brighter than the stats under it.
        SetTextColor(
            hdc,
            if i == 0 {
                rgb(232, 234, 240)
            } else {
                rgb(150, 156, 172)
            },
        );
        let mut text: Vec<u16> = line.encode_utf16().collect();
        let mut r = row(y, 22.0);
        DrawTextW(hdc, &mut text, &mut r, flags);
        y += (22.0 * s) as i32;
    }

    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(title_font.0));
    let _ = DeleteObject(HGDIOBJ(body_font.0));
    let _ = EndPaint(hwnd, &ps);
}
