//! Filesystem free-space queries for disk-pressure UX.
//!
//! Wraps the `fs4` crate (statvfs / GetDiskFreeSpaceExW behind a
//! cross-platform API) so the rest of the app doesn't have to
//! think about per-OS quirks. Used by:
//!
//!   - `commands::gdrive_watch_folder` for the pre-flight check
//!     that refuses to start a watch when free space is below
//!     `MIN_FREE_BYTES_FOR_WATCH`.
//!   - The Settings → Storage Tauri command (planned next slice)
//!     for showing "X GB free / Y GB used by Sery cache".
//!
//! Errors from the underlying syscall are surfaced as
//! `AgentError::Config` rather than swallowed — callers usually
//! want to either show or log them, not silently treat as "lots of
//! space available." The one exception is when the path doesn't
//! exist yet (first-run, Drive cache dir not yet created): in that
//! case we walk up to the nearest existing ancestor so the query
//! still returns something meaningful.

use crate::error::{AgentError, Result};
use fs4::available_space;
use std::path::{Path, PathBuf};

/// Refuse to start a Drive watch when the user has less than this
/// many bytes free on the volume holding the Sery data dir. 5 GiB
/// is enough headroom for a typical small-Drive watch (a few
/// thousand documents) without immediately filling the disk; users
/// with a tight machine can clear other space and try again.
pub const MIN_FREE_BYTES_FOR_WATCH: u64 = 5 * 1024 * 1024 * 1024;

/// Bytes available on the volume holding `path`. If `path` doesn't
/// exist yet, walks up to the nearest existing ancestor so the
/// query still resolves — the data dir may not have been created
/// before the first scan, but `~/.seryai/..` always exists.
pub fn available_bytes(path: &Path) -> Result<u64> {
    let probe = nearest_existing(path);
    available_space(&probe)
        .map_err(|e| AgentError::Config(format!("free-space query failed: {}", e)))
}

fn nearest_existing(path: &Path) -> PathBuf {
    let mut p: PathBuf = path.to_path_buf();
    while !p.exists() {
        match p.parent() {
            Some(parent) => p = parent.to_path_buf(),
            None => break,
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_bytes_for_home_is_nonzero() {
        // Smoke test: every dev / CI machine has SOME bytes free
        // in $HOME. Anything else means the syscall is broken.
        let home = dirs::home_dir().expect("home dir");
        let n = available_bytes(&home).expect("query ok");
        assert!(n > 0, "expected nonzero available space, got {}", n);
    }

    #[test]
    fn nonexistent_path_walks_up_to_parent() {
        // The Drive cache dir doesn't exist before the first
        // download — but querying its free space should still work
        // because the parent (~/.seryai or even ~/) does exist.
        let home = dirs::home_dir().expect("home dir");
        let phantom = home.join(".this-dir-does-not-exist-9f8e7d");
        let n = available_bytes(&phantom).expect("query ok via parent");
        assert!(n > 0);
    }
}
