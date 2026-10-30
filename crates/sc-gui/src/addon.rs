use sc_core::addon::{self, EventsStatus};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const NO_WINDOW: u32 = 0x0800_0000;
const REFRESH_EVERY: Duration = Duration::from_millis(1500);

#[derive(Clone, Debug, PartialEq)]
enum JobState {
    Running(&'static str),
    Done(Option<String>),
    Failed(String),
}

pub struct Job {
    state: Mutex<JobState>,
    pub progress: AtomicU64,
    pub total: AtomicU64,
}

impl Job {
    fn new(label: &'static str) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(JobState::Running(label)),
            progress: AtomicU64::new(0),
            total: AtomicU64::new(0),
        })
    }

    fn set(&self, state: JobState, ctx: &egui::Context) {
        if let Ok(mut s) = self.state.lock() {
            *s = state;
        }
        ctx.request_repaint();
    }

    fn get(&self) -> JobState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(JobState::Failed("the add-on task stopped".into()))
    }
}

#[derive(Debug, PartialEq)]
pub enum Finished {
    Installed(String),
    Removed,
    Failed(String),
}

#[derive(Debug, PartialEq)]
pub enum Busy {
    Working {
        label: &'static str,
        percent: Option<u32>,
    },
}

#[derive(Default)]
pub struct Events {
    pub asking: bool,
    job: Option<(Arc<Job>, bool)>,
    installed: Option<String>,
    status: Option<EventsStatus>,
    refreshed: Option<Instant>,
    frozen: bool,
}

impl Events {
    pub fn refresh(&mut self) {
        if self.frozen {
            return;
        }
        if self.refreshed.is_some_and(|t| t.elapsed() < REFRESH_EVERY) {
            return;
        }
        self.refreshed = Some(Instant::now());
        let Some(dir) = addon::events_dir() else {
            return;
        };
        self.installed = addon::installed_version(&dir);
        self.status = addon::read_status(&dir);
    }

    pub fn installed(&self) -> Option<&str> {
        self.installed.as_deref()
    }

    pub fn demo(&mut self) {
        self.frozen = true;
        self.installed = Some(env!("CARGO_PKG_VERSION").to_string());
        self.status = Some(EventsStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            cs2: "not-found".into(),
            league: "waiting".into(),
            r6: "waiting".into(),
            dota2: "not-found".into(),
            last_event: None,
            updated: addon::now_secs(),
        });
    }

    #[cfg(test)]
    pub fn pretend(&mut self, version: Option<&str>, status: Option<EventsStatus>) {
        self.installed = version.map(str::to_string);
        self.status = status;
    }

    pub fn status(&self) -> Option<&EventsStatus> {
        self.status
            .as_ref()
            .filter(|s| self.frozen || addon::status_is_fresh(s, addon::now_secs()))
    }

    pub fn busy(&self) -> Option<Busy> {
        let (job, _) = self.job.as_ref()?;
        match job.get() {
            JobState::Running(label) => {
                let total = job.total.load(Ordering::Relaxed);
                let got = job.progress.load(Ordering::Relaxed);
                let percent = (total > 0).then(|| ((got * 100) / total).min(100) as u32);
                Some(Busy::Working { label, percent })
            }
            _ => None,
        }
    }

    pub fn start_install(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        self.asking = false;
        let job = Job::new("Looking for the add-on");
        self.job = Some((job.clone(), true));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let state = match install(&job, &ctx) {
                Ok(version) => JobState::Done(Some(version)),
                Err(e) => JobState::Failed(e),
            };
            job.set(state, &ctx);
        });
    }

    pub fn start_remove(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        let job = Job::new("Removing");
        self.job = Some((job.clone(), false));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let state = match remove() {
                Ok(()) => JobState::Done(None),
                Err(e) => JobState::Failed(e),
            };
            job.set(state, &ctx);
        });
    }

    pub fn poll(&mut self) -> Option<Finished> {
        let (job, installing) = self.job.as_ref()?;
        let installing = *installing;
        let finished = match job.get() {
            JobState::Running(_) => return None,
            JobState::Done(Some(version)) if installing => Finished::Installed(version),
            JobState::Done(_) => Finished::Removed,
            JobState::Failed(e) => Finished::Failed(e),
        };
        self.job = None;
        self.refreshed = None;
        self.refresh();
        Some(finished)
    }
}

