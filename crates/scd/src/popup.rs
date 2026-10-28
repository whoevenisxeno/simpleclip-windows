use sc_core::config::{GeneralSettings, PopupPosition};
use sc_core::look::{look, Look};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreatePen,
    CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, Ellipse, FillRect, GdiFlush, GetDC,
    ReleaseDC, SelectObject, SetBkMode, SetBrushOrgEx, SetStretchBltMode, SetTextColor, StretchBlt,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION,
    CLEARTYPE_QUALITY, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS, DT_LEFT,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, HALFTONE, HDC,
    HGDIOBJ, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::UI::HiDpi::GetDpiForSystem;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
    RegisterClassExW, ShowWindow, SystemParametersInfoW, TranslateMessage, UpdateLayeredWindow,
    CS_HREDRAW, CS_VREDRAW, MSG, PM_REMOVE, SPI_GETWORKAREA, SW_SHOWNOACTIVATE,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, ULW_ALPHA, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Saved,
    Error,
    Info,
}

#[derive(Clone)]
pub struct Popup {
    pub kind: Kind,
    pub title: String,
    pub lines: Vec<String>,
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

#[derive(Clone)]
struct Placement {
    position: PopupPosition,
    animate: bool,
    theme: String,
    accent: Option<String>,
}

static PLACEMENT: Mutex<Placement> = Mutex::new(Placement {
    position: PopupPosition::TopRight,
    animate: true,
    theme: String::new(),
    accent: None,
});

pub fn configure(g: &GeneralSettings) {
    if let Ok(mut p) = PLACEMENT.lock() {
        *p = Placement {
            position: g.popup_position,
            animate: g.popup_animation,
            theme: g.theme.clone(),
            accent: g.accent.clone(),
        };
    }
}

static TX: OnceLock<Sender<Popup>> = OnceLock::new();

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
const BADGE: f32 = 30.0;
const RADIUS: f32 = 14.0;
const SHADOW: f32 = 24.0;

fn scale() -> f32 {
    (unsafe { GetDpiForSystem() }) as f32 / 96.0
}

fn rgb(c: [u8; 3]) -> COLORREF {
    COLORREF(c[0] as u32 | (c[1] as u32) << 8 | (c[2] as u32) << 16)
}

fn mix(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2])]
}

fn luminance(c: [u8; 3]) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c[0]) + 0.7152 * lin(c[1]) + 0.0722 * lin(c[2])
}

fn on_color(c: [u8; 3]) -> [u8; 3] {
    if luminance(c) > 0.42 {
        [20, 20, 22]
    } else {
        [255, 255, 255]
    }
}

fn kind_color(kind: Kind, l: &Look) -> [u8; 3] {
    match kind {
        Kind::Saved => l.green,
        Kind::Error => l.red,
        Kind::Info => l.accent,
    }
}

fn system_dark() -> bool {
    let mut value: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut len),
        )
    };
    status.is_err() || value == 0
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
            while let Ok(newer) = rx.try_recv() {
                popup = newer;
            }
            run_one(&popup, hinst.into(), class, &rx);
        }
    }
}

fn size_of(popup: &Popup, s: f32) -> (i32, i32) {
    if popup.compact {
        ((COMPACT_WIDTH as f32 * s) as i32, (52.0 * s) as i32)
    } else {
        (
            (WIDTH as f32 * s) as i32,
            ((18.0 + 30.0 + 21.0 * popup.lines.len() as f32 + 16.0) * s) as i32,
        )
    }
}

fn ease_out(p: f32) -> f32 {
    1.0 - (1.0 - p.clamp(0.0, 1.0)).powi(3)
}

fn ease_in(p: f32) -> f32 {
    p.clamp(0.0, 1.0).powi(2)
}

