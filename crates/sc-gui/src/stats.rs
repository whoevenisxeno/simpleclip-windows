use crate::library::Library;
use crate::screens::gallery::Clip;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::SystemTime;
use time::{Date, OffsetDateTime, UtcOffset};

pub const DAYS: usize = 14;

#[derive(Default, Debug, PartialEq)]
pub struct Stats {
    pub clips: usize,
    pub shots: usize,
    pub bytes: u64,
    pub clip_bytes: u64,
    pub today: usize,
    pub week: usize,
    pub streak: u32,
    pub best_streak: u32,
    pub busiest: Option<(Date, usize)>,
    pub hour: Option<u8>,
    pub games: Vec<(String, usize)>,
    pub days: Vec<(Date, usize)>,
    pub edited: usize,
    pub compressed: usize,
    pub marked: usize,
    pub since: Option<Date>,
}

fn date(t: SystemTime, off: UtcOffset) -> OffsetDateTime {
    OffsetDateTime::from(t).to_offset(off)
}

impl Stats {
    pub fn compute(clips: &[Clip], lib: &Library, now: SystemTime, off: UtcOffset) -> Self {
        let today = date(now, off).date();
        let mut s = Stats::default();
        let mut per_day: BTreeMap<Date, usize> = BTreeMap::new();
        let mut hours = [0usize; 24];
        let mut games: HashMap<String, usize> = HashMap::new();
        for c in clips {
            s.bytes += c.bytes;
            if c.is_image {
                s.shots += 1;
                continue;
            }
            if lib.is_trimmed(&c.path) {
                s.edited += 1;
                continue;
            }
            if lib.is_compressed(&c.path) {
                s.compressed += 1;
                continue;
            }
            s.clips += 1;
            s.clip_bytes += c.bytes;
            if !lib.bookmarks_of(&c.path).is_empty() {
                s.marked += 1;
            }
            let l = date(c.modified, off);
            *per_day.entry(l.date()).or_default() += 1;
            hours[l.hour() as usize] += 1;
            *games.entry(c.game.clone()).or_default() += 1;
        }
        s.today = per_day.get(&today).copied().unwrap_or(0);
        s.week = per_day
            .iter()
            .filter(|(d, _)| (today - **d).whole_days() < 7 && **d <= today)
            .map(|(_, n)| n)
            .sum();
        s.since = per_day.keys().next().copied();
        s.busiest = per_day
            .iter()
            .max_by_key(|(d, n)| (**n, std::cmp::Reverse(**d)))
            .map(|(d, n)| (*d, *n));
        s.hour = hours
            .iter()
            .enumerate()
            .filter(|(_, n)| **n > 0)
            .max_by_key(|(h, n)| (**n, std::cmp::Reverse(*h)))
            .map(|(h, _)| h as u8);
        let set: BTreeSet<Date> = per_day.keys().copied().collect();
        s.streak = streak(&set, today);
        s.best_streak = best(&set);
        s.days = (0..DAYS as i64)
            .rev()
            .filter_map(|i| today.checked_sub(time::Duration::days(i)))
            .map(|d| (d, per_day.get(&d).copied().unwrap_or(0)))
            .collect();
        let mut g: Vec<(String, usize)> = games.into_iter().collect();
        g.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        if g.len() > 5 {
            let rest: usize = g[4..].iter().map(|x| x.1).sum();
            g.truncate(4);
            g.push(("Everything else".into(), rest));
        }
        s.games = g;
        s
    }

    pub fn avg_bytes(&self) -> u64 {
        if self.clips == 0 {
            0
        } else {
            self.clip_bytes / self.clips as u64
        }
    }
}

fn streak(set: &BTreeSet<Date>, today: Date) -> u32 {
    let mut d = if set.contains(&today) {
        today
    } else {
        match today.previous_day() {
            Some(y) if set.contains(&y) => y,
            _ => return 0,
        }
    };
    let mut n = 1;
    while let Some(p) = d.previous_day() {
        if !set.contains(&p) {
            break;
        }
        n += 1;
        d = p;
    }
    n
}

fn best(set: &BTreeSet<Date>) -> u32 {
    let (mut top, mut run, mut last) = (0, 0, None::<Date>);
    for d in set {
        run = if last.and_then(|l| l.next_day()) == Some(*d) {
            run + 1
        } else {
            1
        };
        top = top.max(run);
        last = Some(*d);
    }
    top
}

