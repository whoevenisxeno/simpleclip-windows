#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn w(&self) -> i32 {
        self.right - self.left
    }

    pub fn h(&self) -> i32 {
        self.bottom - self.top
    }

    pub fn covers(&self, o: &Rect) -> bool {
        self.left <= o.left && self.top <= o.top && self.right >= o.right && self.bottom >= o.bottom
    }
}

pub fn place(work: Rect, size: (i32, i32), at: [f32; 2]) -> (i32, i32) {
    let fit = |lo: i32, span: i32, len: i32, f: f32| {
        let want = lo + (f.clamp(0.0, 1.0) * span as f32).round() as i32;
        want.min(lo + span - len).max(lo)
    };
    (
        fit(work.left, work.w(), size.0, at[0]),
        fit(work.top, work.h(), size.1, at[1]),
    )
}

pub const SHELL: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

pub struct Front<'a> {
    pub rect: Rect,
    pub monitor: Rect,
    pub class: &'a str,
    pub title: &'a str,
    pub ours: bool,
    pub minimized: bool,
    pub framed: bool,
}

pub fn is_game(f: &Front) -> bool {
    !f.ours
        && !f.minimized
        && !f.framed
        && f.monitor.w() > 0
        && f.monitor.h() > 0
        && !SHELL.contains(&f.class)
        && !f.class.starts_with("SimpleClip")
        && f.title != "SimpleClip"
        && f.rect.covers(&f.monitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    #[test]
    fn the_overlay_lands_at_its_fraction_and_stays_on_screen() {
        let work = Rect::new(0, 0, 1920, 1040);
        assert_eq!(place(work, (400, 200), [0.0, 0.0]), (0, 0));
        assert_eq!(place(work, (400, 200), [0.5, 0.5]), (960, 520));
        assert_eq!(place(work, (400, 200), [1.0, 1.0]), (1520, 840));
        assert_eq!(place(work, (400, 200), [0.9, 0.95]), (1520, 840));
        assert_eq!(place(work, (400, 200), [-3.0, 7.0]), (0, 840));
        let second = Rect::new(-2560, 100, 0, 1540);
        assert_eq!(place(second, (400, 200), [0.25, 0.0]), (-1920, 100));
        assert_eq!(place(second, (400, 200), [1.0, 1.0]), (-400, 1340));
        assert_eq!(
            place(Rect::new(0, 0, 300, 100), (400, 200), [0.5, 0.5]),
            (0, 0)
        );
    }

    fn front<'a>(rect: Rect, class: &'a str, title: &'a str) -> Front<'a> {
        Front {
            rect,
            monitor: SCREEN,
            class,
            title,
            ours: false,
            minimized: false,
            framed: false,
        }
    }

    #[test]
    fn only_full_screen_windows_count_as_games() {
        assert!(is_game(&front(SCREEN, "UnrealWindow", "Fortnite")));
        assert!(is_game(&front(
            Rect::new(-8, -8, 1928, 1088),
            "SDL_app",
            "Game"
        )));
        assert!(!is_game(&front(
            Rect::new(0, 0, 1920, 1040),
            "Chrome_WidgetWin_1",
            "Tab"
        )));
        assert!(!is_game(&front(
            Rect::new(100, 100, 900, 700),
            "Notepad",
            "x"
        )));
        for shell in SHELL {
            assert!(!is_game(&front(SCREEN, shell, "")), "{shell}");
        }
        assert!(!is_game(&front(SCREEN, "Window Class", "SimpleClip")));
        assert!(!is_game(&front(SCREEN, "SimpleClipPopup", "")));
        let mut mine = front(SCREEN, "Game", "Game");
        mine.ours = true;
        assert!(!is_game(&mine));
        let mut small = front(SCREEN, "Game", "Game");
        small.minimized = true;
        assert!(!is_game(&small));
        let mut browser = front(Rect::new(-8, -8, 1928, 1088), "MozillaWindowClass", "Tab");
        browser.framed = true;
        assert!(!is_game(&browser));
        let mut other = front(SCREEN, "Game", "Game");
        other.monitor = Rect::new(1920, 0, 3840, 1080);
        assert!(!is_game(&other));
    }
}