fn rounded_distance(px: f32, py: f32, half: (f32, f32), r: f32) -> f32 {
    let qx = px.abs() - (half.0 - r);
    let qy = py.abs() - (half.1 - r);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn composite(card: &[u8], cw: i32, ch: i32, margin: i32, look: &Look, s: f32) -> Vec<u8> {
    let (w, h) = ((cw + 2 * margin) as usize, (ch + 2 * margin) as usize);
    let half = (cw as f32 / 2.0, ch as f32 / 2.0);
    let radius = RADIUS * s;
    let border_w = s.max(1.0);
    let sigma = 9.0 * s;
    let offset = 5.0 * s;
    let amp = if look.dark { 0.6 } else { 0.3 };
    let line = look.line;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let px = x as f32 + 0.5 - margin as f32 - half.0;
            let py = y as f32 + 0.5 - margin as f32 - half.1;
            let d = rounded_distance(px, py, half, radius);
            let cover = (0.5 - d).clamp(0.0, 1.0);
            let ds = rounded_distance(px, py - offset, half, radius);
            let shadow = if ds <= 0.0 {
                amp
            } else {
                amp * (-(ds / sigma) * (ds / sigma)).exp()
            };
            let alpha = cover + shadow * (1.0 - cover);
            let (mut b, mut g, mut r) = (0.0, 0.0, 0.0);
            if cover > 0.0 {
                let ix = ((px + half.0).floor() as i32).clamp(0, cw - 1) as usize;
                let iy = ((py + half.1).floor() as i32).clamp(0, ch - 1) as usize;
                let at = (iy * cw as usize + ix) * 4;
                let inner = (0.5 - (d + border_w)).clamp(0.0, 1.0);
                let c =
                    |content: u8, edge: u8| edge as f32 + (content as f32 - edge as f32) * inner;
                b = c(card[at], line[2]) * cover;
                g = c(card[at + 1], line[1]) * cover;
                r = c(card[at + 2], line[0]) * cover;
            }
            let o = (y * w + x) * 4;
            out[o] = b.round() as u8;
            out[o + 1] = g.round() as u8;
            out[o + 2] = r.round() as u8;
            out[o + 3] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

struct Surface {
    screen: HDC,
    mem: HDC,
    dib: windows::Win32::Graphics::Gdi::HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Surface {
    unsafe fn new(width: i32, height: i32, pixels: Option<&[u8]>) -> Option<Self> {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            return None;
        }
        if let Some(p) = pixels {
            std::ptr::copy_nonoverlapping(p.as_ptr(), bits as *mut u8, p.len());
        }
        let old = SelectObject(mem, HGDIOBJ(dib.0));
        Some(Self {
            screen,
            mem,
            dib,
            old,
            bits: bits as *mut u8,
            width,
            height,
        })
    }

    unsafe fn read(&self) -> Vec<u8> {
        let _ = GdiFlush();
        let mut out = vec![0u8; (self.width * self.height * 4) as usize];
        std::ptr::copy_nonoverlapping(self.bits, out.as_mut_ptr(), out.len());
        out
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

unsafe fn paint_card(popup: &Popup, look: &Look, s: f32, width: i32, height: i32) -> Vec<u8> {
    let Some(surface) = Surface::new(width, height, None) else {
        return vec![0; (width * height * 4) as usize];
    };
    let hdc = surface.mem;
    let rc = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let bg = CreateSolidBrush(rgb(look.card));
    FillRect(hdc, &rc, bg);
    let _ = DeleteObject(HGDIOBJ(bg.0));

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

    let pad = (18.0 * s) as i32;
    let badge = (BADGE * s) as i32;
    let color = kind_color(popup.kind, look);
    let row_h = (30.0 * s) as i32;
    let top = if popup.compact {
        (height - badge) / 2
    } else {
        (16.0 * s) as i32 + (row_h - badge) / 2
    };
    let ss = 4;
    let big = badge * ss;
    let mem = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, big, big);
    let old_bmp = SelectObject(mem, HGDIOBJ(bmp.0));
    let bg2 = CreateSolidBrush(rgb(look.card));
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
    let fill = CreateSolidBrush(rgb(color));
    let none = CreatePen(PS_SOLID, 0, rgb(color));
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
    SetTextColor(mem, rgb(on_color(color)));
    let glyph = match popup.kind {
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
    let text_x = pad + badge + (14.0 * s) as i32;
    let first = mix(look.text, look.text_2, 0.35);
    let rest = mix(look.text_3, look.text_2, 0.55);
    if popup.compact {
        SelectObject(hdc, HGDIOBJ(title_font.0));
        SetTextColor(hdc, rgb(look.text));
        let mut t: Vec<u16> = popup.title.encode_utf16().collect();
        let mut measure = RECT::default();
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
        if let Some(detail) = popup.lines.first() {
            SelectObject(hdc, HGDIOBJ(body_font.0));
            SetTextColor(hdc, rgb(look.text_2));
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
        let mut y = (16.0 * s) as i32;
        let row = |top: i32, h: f32| RECT {
            left: text_x,
            top,
            right: rc.right - pad,
            bottom: top + (h * s) as i32,
        };
        SelectObject(hdc, HGDIOBJ(title_font.0));
        SetTextColor(hdc, rgb(look.text));
        let mut text: Vec<u16> = popup.title.encode_utf16().collect();
        let mut r = row(y, 30.0);
        DrawTextW(hdc, &mut text, &mut r, flags);
        y += (32.0 * s) as i32;

        SelectObject(hdc, HGDIOBJ(body_font.0));
        for (i, line) in popup.lines.iter().enumerate() {
            SetTextColor(hdc, rgb(if i == 0 { first } else { rest }));
            let mut text: Vec<u16> = line.encode_utf16().collect();
            let mut r = row(y, 22.0);
            DrawTextW(hdc, &mut text, &mut r, flags);
            y += (21.0 * s) as i32;
        }
    }

    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(title_font.0));
    let _ = DeleteObject(HGDIOBJ(body_font.0));
    surface.read()
}

unsafe fn present(hwnd: HWND, surface: &Surface, x: i32, y: i32, alpha: u8) {
    let position = POINT { x, y };
    let size = SIZE {
        cx: surface.width,
        cy: surface.height,
    };
    let origin = POINT { x: 0, y: 0 };
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let _ = UpdateLayeredWindow(
        hwnd,
        Some(surface.screen),
        Some(&position),
        Some(&size),
        Some(surface.mem),
        Some(&origin),
        COLORREF(0),
        Some(&blend),
        ULW_ALPHA,
    );
}

unsafe fn run_one(
    popup: &Popup,
    hinst: windows::Win32::Foundation::HINSTANCE,
    class: PCWSTR,
    rx: &std::sync::mpsc::Receiver<Popup>,
) {
    let place = PLACEMENT.lock().unwrap().clone();
    let look = look(&place.theme, place.accent.as_deref(), system_dark());
    let s = scale();
    let (cw, ch) = size_of(popup, s);
    let margin = (MARGIN as f32 * s) as i32;
    let shadow = (SHADOW * s) as i32;
    let (w, h) = (cw + 2 * shadow, ch + 2 * shadow);

    let mut work = RECT::default();
    let _ = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work as *mut RECT as *mut _),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    );
    let (cx, cy) = match place.position {
        PopupPosition::TopRight => (work.right - cw - margin, work.top + margin),
        PopupPosition::TopLeft => (work.left + margin, work.top + margin),
        PopupPosition::BottomRight => (work.right - cw - margin, work.bottom - ch - margin),
        PopupPosition::BottomLeft => (work.left + margin, work.bottom - ch - margin),
        PopupPosition::Center => (
            work.left + (work.right - work.left - cw) / 2,
            work.top + (work.bottom - work.top - ch) / 2,
        ),
    };
    let (x, y) = (cx - shadow, cy - shadow);
    let travel = (44.0 * s) as i32;
    let (dx, dy) = match place.position {
        PopupPosition::TopRight | PopupPosition::BottomRight => (travel, 0),
        PopupPosition::TopLeft | PopupPosition::BottomLeft => (-travel, 0),
        PopupPosition::Center => (0, travel / 2),
    };

    let card = paint_card(popup, &look, s, cw, ch);
    let pixels = composite(&card, cw, ch, shadow, &look, s);
    let Some(surface) = Surface::new(w, h, Some(&pixels)) else {
        return;
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
    present(hwnd, &surface, x + dx, y + dy, 0);
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
    let started = Instant::now();
    let total = fade_in + hold + fade_out;
    let mut msg = MSG::default();
    loop {
        let t = started.elapsed();
        if t >= total || rx.try_recv().map(restash).unwrap_or(false) {
            break;
        }
        let (alpha, k) = if t < fade_in {
            let p = t.as_secs_f32() / fade_in.as_secs_f32();
            (ease_out(p), ease_out(p))
        } else if t < fade_in + hold {
            (1.0, 1.0)
        } else {
            let p = (t - fade_in - hold).as_secs_f32() / fade_out.as_secs_f32();
            (1.0 - ease_in(p), 1.0 - ease_in(p))
        };
        let k = if place.animate { k } else { 1.0 };
        present(
            hwnd,
            &surface,
            x + ((1.0 - k) * dx as f32) as i32,
            y + ((1.0 - k) * dy as f32) as i32,
            (alpha * 255.0) as u8,
        );
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = DestroyWindow(hwnd);
}

fn restash(next: Popup) -> bool {
    if let Some(tx) = TX.get() {
        let _ = tx.send(next);
    }
    true
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    DefWindowProcW(hwnd, msg, wp, lp)
}
