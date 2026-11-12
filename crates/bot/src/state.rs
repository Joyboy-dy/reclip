//! Downloads the bot follows, persisted to a small JSON file so tracking
//! resumes after a restart (a `delegate` key cannot list every user's jobs).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use reclip_shared::MediaFormat;
use serde::{Deserialize, Serialize};

use crate::telegram::MessageId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tracked {
    pub job_id: String,
    pub user_id: i64,
    pub chat_id: i64,
    /// The progress message, edited while the job runs.
    pub message_id: MessageId,
    /// `fr` or `en`.
    pub lang: String,
    pub format: MediaFormat,
    pub title: String,
    #[serde(default)]
    pub uploader: Option<String>,
    /// Seconds.
    #[serde(default)]
    pub duration: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub tracked: Vec<Tracked>,
}

/// The state and its file. Every change is written atomically
/// (temporary file then rename).
pub struct StateStore {
    path: PathBuf,
    inner: Mutex<State>,
}

impl StateStore {
    /// Reads the state file; a missing file is an empty state.
    pub fn load(path: &Path) -> Result<StateStore> {
        let state = match std::fs::read(path) {
            Ok(raw) => serde_json::from_slice(&raw)
                .with_context(|| format!("invalid state file {}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => State::default(),
            Err(e) => {
                return Err(e).with_context(|| format!("cannot read {}", path.display()));
            }
        };
        Ok(StateStore {
            path: path.to_path_buf(),
            inner: Mutex::new(state),
        })
    }

    /// An empty state that will be written to `path` on the first change.
    pub fn empty(path: &Path) -> StateStore {
        StateStore {
            path: path.to_path_buf(),
            inner: Mutex::new(State::default()),
        }
    }

    pub fn snapshot(&self) -> Vec<Tracked> {
        self.lock().tracked.clone()
    }

    pub fn get(&self, job_id: &str) -> Option<Tracked> {
        self.lock()
            .tracked
            .iter()
            .find(|t| t.job_id == job_id)
            .cloned()
    }

    pub fn for_user(&self, user_id: i64) -> Vec<Tracked> {
        self.lock()
            .tracked
            .iter()
            .filter(|t| t.user_id == user_id)
            .cloned()
            .collect()
    }

    /// Adds (or replaces) an entry and saves.
    pub fn add(&self, t: Tracked) {
        let mut s = self.lock();
        s.tracked.retain(|x| x.job_id != t.job_id);
        s.tracked.push(t);
        self.save(&s);
    }

    /// Removes an entry and saves; returns whether it was there.
    pub fn remove(&self, job_id: &str) -> bool {
        let mut s = self.lock();
        let before = s.tracked.len();
        s.tracked.retain(|x| x.job_id != job_id);
        let removed = s.tracked.len() != before;
        if removed {
            self.save(&s);
        }
        removed
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A failed write is logged: tracking goes on in memory.
    fn save(&self, s: &State) {
        if let Err(e) = write_atomic(&self.path, s) {
            tracing::warn!(path = %self.path.display(), error = %e, "cannot save the bot state");
        }
    }
}

fn write_atomic(path: &Path, s: &State) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let body = serde_json::to_vec_pretty(s).map_err(std::io::Error::other)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&body)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked(id: &str, user: i64) -> Tracked {
        Tracked {
            job_id: id.into(),
            user_id: user,
            chat_id: user,
            message_id: 7,
            lang: "fr".into(),
            format: MediaFormat::Mp3,
            title: "T".into(),
            uploader: Some("U".into()),
            duration: Some(61),
        }
    }

    #[test]
    fn missing_file_is_empty_and_changes_are_persisted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join("state.json");
        let s = StateStore::load(&path).unwrap();
        assert!(s.snapshot().is_empty());
        s.add(tracked("a", 1));
        s.add(tracked("b", 2));
        s.add(tracked("a", 1));
        assert_eq!(s.snapshot().len(), 2);
        assert_eq!(s.for_user(2)[0].job_id, "b");
        assert!(s.remove("b"));
        assert!(!s.remove("b"));
        let again = StateStore::load(&path).unwrap();
        assert_eq!(again.snapshot(), vec![tracked("a", 1)]);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_file_is_an_error_and_empty_writes_lazily() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        std::fs::write(&path, "{nope").unwrap();
        assert!(StateStore::load(&path).is_err());
        let s = StateStore::empty(&path);
        assert!(s.get("x").is_none());
        s.add(tracked("x", 3));
        assert_eq!(StateStore::load(&path).unwrap().snapshot().len(), 1);
    }
}