pub fn hour_label(h: u8) -> String {
    let part = match h {
        5..=11 => "Mornings",
        12..=16 => "Afternoons",
        17..=21 => "Evenings",
        _ => "Late nights",
    };
    let (n, ap) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{part}, around {n} {ap}")
}

pub fn date_label(d: Date) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {}",
        d.day(),
        MONTHS[d.month() as usize - 1],
        d.year()
    )
}

pub fn short_date(d: Date) -> String {
    const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{} {} {}",
        DAYS[d.weekday().number_days_from_monday() as usize],
        d.day(),
        MONTHS[d.month() as usize - 1]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    const DAY: u64 = 86_400;

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn clip(name: &str, game: &str, t: u64, bytes: u64) -> Clip {
        Clip {
            path: PathBuf::from(format!("V/{name}")),
            name: name.into(),
            modified: at(t),
            bytes,
            is_image: name.ends_with(".png"),
            game: game.into(),
        }
    }

    #[test]
    fn counts_streaks_and_games() {
        let now = 1_000 * DAY + 20 * 3600;
        let clips = vec![
            clip("sc-r6-1.mp4", "Rainbowsix", now - 3600, 100),
            clip("sc-r6-2.mp4", "Rainbowsix", now - 2 * 3600, 100),
            clip("sc-cs2-1.mp4", "Cs2", now - DAY, 50),
            clip("sc-cs2-2.mp4", "Cs2", now - 2 * DAY, 50),
            clip("sc-cs2-3.mp4", "Cs2", now - 10 * DAY, 50),
            clip("sc-cs2-3-trim.mp4", "Cs2", now - DAY, 10),
            clip("sc-cs2-3-10mb.mp4", "Cs2", now - DAY, 10),
            clip("shot.png", "Desktop", now, 5),
        ];
        let s = Stats::compute(&clips, &Library::default(), at(now), UtcOffset::UTC);
        assert_eq!(s.clips, 5);
        assert_eq!(s.shots, 1);
        assert_eq!(s.edited, 1);
        assert_eq!(s.compressed, 1);
        assert_eq!(s.bytes, 375);
        assert_eq!(s.avg_bytes(), 70);
        assert_eq!(s.today, 2);
        assert_eq!(s.week, 4);
        assert_eq!(s.streak, 3);
        assert_eq!(s.best_streak, 3);
        assert_eq!(s.busiest.map(|b| b.1), Some(2));
        assert_eq!(s.hour, Some(20));
        assert_eq!(s.games[0], ("Cs2".to_string(), 3));
        assert_eq!(s.days.len(), DAYS);
        assert_eq!(s.days.last().unwrap().1, 2);
        assert_eq!(s.days.iter().map(|d| d.1).sum::<usize>(), 5);
    }

    #[test]
    fn streak_survives_until_a_full_day_is_missed() {
        let now = 1_000 * DAY + 3600;
        let one = |gap: u64| {
            let c = vec![
                clip("a.mp4", "A", now - gap * DAY, 1),
                clip("b.mp4", "A", now - (gap + 1) * DAY, 1),
            ];
            Stats::compute(&c, &Library::default(), at(now), UtcOffset::UTC).streak
        };
        assert_eq!(one(1), 2);
        assert_eq!(one(2), 0);
    }

    #[test]
    fn many_games_fold_into_the_rest() {
        let clips: Vec<Clip> = (0..7)
            .map(|i| clip(&format!("c{i}.mp4"), &format!("G{i}"), 0, 1))
            .collect();
        let s = Stats::compute(&clips, &Library::default(), at(0), UtcOffset::UTC);
        assert_eq!(s.games.len(), 5);
        assert_eq!(s.games[4], ("Everything else".to_string(), 3));
    }

    #[test]
    fn empty_folder_has_no_highlights() {
        let s = Stats::compute(&[], &Library::default(), at(0), UtcOffset::UTC);
        assert_eq!(s.clips, 0);
        assert!(s.busiest.is_none() && s.hour.is_none() && s.since.is_none());
        assert_eq!(s.streak, 0);
    }

    #[test]
    fn labels_read_naturally() {
        assert_eq!(hour_label(21), "Evenings, around 9 PM");
        assert_eq!(hour_label(0), "Late nights, around 12 AM");
        assert_eq!(hour_label(9), "Mornings, around 9 AM");
    }
}
