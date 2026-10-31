use crate::theme::icon;
use sc_core::addon::{self as legacy, EventsStatus};
use sc_core::store::{self, Button, Entry, Manifest, Status};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;
const REFRESH_EVERY: Duration = Duration::from_millis(1500);
const SAVE_AFTER: Duration = Duration::from_millis(400);
const BUTTON_WAIT: Duration = Duration::from_secs(15);
const UNINSTALL_WAIT: Duration = Duration::from_secs(20);
pub const EVENTS: &str = "events";

type Slot<T> = Arc<Mutex<Option<T>>>;

fn slot<T>() -> Slot<T> {
    Arc::new(Mutex::new(None))
}

fn put<T>(s: &Slot<T>, v: T, ctx: &egui::Context) {
    if let Ok(mut g) = s.lock() {
        *g = Some(v);
    }
    ctx.request_repaint();
}

fn take<T>(s: &Slot<T>) -> Option<T> {
    s.lock().ok()?.take()
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Get,
    Update,
    Current,
    Needs(String),
    Local,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub about: String,
    pub have: Option<String>,
    pub latest: Option<String>,
    pub state: State,
    pub manifest: Option<Manifest>,
}

impl Card {
    pub fn installed(&self) -> bool {
        self.have.is_some()
    }

    pub fn version_line(&self) -> String {
        match (&self.have, &self.latest) {
            (Some(h), Some(l)) if h != l && self.state != State::Current => {
                format!("Version {h} installed. {l} is available")
            }
            (Some(h), _) => format!("Version {h}"),
            (None, Some(l)) => format!("Version {l}"),
            (None, None) => String::new(),
        }
    }
}

fn title_of(id: &str) -> String {
    Manifest {
        name: id.into(),
        ..Default::default()
    }
    .title()
}

pub fn cards(list: &[Entry], have: &[(PathBuf, Manifest)], app: &str) -> Vec<Card> {
    let mut out: Vec<Card> = list
        .iter()
        .map(|e| {
            let m = have
                .iter()
                .find(|(_, m)| m.name == e.id)
                .map(|(_, m)| m.clone());
            let state = match (&m, e.fits(app)) {
                (Some(m), _) if !store::needs_update(m, e) => State::Current,
                (_, false) => State::Needs(e.min_app.clone()),
                (Some(_), true) => State::Update,
                (None, true) => State::Get,
            };
            let title = if !e.title.is_empty() {
                e.title.clone()
            } else {
                m.as_ref().map_or_else(|| title_of(&e.id), Manifest::title)
            };
            let about = if !e.about.is_empty() {
                e.about.clone()
            } else {
                m.as_ref().map(|m| m.about.clone()).unwrap_or_default()
            };
            Card {
                id: e.id.clone(),
                title,
                about,
                have: m.as_ref().map(|m| m.version.clone()),
                latest: Some(e.version.clone()),
                state,
                manifest: m,
            }
        })
        .collect();
    for (_, m) in have {
        if list.iter().any(|e| e.id == m.name) {
            continue;
        }
        out.push(Card {
            id: m.name.clone(),
            title: m.title(),
            about: m.about.clone(),
            have: Some(m.version.clone()),
            latest: None,
            state: State::Local,
            manifest: Some(m.clone()),
        });
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    Open(PathBuf),
    Toast(String),
    Error(String),
    Quiet,
}

pub fn parse_reply(out: &str) -> Reply {
    let Some(line) = out.lines().map(str::trim).rfind(|l| !l.is_empty()) else {
        return Reply::Quiet;
    };
    let Ok(Value::Object(o)) = serde_json::from_str::<Value>(line) else {
        return Reply::Error("The add-on gave an answer SimpleClip could not read".into());
    };
    let text = |k: &str| {
        o.get(k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    if let Some(e) = text("error") {
        return Reply::Error(e);
    }
    if let Some(p) = text("open") {
        return Reply::Open(PathBuf::from(p));
    }
    text("toast").map_or(Reply::Quiet, Reply::Toast)
}

pub fn glyph(name: &str) -> &'static str {
    match name {
        "dice" | "shuffle" | "random" => icon::SHUFFLE,
        "camera" | "screenshot" => icon::CAMERA,
        "folder" => icon::FOLDER,
        "cut" | "scissors" | "trim" => icon::CUT,
        "save" => icon::SAVE,
        "search" => icon::SEARCH,
        "refresh" => icon::REFRESH,
        "tag" | "bookmark" => icon::TAG,
        "share" => icon::SHARE,
        "copy" => icon::COPY,
        "clips" | "film" => icon::CLIPS,
        "add" | "plus" => icon::ADD,
        _ => icon::PLAY,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Home {
    pub addon: String,
    pub exe: PathBuf,
    pub button: Button,
}

fn plain_exe(name: &str) -> bool {
    Path::new(name).file_name().and_then(|n| n.to_str()) == Some(name) && !name.contains("..")
}

pub fn home_buttons(
    have: &[(PathBuf, Manifest)],
    values: &HashMap<String, Map<String, Value>>,
) -> Vec<Home> {
    have.iter()
        .filter(|(_, m)| values.get(&m.name).is_none_or(store::is_enabled))
        .filter(|(_, m)| plain_exe(&m.exe_name()))
        .flat_map(|(dir, m)| {
            m.buttons
                .iter()
                .filter(|b| b.place == "home" && !b.id.is_empty() && !b.label.is_empty())
                .map(|b| Home {
                    addon: m.name.clone(),
                    exe: dir.join(m.exe_name()),
                    button: b.clone(),
                })
        })
        .collect()
}

pub fn run_button(exe: &Path, id: &str, clips: &Path, wait: Duration) -> Reply {
    let mut cmd = Command::new(exe);
    cmd.arg("--button")
        .arg(id)
        .arg("--clips")
        .arg(clips)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    let Ok(mut child) = cmd.spawn() else {
        return Reply::Error("The add-on could not start".into());
    };
    let mut out = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_string(&mut s);
        }
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Reply::Error("The add-on took too long to answer".into());
            }
        }
    }
    parse_reply(&reader.join().unwrap_or_default())
}

