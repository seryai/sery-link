//! F43 / F44 / F46 / F48 — incremental sync manifest.
//!
//! Each cache-and-scan source kind (SFTP, WebDAV, Dropbox, Azure)
//! writes a manifest file at `<cache_dir>/.sery-manifest.json` that
//! records the size + mtime marker each remote file had at the
//! last download. Subsequent rescans:
//!
//!   1. Load the manifest.
//!   2. For each remote file in the new listing, compare to the
//!      manifest entry. Same size + same mtime marker → skip.
//!   3. Download whatever's new or changed.
//!   4. Drop entries from the cache that are no longer in the
//!      remote listing (so removed-from-server files don't linger
//!      in scan results).
//!   5. Save the updated manifest.
//!
//! The mtime marker is intentionally typed as `String` — different
//! protocols expose mtimes in different shapes (Unix epoch from
//! SFTP stat, RFC 3339 from WebDAV/Dropbox, RFC 1123 HTTP-date from
//! Azure). The manifest compares them byte-for-byte; protocol-
//! specific code is responsible for using the same shape on each
//! call so the comparison is meaningful.
//!
//! On manifest read failure (corrupt JSON, missing file), we treat
//! the cache as empty and force a full re-download — safer than
//! silently using stale cached files. Bad case: extra bandwidth.
//! Good case: never serve incorrect data.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const MANIFEST_FILE: &str = ".sery-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileEntry {
    pub size_bytes: u64,
    /// Protocol-specific mtime marker. SFTP uses Unix-epoch seconds
    /// as a stringified integer; WebDAV / Dropbox use ISO-8601;
    /// Azure uses HTTP-date. Compared byte-for-byte; same protocol
    /// must be used consistently across calls.
    pub mtime_marker: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncManifest {
    /// Map from a stable per-source key (e.g. SFTP absolute path,
    /// Dropbox path_lower, Azure blob name) to the entry recorded
    /// at last successful download.
    #[serde(default)]
    pub files: HashMap<String, FileEntry>,
}

impl SyncManifest {
    pub fn manifest_path(cache_dir: &Path) -> PathBuf {
        cache_dir.join(MANIFEST_FILE)
    }

