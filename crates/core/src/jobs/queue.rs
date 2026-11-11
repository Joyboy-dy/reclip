//! Fair, bounded job queue. Pure: no I/O, no clock.
//!
//! Fairness: owners take turns. The owner picked next is the one served
//! longest ago; owners never served (or back after their queue emptied) come
//! first, in arrival order. Within an owner, jobs keep their arrival order,
//! except that a job whose site is saturated is skipped (not blocking the
//! others) and keeps its place for later.

use std::collections::{HashMap, VecDeque};

/// The site a URL counts against for the per-site limit: lowercase host
/// without a leading `www.`, `m.` or `music.`; `youtu.be` is `youtube.com`.
/// Unparsable input is returned as is.
pub fn site_of(url: &str) -> String {
    let Some(host) = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    else {
        return url.to_string();
    };
    let host = ["www.", "m.", "music."]
        .iter()
        .find_map(|p| host.strip_prefix(p))
        .unwrap_or(&host);
    match host {
        "youtu.be" => "youtube.com".to_string(),
        h => h.to_string(),
    }
}

struct Waiting {
    id: String,
    site: String,
}

struct OwnerQueue {
    jobs: VecDeque<Waiting>,
    /// Tick of the last job taken from this owner, 0 if none yet.
    last_served: u64,
    /// Tick at which this owner (re)entered the queue.
    arrived: u64,
}

/// Waiting jobs, grouped by owner.
pub struct Queue {
    max_total: usize,
    max_per_site: usize,
    owners: HashMap<String, OwnerQueue>,
    tick: u64,
}

impl Queue {
    pub fn new(max_total: usize, max_per_site: usize) -> Queue {
        Queue {
            max_total,
            max_per_site,
            owners: HashMap::new(),
            tick: 0,
        }
    }

    pub fn push(&mut self, job_id: &str, owner: &str, site: &str) {
        self.tick += 1;
        let arrived = self.tick;
        self.owners
            .entry(owner.to_string())
            .or_insert_with(|| OwnerQueue {
                jobs: VecDeque::new(),
                last_served: 0,
                arrived,
            })
            .jobs
            .push_back(Waiting {
                id: job_id.to_string(),
                site: site.to_string(),
            });
    }

    /// Drop a waiting job. `false` if it was not queued.
    pub fn remove(&mut self, job_id: &str) -> bool {
        let Some(owner) = self.owners.iter_mut().find_map(|(owner, q)| {
            let pos = q.jobs.iter().position(|w| w.id == job_id)?;
            q.jobs.remove(pos);
            Some(owner.clone())
        }) else {
            return false;
        };
        self.drop_if_empty(&owner);
        true
    }

    /// Take the next job allowed to start, given what already runs.
    pub fn next(
        &mut self,
        running_total: usize,
        running_per_site: &HashMap<String, usize>,
    ) -> Option<String> {
        if running_total >= self.max_total {
            return None;
        }
        let max_per_site = self.max_per_site;
        let free = |site: &str| running_per_site.get(site).copied().unwrap_or(0) < max_per_site;
        let mut order: Vec<(&String, &OwnerQueue)> = self.owners.iter().collect();
        order.sort_by_key(|(_, q)| (q.last_served, q.arrived));
        let (owner, pos) = order.into_iter().find_map(|(owner, q)| {
            let pos = q.jobs.iter().position(|w| free(&w.site))?;
            Some((owner.clone(), pos))
        })?;
        self.tick += 1;
        let q = self.owners.get_mut(&owner)?;
        let job = q.jobs.remove(pos)?;
        q.last_served = self.tick;
        self.drop_if_empty(&owner);
        Some(job.id)
    }

