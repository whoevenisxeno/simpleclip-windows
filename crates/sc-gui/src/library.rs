use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const COLORS: [[u8; 3]; 8] = [
    [0xff, 0x45, 0x3a],
    [0xff, 0x9f, 0x0a],
    [0xff, 0xd6, 0x0a],
    [0x30, 0xd1, 0x58],
    [0x64, 0xd2, 0xff],
    [0x0a, 0x84, 0xff],
    [0xbf, 0x5a, 0xf2],
    [0xff, 0x37, 0x5f],
];

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Bookmark {
    pub id: u32,
    pub name: String,
    pub color: [u8; 3],
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Default, Debug)]
pub struct ClipMeta {
    #[serde(default)]
    pub bookmarks: Vec<u32>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub trimmed: bool,
    #[serde(default)]
    pub compressed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<String>,
}

impl ClipMeta {
    fn is_empty(&self) -> bool {
        self.bookmarks.is_empty()
            && self.note.trim().is_empty()
            && !self.trimmed
            && !self.compressed
            && self.game.is_none()
    }
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Library {
    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,
    #[serde(default)]
    clips: HashMap<String, ClipMeta>,
    #[serde(default)]
    next_id: u32,
    #[serde(skip)]
    path: PathBuf,
    #[serde(skip)]
    dirty_since: Option<std::time::Instant>,
}

fn key(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

impl Library {
    pub fn default_path() -> PathBuf {
        if let Some(p) = std::env::var_os("SC_LIBRARY") {
            return PathBuf::from(p);
        }
        directories::ProjectDirs::from("", "", "SimpleClip")
            .map(|d| d.data_dir().join("library.json"))
            .unwrap_or_else(|| PathBuf::from("library.json"))
    }

    pub fn load() -> Self {
        Self::load_from(Self::default_path())
    }

    pub fn load_from(path: PathBuf) -> Self {
        let mut lib: Library = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        lib.path = path;
        lib
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        self.dirty_since = None;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, &self.path)
    }

    fn touch(&mut self) {
        self.dirty_since.get_or_insert_with(std::time::Instant::now);
    }

    pub fn flush_if_due(&mut self) {
        if self
            .dirty_since
            .is_some_and(|t| t.elapsed() > std::time::Duration::from_millis(500))
        {
            let _ = self.save();
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some()
    }

    pub fn bookmark(&self, id: u32) -> Option<&Bookmark> {
        self.bookmarks.iter().find(|b| b.id == id)
    }

    pub fn add_bookmark(&mut self, name: &str, color: [u8; 3]) -> u32 {
        let id = self
            .bookmarks
            .iter()
            .map(|b| b.id)
            .max()
            .unwrap_or(0)
            .max(self.next_id)
            + 1;
        self.next_id = id;
        self.bookmarks.push(Bookmark {
            id,
            name: name.trim().to_string(),
            color,
        });
        self.touch();
        id
    }

    pub fn edit_bookmark(&mut self, id: u32, name: &str, color: [u8; 3]) {
        if let Some(b) = self.bookmarks.iter_mut().find(|b| b.id == id) {
            if b.name != name || b.color != color {
                b.name = name.to_string();
                b.color = color;
                self.touch();
            }
        }
    }

    pub fn delete_bookmark(&mut self, id: u32) {
        self.bookmarks.retain(|b| b.id != id);
        for m in self.clips.values_mut() {
            m.bookmarks.retain(|b| *b != id);
        }
        self.clips.retain(|_, m| !m.is_empty());
        self.touch();
    }

    pub fn bookmark_uses(&self, id: u32) -> usize {
        self.clips
            .values()
            .filter(|m| m.bookmarks.contains(&id))
            .count()
    }

    pub fn meta(&self, path: &Path) -> Option<&ClipMeta> {
        self.clips.get(&key(path))
    }

    pub fn bookmarks_of(&self, path: &Path) -> Vec<&Bookmark> {
        self.meta(path)
            .map(|m| {
                m.bookmarks
                    .iter()
                    .filter_map(|id| self.bookmark(*id))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn note(&self, path: &Path) -> &str {
        self.meta(path).map(|m| m.note.as_str()).unwrap_or("")
    }

    fn meta_mut(&mut self, path: &Path) -> &mut ClipMeta {
        self.touch();
        self.clips.entry(key(path)).or_default()
    }

    pub fn toggle_bookmark(&mut self, path: &Path, id: u32) {
        let m = self.meta_mut(path);
        if let Some(i) = m.bookmarks.iter().position(|b| *b == id) {
            m.bookmarks.remove(i);
        } else {
            m.bookmarks.push(id);
        }
        self.prune(path);
    }

    pub fn is_trimmed(&self, path: &Path) -> bool {
        self.meta(path).is_some_and(|m| m.trimmed) || has_trim_suffix(path)
    }

    pub fn is_compressed(&self, path: &Path) -> bool {
        self.meta(path).is_some_and(|m| m.compressed) || has_compress_suffix(path)
    }

    pub fn game(&self, path: &Path) -> Option<&str> {
        self.meta(path).and_then(|m| m.game.as_deref())
    }

    pub fn set_game_if_unset(&mut self, path: &Path, game: &str) {
        if self.game(path).is_some() || game.is_empty() {
            return;
        }
        self.meta_mut(path).game = Some(game.to_string());
    }

    pub fn set_trimmed(&mut self, path: &Path, trimmed: bool) {
        if self.meta(path).is_some_and(|m| m.trimmed) == trimmed {
            return;
        }
        self.meta_mut(path).trimmed = trimmed;
        self.prune(path);
    }

    pub fn set_compressed(&mut self, path: &Path, compressed: bool) {
        if self.meta(path).is_some_and(|m| m.compressed) == compressed {
            return;
        }
        self.meta_mut(path).compressed = compressed;
        self.prune(path);
    }

    pub fn set_note(&mut self, path: &Path, note: &str) {
        if self.note(path) == note {
            return;
        }
        self.meta_mut(path).note = note.to_string();
        self.prune(path);
    }

    fn prune(&mut self, path: &Path) {
        if self.clips.get(&key(path)).is_some_and(ClipMeta::is_empty) {
            self.clips.remove(&key(path));
        }
    }

    pub fn rekey(&mut self, old: &Path, new: &Path) {
        if let Some(m) = self.clips.remove(&key(old)) {
            self.clips.insert(key(new), m);
            self.touch();
        }
    }

    pub fn rekey_if_missing(&mut self, old: &Path, new: &Path) {
        if old != new {
            self.rekey(old, new);
        }
    }

    pub fn forget(&mut self, path: &Path) {
        if self.clips.remove(&key(path)).is_some() {
            self.touch();
        }
    }
}

fn has_trim_suffix(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let Some(i) = stem.rfind("-trim") else {
        return false;
    };
    stem[i + 5..].chars().all(|c| c.is_ascii_digit())
}

fn has_compress_suffix(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let is_size = |p: &str| {
        p.strip_suffix("mb")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
    };
    let parts: Vec<&str> = stem.split('-').collect();
    match parts.as_slice() {
        [.., last] if is_size(last) => true,
        [.., size, n] => is_size(size) && !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

pub fn clean_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    trimmed.chars().take(120).collect()
}

fn free_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let make = |n: u32| {
        let file = if n <= 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        dir.join(file)
    };
    (1..).map(make).find(|p| !p.exists()).unwrap()
}

pub fn rename_clip(path: &Path, new_stem: &str) -> Result<PathBuf, String> {
    let stem = clean_name(new_stem);
    if stem.is_empty() {
        return Err("Type a name first.".into());
    }
    let dir = path.parent().ok_or("That clip has no folder.")?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    if path.file_stem().and_then(|s| s.to_str()) == Some(stem.as_str()) {
        return Ok(path.to_path_buf());
    }
    let target = free_path(dir, &stem, ext);
    std::fs::rename(path, &target).map_err(|e| format!("Couldn’t rename: {e}"))?;
    Ok(target)
}

pub fn move_clip(path: &Path, dest_dir: &Path) -> Result<PathBuf, String> {
    if path.parent() == Some(dest_dir) {
        return Ok(path.to_path_buf());
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("Couldn’t create the folder: {e}"))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("clip");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("mp4");
    let target = free_path(dest_dir, stem, ext);
    if std::fs::rename(path, &target).is_err() {
        std::fs::copy(path, &target).map_err(|e| format!("Couldn’t move: {e}"))?;
        std::fs::remove_file(path)
            .map_err(|e| format!("Moved a copy but couldn’t remove the original: {e}"))?;
    }
    Ok(target)
}

pub fn create_folder(root: &Path, name: &str) -> Result<PathBuf, String> {
    let clean = clean_name(name);
    if clean.is_empty() {
        return Err("Type a folder name first.".into());
    }
    let dir = root.join(&clean);
    if dir.exists() {
        return Err(format!(
            "A folder called \u{201C}{clean}\u{201D} already exists."
        ));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn’t create the folder: {e}"))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sc-lib-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn bookmarks_notes_and_rekey_roundtrip() {
        let dir = tmp("roundtrip");
        let file = dir.join("lib.json");
        let mut lib = Library::load_from(file.clone());
        let id = lib.add_bookmark("Clutch", COLORS[3]);
        let clip = dir.join("a.mp4");
        lib.toggle_bookmark(&clip, id);
        lib.set_note(&clip, "ace on B site");
        lib.save().unwrap();

        let mut back = Library::load_from(file);
        assert_eq!(back.bookmarks_of(&clip).len(), 1);
        assert_eq!(back.note(&clip), "ace on B site");

        let moved = dir.join("b.mp4");
        back.rekey(&clip, &moved);
        assert!(back.meta(&clip).is_none());
        assert_eq!(back.bookmarks_of(&moved)[0].name, "Clutch");
    }

    #[test]
    fn deleting_a_bookmark_clears_it_everywhere() {
        let mut lib = Library::default();
        let id = lib.add_bookmark("Fail", COLORS[0]);
        let p = Path::new("x.mp4");
        lib.toggle_bookmark(p, id);
        assert_eq!(lib.bookmark_uses(id), 1);
        lib.delete_bookmark(id);
        assert!(lib.bookmarks.is_empty());
        assert!(lib.meta(p).is_none());
    }

    #[test]
    fn empty_entries_are_pruned() {
        let mut lib = Library::default();
        let p = Path::new("x.mp4");
        lib.set_note(p, "hi");
        lib.set_note(p, "");
        assert!(lib.meta(p).is_none());
    }

    #[test]
    fn compressed_clips_are_flagged_and_recognised_by_name() {
        let mut lib = Library::default();
        let a = Path::new("V/a.mp4");
        assert!(!lib.is_compressed(a));
        lib.set_compressed(a, true);
        assert!(lib.is_compressed(a));
        lib.set_compressed(a, false);
        assert!(!lib.is_compressed(a));
        assert!(lib.is_compressed(Path::new("V/sc-game-20261019-10mb.mp4")));
        assert!(lib.is_compressed(Path::new("V/sc-game-20261019-trim-25mb-2.mp4")));
        assert!(!lib.is_compressed(Path::new("V/sc-game-20261019.mp4")));
        assert!(!lib.is_compressed(Path::new("V/my-2-clips.mp4")));
        assert!(!lib.is_compressed(Path::new("V/mb.mp4")));
    }

    #[test]
    fn trimmed_clips_are_flagged_and_recognised_by_name() {
        let mut lib = Library::default();
        let a = Path::new("V/a.mp4");
        assert!(!lib.is_trimmed(a));
        lib.set_trimmed(a, true);
        assert!(lib.is_trimmed(a));
        let b = Path::new("V/renamed.mp4");
        lib.rekey(a, b);
        assert!(lib.is_trimmed(b));
        assert!(lib.is_trimmed(Path::new("V/sc-game-20261019-trim.mp4")));
        assert!(lib.is_trimmed(Path::new("V/sc-game-20261019-trim2.mp4")));
        assert!(!lib.is_trimmed(Path::new("V/trimmer-time.mp4")));
        lib.set_trimmed(b, false);
        assert!(lib.meta(b).is_none());
    }

    #[test]
    fn names_are_made_safe() {
        assert_eq!(clean_name("  my: clip?*  "), "my- clip--");
        assert_eq!(clean_name("..."), "");
        assert_eq!(clean_name("ok name"), "ok name");
    }

    #[test]
    fn rename_move_and_collisions() {
        let dir = tmp("files");
        let a = dir.join("a.mp4");
        std::fs::write(&a, b"1").unwrap();
        std::fs::write(dir.join("b.mp4"), b"2").unwrap();

        let renamed = rename_clip(&a, "b").unwrap();
        assert_eq!(renamed.file_name().unwrap(), "b (2).mp4");
        assert_eq!(std::fs::read(dir.join("b.mp4")).unwrap(), b"2");

        let folder = create_folder(&dir, "Best of").unwrap();
        assert!(create_folder(&dir, "Best of").is_err());
        let moved = move_clip(&renamed, &folder).unwrap();
        assert!(moved.exists() && !renamed.exists());
        assert!(rename_clip(&moved, "  ").is_err());
    }
}