    /// Load the manifest from `<cache_dir>/.sery-manifest.json`.
    /// Returns an empty manifest when the file is missing or
    /// unparseable — defensive: a corrupt manifest forces a full
    /// re-download instead of risking stale data.
    pub fn load(cache_dir: &Path) -> Self {
        let path = Self::manifest_path(cache_dir);
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| Self::default()),
            Err(_) => Self::default(),
        }
    }

    /// Persist the manifest. Caller is responsible for ensuring
    /// `cache_dir` exists.
    pub fn save(&self, cache_dir: &Path) -> Result<()> {
        let path = Self::manifest_path(cache_dir);
        let json = serde_json::to_string_pretty(self).map_err(|e| {
            AgentError::Serialization(format!("encode manifest: {e}"))
        })?;
        std::fs::write(&path, json).map_err(|e| {
            AgentError::FileSystem(format!(
                "write manifest {}: {e}",
                path.display()
            ))
        })?;
        Ok(())
    }

    /// Returns true when the file at `key` needs (re-)downloading.
    /// True when:
    ///   - The key isn't in the manifest at all (first time seen).
    ///   - Size has changed.
    ///   - mtime marker has changed.
    /// Local file existence is NOT consulted here — that's the
    /// caller's responsibility, since "manifest hit but local file
    /// got deleted out from under us" still means we need to
    /// re-download.
    pub fn needs_download(
        &self,
        key: &str,
        current_size: u64,
        current_mtime: &str,
    ) -> bool {
        match self.files.get(key) {
            Some(entry) => {
                entry.size_bytes != current_size
                    || entry.mtime_marker != current_mtime
            }
            None => true,
        }
    }

    /// Record a successful download. Overwrites any existing entry.
    pub fn record(&mut self, key: String, size_bytes: u64, mtime_marker: String) {
        self.files.insert(
            key,
            FileEntry {
                size_bytes,
                mtime_marker,
            },
        );
    }

    /// Drop manifest entries that aren't in the supplied "current
    /// listing" set. Used after a walk to garbage-collect entries
    /// for files removed from the remote source. Returns the keys
    /// that were dropped — caller can use the list to also delete
    /// the stale local cache files (otherwise they'd keep showing
    /// up in scan results).
    pub fn drop_missing(&mut self, current_keys: &std::collections::HashSet<String>) -> Vec<String> {
        let stale: Vec<String> = self
            .files
            .keys()
            .filter(|k| !current_keys.contains(k.as_str()))
            .cloned()
            .collect();
        for k in &stale {
            self.files.remove(k);
        }
        stale
    }
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn needs_download_true_for_unknown_key() {
        let m = SyncManifest::default();
        assert!(m.needs_download("/foo.csv", 100, "2026-01-01"));
    }

    #[test]
    fn needs_download_false_for_exact_match() {
        let mut m = SyncManifest::default();
        m.record("/foo.csv".into(), 100, "2026-01-01".into());
        assert!(!m.needs_download("/foo.csv", 100, "2026-01-01"));
    }

    #[test]
    fn needs_download_true_when_size_changed() {
        let mut m = SyncManifest::default();
        m.record("/foo.csv".into(), 100, "2026-01-01".into());
        assert!(m.needs_download("/foo.csv", 200, "2026-01-01"));
    }

    #[test]
    fn needs_download_true_when_mtime_changed() {
        let mut m = SyncManifest::default();
        m.record("/foo.csv".into(), 100, "2026-01-01".into());
        assert!(m.needs_download("/foo.csv", 100, "2026-01-02"));
    }

    #[test]
    fn record_overwrites_existing_entry() {
        let mut m = SyncManifest::default();
        m.record("/foo.csv".into(), 100, "2026-01-01".into());
        m.record("/foo.csv".into(), 200, "2026-01-02".into());
        assert_eq!(m.files.len(), 1);
        assert_eq!(m.files["/foo.csv"].size_bytes, 200);
        assert_eq!(m.files["/foo.csv"].mtime_marker, "2026-01-02");
    }

    #[test]
    fn drop_missing_returns_keys_no_longer_present() {
        let mut m = SyncManifest::default();
        m.record("/a.csv".into(), 1, "x".into());
        m.record("/b.csv".into(), 2, "y".into());
        m.record("/c.csv".into(), 3, "z".into());
        let current: HashSet<String> =
            ["/a.csv".to_string(), "/c.csv".to_string()].into_iter().collect();
        let dropped = m.drop_missing(&current);
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0], "/b.csv");
        assert_eq!(m.files.len(), 2);
        assert!(m.files.contains_key("/a.csv"));
        assert!(m.files.contains_key("/c.csv"));
    }

    #[test]
    fn save_then_load_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = SyncManifest::default();
        m.record("/data.parquet".into(), 1234, "2026-05-04T00:00:00Z".into());
        m.record("/x.csv".into(), 5678, "epoch:1717459200".into());
        m.save(dir.path()).unwrap();

        let loaded = SyncManifest::load(dir.path());
        assert_eq!(loaded.files.len(), 2);
        assert_eq!(
            loaded.files["/data.parquet"],
            FileEntry {
                size_bytes: 1234,
                mtime_marker: "2026-05-04T00:00:00Z".into(),
            }
        );
    }

    #[test]
    fn load_returns_empty_on_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let m = SyncManifest::load(dir.path());
        assert!(m.files.is_empty());
    }

    #[test]
    fn load_returns_empty_on_corrupt_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            SyncManifest::manifest_path(dir.path()),
            "{not valid json",
        )
        .unwrap();
        let m = SyncManifest::load(dir.path());
        assert!(m.files.is_empty());
    }
}