    pub fn len(&self) -> usize {
        self.owners.values().map(|q| q.jobs.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.owners.is_empty()
    }

    fn drop_if_empty(&mut self, owner: &str) {
        if self.owners.get(owner).is_some_and(|q| q.jobs.is_empty()) {
            self.owners.remove(owner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drain the queue as a scheduler would, with jobs never finishing.
    fn drain(q: &mut Queue, sites: &[(&str, &str)]) -> Vec<String> {
        let site = |id: &str| sites.iter().find(|(j, _)| *j == id).unwrap().1.to_string();
        let mut per_site = HashMap::new();
        let mut out = Vec::new();
        while let Some(id) = q.next(out.len(), &per_site) {
            *per_site.entry(site(&id)).or_insert(0) += 1;
            out.push(id);
        }
        out
    }

    #[test]
    fn respects_global_limit() {
        let mut q = Queue::new(2, 10);
        for id in ["a", "b", "c"] {
            q.push(id, "o", id);
        }
        let none = HashMap::new();
        assert_eq!(q.next(2, &none), None);
        assert_eq!(q.next(1, &none).as_deref(), Some("a"));
        assert_eq!(q.next(2, &none), None);
        assert_eq!(q.next(0, &none).as_deref(), Some("b"));
    }

    #[test]
    fn respects_per_site_limit() {
        let mut q = Queue::new(10, 2);
        for id in ["a", "b", "c"] {
            q.push(id, "o", "s.com");
        }
        let sites = [("a", "s.com"), ("b", "s.com"), ("c", "s.com")];
        assert_eq!(drain(&mut q, &sites), ["a", "b"]);
        let busy = HashMap::from([("s.com".to_string(), 1)]);
        assert_eq!(q.next(1, &busy).as_deref(), Some("c"));
    }

    #[test]
    fn skips_saturated_site_without_blocking_others() {
        let mut q = Queue::new(10, 1);
        q.push("a1", "o", "a.com");
        q.push("a2", "o", "a.com");
        q.push("b1", "o", "b.com");
        q.push("a3", "p", "a.com");
        q.push("c1", "p", "c.com");
        let sites = [
            ("a1", "a.com"),
            ("a2", "a.com"),
            ("b1", "b.com"),
            ("a3", "a.com"),
            ("c1", "c.com"),
        ];
        assert_eq!(drain(&mut q, &sites), ["a1", "c1", "b1"]);
        let free = HashMap::new();
        // `p` was served before `o` last time, so its waiting job goes first.
        assert_eq!(q.next(0, &free).as_deref(), Some("a3"));
    }

    #[test]
    fn round_robin_between_owners() {
        let mut q = Queue::new(1, 10);
        for id in ["a1", "a2", "a3"] {
            q.push(id, "A", id);
        }
        q.push("b1", "B", "b1");
        let none = HashMap::new();
        let order: Vec<String> = std::iter::from_fn(|| q.next(0, &none)).collect();
        assert_eq!(order, ["a1", "b1", "a2", "a3"]);
    }

    #[test]
    fn owner_served_last_goes_after_newcomers() {
        let mut q = Queue::new(1, 10);
        q.push("a1", "A", "x");
        q.push("a2", "A", "x");
        let none = HashMap::new();
        assert_eq!(q.next(0, &none).as_deref(), Some("a1"));
        q.push("c1", "C", "x");
        assert_eq!(q.next(0, &none).as_deref(), Some("c1"));
        assert_eq!(q.next(0, &none).as_deref(), Some("a2"));
    }

    #[test]
    fn remove_queued_job() {
        let mut q = Queue::new(10, 10);
        q.push("a", "o", "x");
        q.push("b", "o", "y");
        assert!(q.remove("a"));
        assert!(!q.remove("a"));
        assert!(!q.remove("zz"));
        let none = HashMap::new();
        assert_eq!(q.next(0, &none).as_deref(), Some("b"));
        assert_eq!(q.next(1, &none), None);
        assert!(q.is_empty());
    }

    #[test]
    fn site_of_normalises_hosts() {
        for (url, site) in [
            ("https://www.YouTube.com/watch?v=x", "youtube.com"),
            ("https://m.youtube.com/watch?v=x", "youtube.com"),
            ("https://music.youtube.com/watch?v=x", "youtube.com"),
            ("https://youtu.be/abc", "youtube.com"),
            ("https://www.youtu.be/abc", "youtube.com"),
            ("https://vimeo.com/1", "vimeo.com"),
            ("http://slow.example/1", "slow.example"),
            ("https://sub.www.example.com/", "sub.www.example.com"),
            ("not a url", "not a url"),
        ] {
            assert_eq!(site_of(url), site, "{url}");
        }
    }
}