#[derive(Debug, PartialEq)]
pub enum Finished {
    Installed {
        id: String,
        title: String,
        version: String,
        update: bool,
    },
    Removed {
        id: String,
        title: String,
    },
    Failed(String),
}

#[derive(Debug, PartialEq)]
pub enum Busy {
    Working {
        label: &'static str,
        percent: Option<u32>,
    },
}

struct Work {
    id: String,
    title: String,
    update: bool,
    remove: bool,
    label: Arc<Mutex<&'static str>>,
    done: Slot<Result<String, String>>,
    part: PathBuf,
    size: u64,
}

#[derive(Default)]
pub struct Shop {
    pub asking: bool,
    list: Option<Result<Vec<Entry>, String>>,
    fetch: Option<Slot<Result<Vec<Entry>, String>>>,
    have: Vec<(PathBuf, Manifest)>,
    staged: HashSet<String>,
    values: HashMap<String, Map<String, Value>>,
    dirty: HashSet<String>,
    edited: Option<Instant>,
    status: HashMap<String, Status>,
    events: Option<EventsStatus>,
    work: Option<Work>,
    press: Option<Slot<Reply>>,
    refreshed: Option<Instant>,
    frozen: bool,
}

impl Shop {
    pub fn open(&mut self, ctx: &egui::Context) {
        if self.list.is_none() && self.fetch.is_none() {
            self.reload(ctx);
        }
    }

