use sc_core::store::{self, Saved};
use sc_keys::burn::{self, Outcome};
use sc_keys::settings::{self, Settings};
use sc_keys::tape;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const LOOK: Duration = Duration::from_millis(500);
const SHOW_FOR: Duration = Duration::from_secs(12);
const NO_WINDOW: u32 = 0x0800_0000;
const BELOW_NORMAL: u32 = 0x0000_4000;

static NOTE: Mutex<Option<(String, Option<Instant>)>> = Mutex::new(None);

fn say(line: &str, keep: bool) {
    let until = (!keep).then(|| Instant::now() + SHOW_FOR);
    if let Ok(mut n) = NOTE.lock() {
        *n = Some((line.to_string(), until));
    }
}

pub fn note() -> Option<String> {
    let mut n = NOTE.lock().ok()?;
    match n.as_ref() {
        Some((line, None)) => Some(line.clone()),
        Some((line, Some(until))) if Instant::now() < *until => Some(line.clone()),
        _ => {
            *n = None;
            None
        }
    }
}

pub fn fresh(list: &[Saved], seen: u64) -> Vec<Saved> {
    let mut out: Vec<Saved> = list.iter().filter(|s| s.end_ms > seen).cloned().collect();
    out.sort_by_key(|s| s.end_ms);
    out
}

fn name_of(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "the clip".into())
}

fn helper(dir: &Path, clip: &Path, start: f64, events: &[tape::Event]) -> Result<Outcome, String> {
    use std::os::windows::process::CommandExt;
    let work = dir.join("work");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let list = work.join("events.txt");
    std::fs::write(&list, tape::encode(events)).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let out = std::process::Command::new(exe)
        .arg("--dir")
        .arg(dir)
        .arg("--burn")
        .arg(clip)
        .arg("--start")
        .arg(format!("{start:.4}"))
        .arg("--events")
        .arg(&list)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(NO_WINDOW | BELOW_NORMAL)
        .output();
    let _ = std::fs::remove_file(&list);
    let out = out.map_err(|e| format!("the helper could not start: {e}"))?;
    burn::read_report(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .last()
            .unwrap_or_default(),
    )
}

fn handle(dir: &Path, saved: &Saved) {
    let s = Settings::read(&settings::manifest(), &store::read_settings(dir));
    if !s.enabled || !saved.path.exists() {
        return;
    }
    let start = saved.start_ms as f64 / 1000.0;
    let end = saved.end_ms as f64 / 1000.0;
    let events = tape::recorded(start, end);
    if !tape::pressed_during(&events, start, end) {
        tracing::info!("no keys pressed during the clip, left as it is");
        return;
    }
    say("Adding your keys to the clip", true);
    let took = Instant::now();
    let name = name_of(&saved.path);
    match helper(dir, &saved.path, start, &events) {
        Ok(Outcome::Added) => {
            tracing::info!(secs = took.elapsed().as_secs_f32(), "keys added");
            say(&format!("Added your keys to {name}"), false);
        }
        Ok(Outcome::Beside(p)) => {
            tracing::info!("the clip was busy, saved a copy with keys");
            say(
                &format!("{name} was open, saved {} with your keys", name_of(&p)),
                false,
            );
        }
        Ok(Outcome::Quiet) => {
            if let Ok(mut n) = NOTE.lock() {
                *n = None;
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not add the keys");
            say(&format!("Could not add your keys to {name}: {e}"), false);
        }
    }
}

pub fn spawn(dir: PathBuf) {
    let Some(root) = dir.parent().map(Path::to_path_buf) else {
        return;
    };
    let _ = std::fs::remove_dir_all(dir.join("work"));
    let _ = std::thread::Builder::new()
        .name("clips".into())
        .spawn(move || {
            let mut seen = store::read_saves(&root)
                .iter()
                .map(|s| s.end_ms)
                .max()
                .unwrap_or(0);
            loop {
                std::thread::sleep(LOOK);
                for saved in fresh(&store::read_saves(&root), seen) {
                    seen = seen.max(saved.end_ms);
                    handle(&dir, &saved);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(end_ms: u64) -> Saved {
        Saved {
            path: PathBuf::from(format!("{end_ms}.mp4")),
            start_ms: end_ms.saturating_sub(30_000),
            end_ms,
        }
    }

    #[test]
    fn only_clips_saved_after_the_last_one_seen_are_picked_up() {
        let list = [saved(5_000), saved(9_000), saved(7_000)];
        let got: Vec<u64> = fresh(&list, 5_000).iter().map(|s| s.end_ms).collect();
        assert_eq!(got, [7_000, 9_000]);
        assert!(fresh(&list, 9_000).is_empty());
    }

    #[test]
    fn notes_last_a_while_and_busy_ones_stay() {
        say("Adding your keys to the clip", true);
        assert_eq!(note().as_deref(), Some("Adding your keys to the clip"));
        say("Added", false);
        assert_eq!(note().as_deref(), Some("Added"));
        *NOTE.lock().unwrap() = Some(("old".into(), Some(Instant::now())));
        assert_eq!(note(), None);
    }
}