fn install(job: &Job, ctx: &egui::Context) -> Result<String, String> {
    let dest = addon::events_dir().ok_or("couldn’t find the SimpleClip data folder")?;
    if let Ok(local) = std::env::var("SC_ADDON_ZIP") {
        job.set(JobState::Running("Installing"), ctx);
        return unpack_and_swap(Path::new(&local), &dest);
    }
    let package = sc_core::update::fetch_addon(addon::EVENTS_ZIP_SUFFIX)?.ok_or(
        "The game events add-on isn’t part of a published release yet. Try again after the next update.",
    )?;
    if package.asset.sha256.is_none() {
        return Err("GitHub gave no checksum for the add-on, so it was not installed".to_string());
    }
    job.total.store(package.asset.size, Ordering::Relaxed);
    job.set(JobState::Running("Downloading"), ctx);
    let zip = std::env::temp_dir().join(format!("SimpleClip-{}-events.zip", package.version));
    crate::update::download_file(&package.asset, &zip, &job.progress, ctx)?;
    job.set(JobState::Running("Installing"), ctx);
    let result = unpack_and_swap(&zip, &dest);
    let _ = std::fs::remove_file(&zip);
    result
}

pub fn unpack_and_swap(zip: &Path, dest: &Path) -> Result<String, String> {
    let parent = dest.parent().ok_or("couldn’t find the add-on folder")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("couldn’t prepare the add-on: {e}"))?;
    let stage = dest.with_file_name("events.new");
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("couldn’t prepare the add-on: {e}"))?;
    let unpacked = std::process::Command::new("tar")
        .arg("-xf")
        .arg(zip)
        .arg("-C")
        .arg(&stage)
        .creation_flags(NO_WINDOW)
        .status();
    if !unpacked.map(|s| s.success()).unwrap_or(false) {
        let _ = std::fs::remove_dir_all(&stage);
        return Err("couldn’t unpack the add-on".to_string());
    }
    let Some(version) = addon::installed_version(&stage) else {
        let _ = std::fs::remove_dir_all(&stage);
        return Err("the add-on package was incomplete, so it was not installed".to_string());
    };
    if dest.exists() && std::fs::remove_dir_all(dest).is_err() {
        let _ = std::fs::remove_dir_all(&stage);
        return Err("the add-on is in use. Turn it off, remove it, then try again".to_string());
    }
    std::fs::rename(&stage, dest).map_err(|e| format!("couldn’t finish installing: {e}"))?;
    Ok(version)
}

fn remove() -> Result<(), String> {
    let dir = addon::events_dir().ok_or("couldn’t find the add-on folder")?;
    remove_in(&dir, 24, Duration::from_millis(500))
}