    pub fn reload(&mut self, ctx: &egui::Context) {
        if self.frozen || self.fetch.is_some() {
            return;
        }
        let s = slot();
        self.fetch = Some(s.clone());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            put(&s, store::fetch_catalog().map(|c| c.addons), &ctx);
        });
    }

    pub fn fetching(&self) -> bool {
        self.fetch.is_some()
    }

    pub fn list(&self) -> Option<&Result<Vec<Entry>, String>> {
        self.list.as_ref()
    }

    pub fn refresh(&mut self) {
        if let Some(got) = self.fetch.as_ref().and_then(take) {
            self.list = Some(got);
            self.fetch = None;
        }
        if self.frozen || self.refreshed.is_some_and(|t| t.elapsed() < REFRESH_EVERY) {
            return;
        }
        self.refreshed = Some(Instant::now());
        let Some(root) = store::root() else {
            return;
        };
        self.load(&root);
    }

    fn load(&mut self, root: &Path) {
        self.have = store::installed(root);
        let mut ids: Vec<String> = self.have.iter().map(|(_, m)| m.name.clone()).collect();
        if let Some(Ok(list)) = &self.list {
            ids.extend(list.iter().map(|e| e.id.clone()));
        }
        self.staged = ids
            .into_iter()
            .filter(|id| store::staged_of(root, id).exists())
            .collect();
        self.values
            .retain(|id, _| self.have.iter().any(|(_, m)| &m.name == id));
        self.status.clear();
        self.events = None;
        for (dir, m) in &self.have {
            if !self.dirty.contains(&m.name) {
                self.values
                    .insert(m.name.clone(), store::read_settings(dir));
            }
            if m.name == EVENTS {
                self.events = legacy::read_status(dir);
            } else if let Some(s) = store::read_status(dir) {
                self.status.insert(m.name.clone(), s);
            }
        }
    }

    pub fn flush(&mut self) -> Option<String> {
        self.save_due(SAVE_AFTER)
    }

    fn save_due(&mut self, after: Duration) -> Option<String> {
        if self.frozen {
            self.dirty.clear();
            return None;
        }
        if self.dirty.is_empty() || self.edited.is_some_and(|t| t.elapsed() < after) {
            return None;
        }
        let mut err = None;
        for id in std::mem::take(&mut self.dirty) {
            let dir = self.have.iter().find(|(_, m)| m.name == id).map(|(d, _)| d);
            if let (Some(dir), Some(v)) = (dir, self.values.get(&id)) {
                if let Err(e) = store::write_settings(dir, v) {
                    err = Some(format!("couldn’t save the add-on settings: {e}"));
                }
            }
        }
        err
    }

    pub fn cards(&self, app: &str) -> Vec<Card> {
        let list = match &self.list {
            Some(Ok(l)) => l.as_slice(),
            _ => &[],
        };
        cards(list, &self.have, app)
    }

    pub fn values_mut(&mut self, id: &str) -> &mut Map<String, Value> {
        self.values.entry(id.to_string()).or_default()
    }

    pub fn touch(&mut self, id: &str) {
        self.dirty.insert(id.to_string());
        self.edited = Some(Instant::now());
    }

    pub fn sync_events(&mut self, on: bool) {
        if !self.have.iter().any(|(_, m)| m.name == EVENTS) {
            return;
        }
        let v = self.values_mut(EVENTS);
        if v.get(store::ENABLED).and_then(Value::as_bool) != Some(on) {
            v.insert(store::ENABLED.into(), Value::Bool(on));
            self.touch(EVENTS);
        }
    }

    pub fn staged(&self, id: &str) -> bool {
        self.staged.contains(id)
    }

    pub fn status_line(&self, id: &str) -> Option<&str> {
        self.status
            .get(id)
            .filter(|s| self.frozen || legacy::now_secs().saturating_sub(s.updated) <= 8)
            .map(|s| s.line.as_str())
            .filter(|l| !l.trim().is_empty())
    }

    pub fn events_installed(&self) -> bool {
        self.have.iter().any(|(_, m)| m.name == EVENTS)
    }

    pub fn events_status(&self) -> Option<&EventsStatus> {
        self.events
            .as_ref()
            .filter(|s| self.frozen || legacy::status_is_fresh(s, legacy::now_secs()))
    }

    pub fn busy(&self) -> bool {
        self.work.is_some()
    }

    pub fn busy_for(&self, id: &str) -> Option<Busy> {
        let w = self.work.as_ref().filter(|w| w.id == id)?;
        let label = w.label.lock().map(|l| *l).unwrap_or("Working");
        let percent = (label == "Downloading" && w.size > 0)
            .then(|| std::fs::metadata(&w.part).ok())
            .flatten()
            .map(|m| ((m.len() * 100) / w.size).min(100) as u32);
        Some(Busy::Working { label, percent })
    }

    pub fn install(&mut self, id: &str, running: bool, ctx: &egui::Context) -> Result<(), String> {
        if self.work.is_some() || self.frozen {
            return Ok(());
        }
        let entry = match &self.list {
            Some(Ok(l)) => l.iter().find(|e| e.id == id).cloned(),
            _ => None,
        }
        .ok_or("The add-on list isn’t loaded yet. Try again in a moment")?;
        let root = store::root().ok_or("couldn’t find the SimpleClip data folder")?;
        let card = self
            .cards(env!("CARGO_PKG_VERSION"))
            .into_iter()
            .find(|c| c.id == id);
        let label = Arc::new(Mutex::new("Downloading"));
        let done = slot();
        self.work = Some(Work {
            id: id.into(),
            title: card
                .as_ref()
                .map_or_else(|| title_of(id), |c| c.title.clone()),
            update: card.is_some_and(|c| c.installed()),
            remove: false,
            label: label.clone(),
            done: done.clone(),
            part: sc_core::store::part_of(&root, &entry),
            size: entry.size,
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let got = install(&root, &entry, running, &label, &ctx);
            put(&done, got, &ctx);
        });
        Ok(())
    }

    pub fn remove(&mut self, id: &str, ctx: &egui::Context) {
        if self.work.is_some() || self.frozen {
            return;
        }
        let Some((dir, m)) = self.have.iter().find(|(_, m)| m.name == id).cloned() else {
            return;
        };
        let Some(root) = dir
            .parent()
            .filter(|r| !r.as_os_str().is_empty())
            .map(Path::to_path_buf)
        else {
            return;
        };
        let label = Arc::new(Mutex::new("Removing"));
        let done = slot();
        self.dirty.remove(id);
        self.work = Some(Work {
            id: id.into(),
            title: m.title(),
            update: false,
            remove: true,
            label,
            done: done.clone(),
            part: PathBuf::new(),
            size: 0,
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let got = remove_in(&root, &m, 24, Duration::from_millis(500)).map(|_| String::new());
            put(&done, got, &ctx);
        });
    }

    pub fn poll(&mut self) -> Option<Finished> {
        let got = take(&self.work.as_ref()?.done)?;
        let w = self.work.take()?;
        self.refreshed = None;
        self.refresh();
        Some(match got {
            Err(e) => Finished::Failed(e),
            Ok(_) if w.remove => Finished::Removed {
                id: w.id,
                title: w.title,
            },
            Ok(version) => Finished::Installed {
                id: w.id,
                title: w.title,
                version,
                update: w.update,
            },
        })
    }

    pub fn home_buttons(&self) -> Vec<Home> {
        home_buttons(&self.have, &self.values)
    }

    pub fn pressing(&self) -> bool {
        self.press.is_some()
    }

    pub fn press(&mut self, h: &Home, clips: PathBuf, ctx: &egui::Context) {
        if self.press.is_some() || self.frozen {
            return;
        }
        let s = slot();
        self.press = Some(s.clone());
        let (exe, id, ctx) = (h.exe.clone(), h.button.id.clone(), ctx.clone());
        std::thread::spawn(move || {
            put(&s, run_button(&exe, &id, &clips, BUTTON_WAIT), &ctx);
        });
    }

    pub fn answer(&mut self) -> Option<Reply> {
        let got = take(self.press.as_ref()?)?;
        self.press = None;
        Some(got)
    }

    pub fn demo(&mut self, events: bool) {
        *self = Self {
            frozen: true,
            ..Default::default()
        };
        let app = env!("CARGO_PKG_VERSION").to_string();
        let entry = |id: &str, title: &str, about: &str, version: &str| Entry {
            id: id.into(),
            title: title.into(),
            about: about.into(),
            version: version.into(),
            ..Default::default()
        };
        self.list = Some(Ok(vec![
            entry(
                EVENTS,
                "Game events",
                "Saves a clip on its own after a multi-kill. Experimental.",
                &app,
            ),
            entry(
                "voice",
                "Clip that",
                "Say \u{201C}simple clip that\u{201D} and it saves a clip. Works offline with Windows speech recognition.",
                "1.0.0",
            ),
            entry(
                "roulette",
                "Clip roulette",
                "A button on Home that plays a random old clip. Nostalgia or trauma.",
                "1.0.0",
            ),
        ]));
        let demo = PathBuf::from("demo");
        if events {
            self.have.push((
                demo.join(EVENTS),
                Manifest {
                    name: EVENTS.into(),
                    version: app.clone(),
                    title: "Game events".into(),
                    exe: legacy::EVENTS_EXE.into(),
                    background: true,
                    ..Default::default()
                },
            ));
        }
        self.have.push((
            demo.join("roulette"),
            Manifest {
                name: "roulette".into(),
                version: "1.0.0".into(),
                title: "Clip roulette".into(),
                exe: "sc-roulette.exe".into(),
                buttons: vec![Button {
                    id: "spin".into(),
                    label: "Clip roulette".into(),
                    place: "home".into(),
                    icon: "dice".into(),
                    hint: "Play a random old clip".into(),
                }],
                settings: vec![store::Setting {
                    key: "old-first".into(),
                    label: "Favour old clips".into(),
                    hint: "Clips older than a week come up three times as often".into(),
                    kind: store::Kind::Toggle { default: true },
                }],
                ..Default::default()
            },
        ));
        self.events = Some(EventsStatus {
            version: app.clone(),
            cs2: "not-found".into(),
            league: "waiting".into(),
            r6: "waiting".into(),
            dota2: "not-found".into(),
            last_event: None,
            updated: legacy::now_secs(),
        });
    }

    #[cfg(test)]
    pub fn pretend(
        &mut self,
        list: Vec<Entry>,
        have: Vec<(PathBuf, Manifest)>,
        events: Option<EventsStatus>,
    ) {
        self.list = Some(Ok(list));
        self.have = have;
        self.events = events;
    }

    #[cfg(test)]
    pub fn pretend_status(&mut self, id: &str, line: &str, updated: u64) {
        self.status.insert(
            id.into(),
            Status {
                line: line.into(),
                updated,
            },
        );
    }

    #[cfg(test)]
    pub fn values(&self, id: &str) -> Option<&Map<String, Value>> {
        self.values.get(id)
    }
}

