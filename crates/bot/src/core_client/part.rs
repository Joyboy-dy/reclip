//! The partial file of a download in progress.

use std::path::{Path, PathBuf};

/// `<dest>.part`: where a download lives until it is complete.
pub(super) fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    name.into()
}

/// Removes the partial file on any error, and when the future is dropped.
pub(super) struct PartGuard {
    pub(super) path: PathBuf,
    pub(super) committed: bool,
}

impl Drop for PartGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