pub fn remove_in(dir: &Path, attempts: u32, pause: Duration) -> Result<(), String> {
    let exe: PathBuf = dir.join(addon::EVENTS_EXE);
    if exe.exists() {
        let _ = std::process::Command::new(&exe)
            .arg("--uninstall")
            .creation_flags(NO_WINDOW)
            .status();
    }
    for attempt in 0..attempts {
        if !dir.exists() || std::fs::remove_dir_all(dir).is_ok() {
            return Ok(());
        }
        if attempt + 1 < attempts {
            std::thread::sleep(pause);
        }
    }
    Err("the add-on is still running. Try again in a moment".to_string())
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

    fn make_zip(dir: &Path, with_exe: bool, name: &str) -> PathBuf {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let mut files = Vec::new();
        if with_exe {
            std::fs::write(src.join(addon::EVENTS_EXE), b"not really a program").unwrap();
            files.push(addon::EVENTS_EXE.to_string());
        }
        addon::write_manifest(&src, "9.9.9").unwrap();
        let manifest = std::fs::read_to_string(src.join(addon::MANIFEST))
            .unwrap()
            .replace("\"events\"", &format!("\"{name}\""));
        std::fs::write(src.join(addon::MANIFEST), manifest).unwrap();
        files.push(addon::MANIFEST.to_string());
        let zip = dir.join("events.zip");
        let status = std::process::Command::new("tar")
            .args(["-a", "-c", "-f"])
            .arg(&zip)
            .arg("-C")
            .arg(&src)
            .args(&files)
            .creation_flags(NO_WINDOW)
            .status()
            .unwrap();
        assert!(status.success());
        zip
    }

    #[test]
    fn a_good_package_is_installed_and_reports_its_version() {
        let dir = temp("good");
        let zip = make_zip(&dir, true, "events");
        let dest = dir.join("addons").join("events");
        assert_eq!(unpack_and_swap(&zip, &dest).unwrap(), "9.9.9");
        assert_eq!(addon::installed_version(&dest).as_deref(), Some("9.9.9"));
        assert!(!dir.join("addons").join("events.new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installing_over_an_old_copy_replaces_it() {
        let dir = temp("over");
        let zip = make_zip(&dir, true, "events");
        let dest = dir.join("addons").join("events");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("old-file.txt"), b"x").unwrap();
        unpack_and_swap(&zip, &dest).unwrap();
        assert!(!dest.join("old-file.txt").exists());
        assert!(dest.join(addon::EVENTS_EXE).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_without_the_program_is_refused_and_leaves_things_alone() {
        let dir = temp("incomplete");
        let zip = make_zip(&dir, false, "events");
        let dest = dir.join("addons").join("events");
        let err = unpack_and_swap(&zip, &dest).unwrap_err();
        assert!(err.contains("incomplete"), "{err}");
        assert!(!dest.exists());
        assert!(!dir.join("addons").join("events.new").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_package_for_something_else_is_refused() {
        let dir = temp("wrong");
        let zip = make_zip(&dir, true, "voice");
        let dest = dir.join("addons").join("events");
        assert!(unpack_and_swap(&zip, &dest).is_err());
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn something_that_is_not_a_zip_is_refused() {
        let dir = temp("junk");
        let zip = dir.join("events.zip");
        std::fs::write(&zip, b"this is not an archive").unwrap();
        let dest = dir.join("addons").join("events");
        assert!(unpack_and_swap(&zip, &dest).is_err());
        assert!(!dest.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn removing_deletes_the_folder() {
        let dir = temp("remove");
        let folder = dir.join("events");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(addon::EVENTS_EXE), b"x").unwrap();
        std::fs::write(folder.join(addon::MANIFEST), b"{}").unwrap();
        remove_in(&folder, 3, Duration::from_millis(10)).unwrap();
        assert!(!folder.exists());
        remove_in(&folder, 1, Duration::from_millis(10)).unwrap();
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

    #[test]
    fn a_job_reports_progress_as_a_percentage() {
        let mut events = Events::default();
        let job = Job::new("Downloading");
        job.total.store(200, Ordering::Relaxed);
        job.progress.store(50, Ordering::Relaxed);
        events.job = Some((job, true));
        assert_eq!(
            events.busy(),
            Some(Busy::Working {
                label: "Downloading",
                percent: Some(25)
            })
        );
        assert_eq!(events.poll(), None);
    }

    #[test]
    fn finished_jobs_hand_back_what_happened_once() {
        let ctx = egui::Context::default();
        let mut events = Events::default();
        let job = Job::new("Installing");
        job.set(JobState::Done(Some("9.9.9".into())), &ctx);
        events.job = Some((job, true));
        assert_eq!(events.poll(), Some(Finished::Installed("9.9.9".into())));
        assert_eq!(events.poll(), None);
        assert!(events.busy().is_none());

        let job = Job::new("Removing");
        job.set(JobState::Done(None), &ctx);
        events.job = Some((job, false));
        assert_eq!(events.poll(), Some(Finished::Removed));

        let job = Job::new("Installing");
        job.set(JobState::Failed("no network".into()), &ctx);
        events.job = Some((job, true));
        assert_eq!(events.poll(), Some(Finished::Failed("no network".into())));
    }
}
