//! Format and quality choices waiting for a button click, kept in memory.

use std::collections::HashMap;
use std::hash::{BuildHasher, RandomState};
use std::time::{Duration, Instant};

use reclip_shared::FormatOption;

use crate::i18n::Lang;

/// Most quality buttons shown (one row per height).
pub const MAX_QUALITIES: usize = 6;
/// Bound on the map, so a flood of links cannot grow it forever.
const MAX_ENTRIES: usize = 10_000;

/// What the user has to choose about one analysed link.
#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    /// Only this user may press the buttons.
    pub user_id: i64,
    pub url: String,
    pub lang: Lang,
    pub title: String,
    pub thumbnail: Option<String>,
    pub uploader: Option<String>,
    /// Seconds.
    pub duration: Option<u32>,
    /// The quality buttons, in order: the button carries the index.
    pub qualities: Vec<FormatOption>,
}

pub struct Pending {
    ttl: Duration,
    prefix: u32,
    counter: u64,
    map: HashMap<String, (Choice, Instant)>,
}

impl Pending {
    pub fn new(ttl: Duration) -> Pending {
        // Random per process: buttons from before a restart never match a
        // new choice by accident.
        let prefix = RandomState::new().hash_one(0u8) as u32;
        Pending {
            ttl,
            prefix,
            counter: 0,
            map: HashMap::new(),
        }
    }

    /// Stores a choice and returns its short id (a few hex digits).
    pub fn insert(&mut self, c: Choice, now: Instant) -> String {
        self.map
            .retain(|_, (_, at)| now.duration_since(*at) < self.ttl);
        if self.map.len() >= MAX_ENTRIES
            && let Some(oldest) = self
                .map
                .iter()
                .min_by_key(|(_, (_, at))| *at)
                .map(|(k, _)| k.clone())
        {
            self.map.remove(&oldest);
        }
        self.counter += 1;
        let id = format!("{:08x}{:x}", self.prefix, self.counter);
        self.map.insert(id.clone(), (c, now));
        id
    }

    /// The choice, unless it is unknown or older than the time limit.
    pub fn get(&self, id: &str, now: Instant) -> Option<&Choice> {
        self.map
            .get(id)
            .filter(|(_, at)| now.duration_since(*at) < self.ttl)
            .map(|(c, _)| c)
    }

    /// Removes the choice so that a second click cannot start a second job.
    pub fn take(&mut self, id: &str, now: Instant) -> Option<Choice> {
        let fresh = self.get(id, now).is_some();
        let entry = self.map.remove(id);
        entry.filter(|_| fresh).map(|(c, _)| c)
    }
}

/// One option per height, highest first, H.264 preferred, at most
/// [`MAX_QUALITIES`].
pub fn qualities(formats: &[FormatOption]) -> Vec<FormatOption> {
    let mut sorted: Vec<&FormatOption> = formats.iter().collect();
    sorted.sort_by_key(|f| (std::cmp::Reverse(f.height), f.codec.is_some()));
    let mut out: Vec<FormatOption> = Vec::new();
    for f in sorted {
        if out.len() == MAX_QUALITIES {
            break;
        }
        if out.last().is_none_or(|l| l.height != f.height) {
            out.push(f.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice() -> Choice {
        Choice {
            user_id: 1,
            url: "https://v/1".into(),
            lang: Lang::En,
            title: "T".into(),
            thumbnail: None,
            uploader: None,
            duration: None,
            qualities: vec![],
        }
    }

    fn fmt(id: &str, height: u32, codec: Option<&str>) -> FormatOption {
        FormatOption {
            id: id.into(),
            label: format!("{height}p"),
            height,
            codec: codec.map(Into::into),
        }
    }

    #[test]
    fn ids_are_short_unique_and_expire() {
        let t0 = Instant::now();
        let mut p = Pending::new(Duration::from_secs(1800));
        let a = p.insert(choice(), t0);
        let b = p.insert(choice(), t0);
        assert_ne!(a, b);
        assert!(a.len() <= 16);
        assert!(p.get(&a, t0 + Duration::from_secs(1799)).is_some());
        assert!(p.get(&a, t0 + Duration::from_secs(1800)).is_none());
        assert!(p.take(&b, t0 + Duration::from_secs(1801)).is_none());
        assert!(p.take(&a, t0).is_some());
        assert!(p.take(&a, t0).is_none(), "taken once only");
    }

    #[test]
    fn one_quality_per_height_h264_first_capped() {
        let formats = vec![
            fmt("vp9-1080", 1080, Some("VP9")),
            fmt("137", 1080, None),
            fmt("22", 720, None),
            fmt("a", 144, None),
            fmt("b", 240, None),
            fmt("c", 360, None),
            fmt("d", 480, None),
            fmt("e", 1440, Some("VP9")),
        ];
        let q = qualities(&formats);
        let ids: Vec<&str> = q.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["e", "137", "22", "d", "c", "b"]);
    }
}