fn install(
    root: &Path,
    entry: &Entry,
    running: bool,
    label: &Mutex<&'static str>,
    ctx: &egui::Context,
) -> Result<String, String> {
    let m = store::stage(root, entry)?;
    if let Ok(mut l) = label.lock() {
        *l = "Installing";
    }
    ctx.request_repaint();
    if running && m.background {
        for _ in 0..40 {
            if !store::staged_of(root, &entry.id).exists() {
                return Ok(m.version);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    store::apply(root, &entry.id)?;
    Ok(m.version)
}

fn uninstall(exe: &Path, dir: &Path) {
    let Ok(mut child) = Command::new(exe)
        .arg("--uninstall")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(NO_WINDOW)
        .spawn()
    else {
        return;
    };
    let start = Instant::now();
    while start.elapsed() < UNINSTALL_WAIT {
        if !matches!(child.try_wait(), Ok(None)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn retry(attempts: u32, pause: Duration, mut done: impl FnMut() -> bool) -> bool {
    for attempt in 0..attempts {
        if done() {
            return true;
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    false
}

pub fn remove_in(root: &Path, m: &Manifest, attempts: u32, pause: Duration) -> Result<(), String> {
    let busy = || "the add-on is still running. Try again in a moment".to_string();
    let dir = store::dir_of(root, &m.name);
    if m.background && dir.exists() {
        let mut v = store::read_settings(&dir);
        v.insert(store::ENABLED.into(), Value::Bool(false));
        let _ = store::write_settings(&dir, &v);
    }
    let exe = dir.join(m.exe_name());
    let free = || !exe.exists() || std::fs::OpenOptions::new().write(true).open(&exe).is_ok();
    if !retry(attempts, pause, free) {
        return Err(busy());
    }
    if exe.exists() {
        uninstall(&exe, &dir);
    }
    let _ = std::fs::remove_dir_all(store::staged_of(root, &m.name));
    if retry(attempts, pause, || {
        !dir.exists() || std::fs::remove_dir_all(&dir).is_ok()
    }) {
        Ok(())
    } else {
        Err(busy())
    }
}

pub fn cs2_text(status: Option<&EventsStatus>, wanted: bool) -> String {
    if !wanted {
        return "Off".to_string();
    }
    match status.map(|s| s.cs2.as_str()) {
        None => "Starting…".to_string(),
        Some("connected") => "Connected to the game".to_string(),
        Some("waiting") => {
            "Ready. Start the game, or restart it once if it was already open".to_string()
        }
        Some("not-found") => "Not found in your Steam folders".to_string(),
        Some("port-busy") => "Another program is using the port it needs".to_string(),
        Some(_) => "Off".to_string(),
    }
}

pub fn dota_text(status: Option<&EventsStatus>, wanted: bool) -> String {
    if !wanted {
        return "Off".to_string();
    }
    match status.map(|s| s.dota2.as_str()) {
        None => "Starting…".to_string(),
        Some("connected") => "Connected to the game".to_string(),
        Some("waiting") => {
            "Ready. Start the game, or restart it once if it was already open".to_string()
        }
        Some("not-found") => "Not found in your Steam folders".to_string(),
        Some("port-busy") => "Another program is using the port it needs".to_string(),
        Some(_) => "Off".to_string(),
    }
}

pub fn r6_text(status: Option<&EventsStatus>, wanted: bool) -> String {
    if !wanted {
        return "Off".to_string();
    }
    match status.map(|s| s.r6.as_str()) {
        None => "Starting…".to_string(),
        Some("watching") => "Watching the kill feed".to_string(),
        Some("waiting") => "Waiting for the game. Reads the kill feed on screen".to_string(),
        Some("ocr-missing") => {
            "Windows text recognition is not installed. Add an English language pack".to_string()
        }
        Some(_) => "Off".to_string(),
    }
}

pub fn league_text(status: Option<&EventsStatus>, wanted: bool) -> String {
    if !wanted {
        return "Off".to_string();
    }
    match status.map(|s| s.league.as_str()) {
        None => "Starting…".to_string(),
        Some("in-game") => "In a match".to_string(),
        Some("waiting") => "Waiting for a match".to_string(),
        Some(_) => "Off".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sc-gui-addon-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(id: &str, version: &str, min_app: &str) -> Entry {
        Entry {
            id: id.into(),
            title: format!("The {id}"),
            version: version.into(),
            min_app: min_app.into(),
            ..Default::default()
        }
    }

    fn have(id: &str, version: &str) -> (PathBuf, Manifest) {
        (
            PathBuf::from(id),
            Manifest {
                name: id.into(),
                version: version.into(),
                exe: format!("sc-{id}.exe"),
                ..Default::default()
            },
        )
    }

    fn state_of(cards: &[Card], id: &str) -> State {
        cards.iter().find(|c| c.id == id).unwrap().state.clone()
    }

    #[test]
    fn each_card_offers_the_right_action() {
        let list = vec![
            entry("voice", "1.0.0", ""),
            entry("roulette", "1.2.0", ""),
            entry("same", "1.0.0", ""),
            entry("future", "1.0.0", "99.0.0"),
            entry("stuck", "2.0.0", "99.0.0"),
        ];
        let got = vec![
            have("roulette", "1.0.0"),
            have("same", "1.0.0"),
            have("stuck", "1.0.0"),
            have("handmade", "0.1.0"),
        ];
        let c = cards(&list, &got, "6.9.0");
        assert_eq!(state_of(&c, "voice"), State::Get);
        assert_eq!(state_of(&c, "roulette"), State::Update);
        assert_eq!(state_of(&c, "same"), State::Current);
        assert_eq!(state_of(&c, "future"), State::Needs("99.0.0".into()));
        assert_eq!(state_of(&c, "stuck"), State::Needs("99.0.0".into()));
        assert_eq!(state_of(&c, "handmade"), State::Local);
        assert_eq!(c.len(), 6);
        assert_eq!(c.last().unwrap().id, "handmade");
        let roulette = c.iter().find(|c| c.id == "roulette").unwrap();
        assert_eq!(
            roulette.version_line(),
            "Version 1.0.0 installed. 1.2.0 is available"
        );
        assert_eq!(roulette.title, "The roulette");
        assert!(roulette.installed());
        let voice = c.iter().find(|c| c.id == "voice").unwrap();
        assert_eq!(voice.version_line(), "Version 1.0.0");
        assert!(!voice.installed());
    }

    #[test]
    fn an_old_events_copy_counts_as_outdated_and_keeps_its_name() {
        let mut list = vec![entry(EVENTS, "1.0.0", "")];
        list[0].title.clear();
        let old = (
            PathBuf::from(EVENTS),
            Manifest {
                name: EVENTS.into(),
                version: "6.9.0".into(),
                ..Default::default()
            },
        );
        let c = cards(&list, &[old], "6.9.0");
        assert_eq!(c[0].state, State::Update);
        assert_eq!(c[0].title, "Game events");
    }

    #[test]
    fn without_a_list_installed_add_ons_still_show() {
        let c = cards(&[], &[have("voice", "1.0.0")], "6.9.0");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].state, State::Local);
        assert_eq!(c[0].version_line(), "Version 1.0.0");
    }

    #[test]
    fn the_last_line_of_a_button_answer_counts() {
        assert_eq!(
            parse_reply("starting\n{\"open\":\"C:\\\\clips\\\\a.mp4\"}\n\n"),
            Reply::Open(PathBuf::from("C:\\clips\\a.mp4"))
        );
        assert_eq!(
            parse_reply("{\"error\":\"old\"}\n{\"toast\":\"Nice\"}"),
            Reply::Toast("Nice".into())
        );
        assert_eq!(
            parse_reply("{\"error\":\"No clips yet\"}"),
            Reply::Error("No clips yet".into())
        );
        assert_eq!(parse_reply(""), Reply::Quiet);
        assert_eq!(parse_reply("  \n  "), Reply::Quiet);
        assert_eq!(parse_reply("{\"other\":1}"), Reply::Quiet);
        assert_eq!(parse_reply("{\"open\":\"\"}"), Reply::Quiet);
        assert!(matches!(parse_reply("done!"), Reply::Error(_)));
        assert!(matches!(parse_reply("[1,2]"), Reply::Error(_)));
    }

    #[test]
    fn icons_map_to_known_glyphs() {
        assert_eq!(glyph("dice"), icon::SHUFFLE);
        assert_eq!(glyph("camera"), icon::CAMERA);
        assert_eq!(glyph("something-new"), icon::PLAY);
    }

    #[test]
    fn only_enabled_add_ons_put_buttons_on_home() {
        let button = |id: &str, place: &str| Button {
            id: id.into(),
            label: "Go".into(),
            place: place.into(),
            ..Default::default()
        };
        let mut a = have("roulette", "1.0.0");
        a.1.buttons = vec![button("spin", "home"), button("x", "clips")];
        let mut b = have("voice", "1.0.0");
        b.1.buttons = vec![button("say", "home")];
        let mut c = have("sneaky", "1.0.0");
        c.1.exe = "..\\evil.exe".into();
        c.1.buttons = vec![button("go", "home")];
        let mut values = HashMap::new();
        let mut off = Map::new();
        off.insert(store::ENABLED.into(), Value::Bool(false));
        values.insert("voice".to_string(), off);
        let got = home_buttons(&[a, b, c], &values);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].button.id, "spin");
        assert_eq!(
            got[0].exe,
            PathBuf::from("roulette").join("sc-roulette.exe")
        );
    }

    #[test]
    fn a_program_that_will_not_start_says_so() {
        let dir = temp("nostart");
        let got = run_button(
            &dir.join("missing.exe"),
            "spin",
            &dir,
            Duration::from_secs(1),
        );
        assert!(matches!(got, Reply::Error(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_turns_it_off_and_deletes_the_folder() {
        let root = temp("remove");
        let dir = root.join("voice");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sc-voice.exe"), b"x").unwrap();
        std::fs::create_dir_all(store::staged_of(&root, "voice")).unwrap();
        let m = Manifest {
            name: "voice".into(),
            version: "1.0.0".into(),
            exe: "sc-voice.exe".into(),
            background: true,
            ..Default::default()
        };
        remove_in(&root, &m, 3, Duration::from_millis(10)).unwrap();
        assert!(!dir.exists());
        assert!(!store::staged_of(&root, "voice").exists());
        remove_in(&root, &m, 1, Duration::from_millis(10)).unwrap();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edited_settings_are_written_to_the_add_on_folder() {
        let root = temp("save");
        let dir = root.join("roulette");
        std::fs::create_dir_all(&dir).unwrap();
        let mut shop = Shop::default();
        shop.pretend(
            Vec::new(),
            vec![(dir.clone(), have("roulette", "1.0.0").1)],
            None,
        );
        shop.values_mut("roulette")
            .insert("old-first".into(), Value::Bool(false));
        shop.touch("roulette");
        assert_eq!(shop.save_due(Duration::from_secs(60)), None);
        assert!(!dir.join(store::SETTINGS).exists());
        assert_eq!(shop.save_due(Duration::ZERO), None);
        assert_eq!(
            store::read_settings(&dir).get("old-first"),
            Some(&Value::Bool(false))
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_events_switch_is_copied_into_its_settings() {
        let mut shop = Shop::default();
        shop.sync_events(false);
        assert!(shop.values(EVENTS).is_none());
        shop.pretend(Vec::new(), vec![have(EVENTS, "1.0.0")], None);
        shop.sync_events(false);
        assert_eq!(
            shop.values(EVENTS).unwrap().get(store::ENABLED),
            Some(&Value::Bool(false))
        );
        assert!(shop.dirty.contains(EVENTS));
    }

    #[test]
    fn the_demo_never_saves_or_installs() {
        let ctx = egui::Context::default();
        let mut shop = Shop::default();
        shop.demo(true);
        assert!(shop.events_installed());
        assert_eq!(shop.home_buttons().len(), 1);
        shop.touch("roulette");
        assert_eq!(shop.save_due(Duration::ZERO), None);
        shop.install("voice", false, &ctx).unwrap();
        shop.remove("roulette", &ctx);
        assert!(!shop.busy());
        shop.reload(&ctx);
        assert!(!shop.fetching());
        let c = shop.cards(env!("CARGO_PKG_VERSION"));
        assert_eq!(state_of(&c, "voice"), State::Get);
        assert_eq!(state_of(&c, "roulette"), State::Current);
        assert_eq!(state_of(&c, EVENTS), State::Current);
    }

    #[test]
    fn a_job_reports_progress_and_hands_back_once() {
        let dir = temp("job");
        let ctx = egui::Context::default();
        let part = dir.join("voice.zip.part");
        std::fs::write(&part, vec![0u8; 50]).unwrap();
        let mut shop = Shop::default();
        let done = slot();
        shop.work = Some(Work {
            id: "voice".into(),
            title: "Clip that".into(),
            update: false,
            remove: false,
            label: Arc::new(Mutex::new("Downloading")),
            done: done.clone(),
            part,
            size: 200,
        });
        assert_eq!(
            shop.busy_for("voice"),
            Some(Busy::Working {
                label: "Downloading",
                percent: Some(25)
            })
        );
        assert_eq!(shop.busy_for("other"), None);
        assert_eq!(shop.poll(), None);
        shop.frozen = true;
        put(&done, Ok("1.0.0".to_string()), &ctx);
        assert_eq!(
            shop.poll(),
            Some(Finished::Installed {
                id: "voice".into(),
                title: "Clip that".into(),
                version: "1.0.0".into(),
                update: false
            })
        );
        assert_eq!(shop.poll(), None);
        assert!(!shop.busy());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_status_lines_read_plainly() {
        let mut s = EventsStatus {
            cs2: "connected".into(),
            league: "in-game".into(),
            ..Default::default()
        };
        assert_eq!(cs2_text(Some(&s), true), "Connected to the game");
        assert_eq!(league_text(Some(&s), true), "In a match");
        assert_eq!(cs2_text(Some(&s), false), "Off");
        assert_eq!(cs2_text(None, true), "Starting…");
        s.cs2 = "not-found".into();
        assert!(cs2_text(Some(&s), true).contains("Not found"));
        s.cs2 = "waiting".into();
        assert!(cs2_text(Some(&s), true).contains("restart it once"));
        s.cs2 = "port-busy".into();
        assert!(cs2_text(Some(&s), true).contains("port"));
        s.league = "waiting".into();
        assert_eq!(league_text(Some(&s), true), "Waiting for a match");
    }

    #[test]
    fn the_newer_games_read_plainly_too() {
        let mut s = EventsStatus {
            r6: "watching".into(),
            dota2: "connected".into(),
            ..Default::default()
        };
        assert_eq!(r6_text(Some(&s), true), "Watching the kill feed");
        assert_eq!(dota_text(Some(&s), true), "Connected to the game");
        assert_eq!(r6_text(Some(&s), false), "Off");
        assert_eq!(dota_text(None, true), "Starting…");
        s.r6 = "ocr-missing".into();
        assert!(r6_text(Some(&s), true).contains("language pack"));
        s.r6 = "waiting".into();
        assert!(r6_text(Some(&s), true).contains("Waiting for the game"));
        s.dota2 = "not-found".into();
        assert!(dota_text(Some(&s), true).contains("Not found"));
    }
}
