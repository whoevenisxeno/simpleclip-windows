use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub path: PathBuf,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Original,
    Trimmed,
    Compressed,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Original => "Original",
            Kind::Trimmed => "Trimmed",
            Kind::Compressed => "Compressed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Group {
    pub keep: Vec<Item>,
    pub remove: Vec<Item>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub remove: Vec<Item>,
    pub groups: Vec<Group>,
}

impl Plan {
    pub fn bytes(&self) -> u64 {
        self.remove.iter().map(|i| i.bytes).sum()
    }
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_size(s: &str) -> bool {
    s.strip_suffix("mb").is_some_and(all_digits)
}

fn strip_compress(stem: &str) -> Option<&str> {
    let (head, last) = stem.rsplit_once('-')?;
    if is_size(last) {
        return Some(head);
    }
    if all_digits(last) {
        let (earlier, size) = head.rsplit_once('-')?;
        if is_size(size) {
            return Some(earlier);
        }
    }
    None
}

fn strip_trim(stem: &str) -> Option<&str> {
    let (head, last) = stem.rsplit_once('-')?;
    let rest = last.strip_prefix("trim")?;
    (rest.is_empty() || all_digits(rest)).then_some(head)
}

fn source_of(stem: &str) -> Option<&str> {
    strip_compress(stem).or_else(|| strip_trim(stem))
}

pub fn kind_of(path: &Path) -> Kind {
    let stem = stem_of(path);
    if strip_compress(&stem).is_some() {
        Kind::Compressed
    } else if strip_trim(&stem).is_some() {
        Kind::Trimmed
    } else {
        Kind::Original
    }
}

fn root_of(stem: &str) -> String {
    let mut current = stem;
    while let Some(source) = source_of(current) {
        current = source;
    }
    current.to_string()
}

fn stem_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

pub fn plan(items: Vec<Item>) -> Plan {
    let stems: Vec<String> = items.iter().map(|i| stem_of(&i.path)).collect();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for s in &stems {
        *seen.entry(s.as_str()).or_default() += 1;
    }
    let mut has_descendant: HashSet<&str> = HashSet::new();
    for stem in &stems {
        let mut current = stem.as_str();
        while let Some(source) = source_of(current) {
            has_descendant.insert(source);
            current = source;
        }
    }
    let doomed: Vec<bool> = stems
        .iter()
        .map(|stem| has_descendant.contains(stem.as_str()) && seen.get(stem.as_str()) == Some(&1))
        .collect();
    let remove: Vec<Item> = items
        .iter()
        .zip(&doomed)
        .filter(|(_, d)| **d)
        .map(|(item, _)| item.clone())
        .collect();
    let mut order: Vec<String> = Vec::new();
    let mut by_root: HashMap<String, Group> = HashMap::new();
    for ((item, stem), gone) in items.iter().zip(&stems).zip(&doomed) {
        let root = root_of(stem);
        let group = by_root.entry(root.clone()).or_insert_with(|| {
            order.push(root.clone());
            Group::default()
        });
        if *gone {
            group.remove.push(item.clone());
        } else {
            group.keep.push(item.clone());
        }
    }
    let groups = order
        .into_iter()
        .filter_map(|root| by_root.remove(&root))
        .filter(|g| !g.remove.is_empty())
        .collect();
    Plan { remove, groups }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(name: &str) -> Item {
        Item {
            path: PathBuf::from(format!("C:/Clips/{name}.mp4")),
            bytes: 10,
        }
    }

    fn removed(names: &[&str]) -> Vec<String> {
        let plan = plan(names.iter().map(|n| item(n)).collect());
        let mut out: Vec<String> = plan.remove.iter().map(|i| stem_of(&i.path)).collect();
        out.sort();
        out
    }

    #[test]
    fn original_trim_and_compressed_keeps_only_the_compressed_copy() {
        let got = removed(&["sc-apex-1", "sc-apex-1-trim", "sc-apex-1-trim-10mb"]);
        assert_eq!(got, vec!["sc-apex-1", "sc-apex-1-trim"]);
    }

    #[test]
    fn original_and_trim_keeps_the_trim() {
        assert_eq!(removed(&["sc-apex-1", "sc-apex-1-trim"]), vec!["sc-apex-1"]);
    }

    #[test]
    fn a_compressed_original_and_a_separate_trim_drop_only_the_original() {
        let got = removed(&["sc-apex-1", "sc-apex-1-trim", "sc-apex-1-25mb"]);
        assert_eq!(got, vec!["sc-apex-1"]);
    }

    #[test]
    fn a_second_trim_is_a_different_moment_and_is_kept() {
        let got = removed(&[
            "sc-apex-1",
            "sc-apex-1-trim",
            "sc-apex-1-trim2",
            "sc-apex-1-trim-10mb",
        ]);
        assert_eq!(got, vec!["sc-apex-1", "sc-apex-1-trim"]);
    }

    #[test]
    fn numbered_compressions_are_recognised() {
        let got = removed(&["sc-apex-1", "sc-apex-1-10mb-2"]);
        assert_eq!(got, vec!["sc-apex-1"]);
    }

    #[test]
    fn unrelated_clips_are_left_alone() {
        assert!(removed(&["sc-apex-1", "sc-apex-2", "sc-valorant-3-trim"]).is_empty());
    }

    #[test]
    fn two_clips_sharing_a_name_are_never_removed() {
        let mut items = vec![item("Ace"), item("Ace-trim")];
        items.push(Item {
            path: PathBuf::from("D:/Other/Ace.mp4"),
            bytes: 5,
        });
        assert!(plan(items).remove.is_empty());
    }

    #[test]
    fn names_are_matched_without_regard_to_case() {
        assert_eq!(removed(&["Ace", "ace-TRIM"]), vec!["ace"]);
    }

    #[test]
    fn the_plan_adds_up_the_space_freed() {
        let plan = plan(vec![item("a"), item("a-trim")]);
        assert_eq!(plan.remove.len(), 1);
        assert_eq!(plan.bytes(), 10);
    }
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn item(name: &str, bytes: u64) -> Item {
        Item {
            path: PathBuf::from(format!("C:/Clips/{name}.mp4")),
            bytes,
        }
    }

    #[test]
    fn a_group_shows_what_stays_and_what_goes() {
        let plan = plan(vec![
            item("a", 100),
            item("a-trim", 40),
            item("a-trim-10mb", 10),
            item("other", 5),
        ]);
        assert_eq!(plan.groups.len(), 1);
        let g = &plan.groups[0];
        assert_eq!(g.keep.len(), 1);
        assert_eq!(kind_of(&g.keep[0].path), Kind::Compressed);
        assert_eq!(g.remove.len(), 2);
        assert_eq!(plan.bytes(), 140);
    }

    #[test]
    fn kinds_are_read_from_the_name() {
        assert_eq!(kind_of(Path::new("x/a.mp4")), Kind::Original);
        assert_eq!(kind_of(Path::new("x/a-trim2.mp4")), Kind::Trimmed);
        assert_eq!(kind_of(Path::new("x/a-trim-25mb.mp4")), Kind::Compressed);
    }
}
