//! F42 — Sources data model (foundation slice).
//!
//! This is the *additive* foundation for the Sources sidebar refactor
//! (`SPEC_F42_SOURCES_SIDEBAR.md` in the datalake repo). It defines
//! the new `SourceKind` enum + `DataSource` struct + the migration
//! function from the old `WatchedFolder` shape, plus tests.
//!
//! **What this slice DOESN'T do (deliberately):**
//!   - It does NOT modify `Config` or any caller. The new `DataSource`
//!     type is unused at runtime today; future sessions wire it in.
//!   - It does NOT migrate existing user data on disk.
//!   - It does NOT touch `scan_cache.db`, the keychain, or the scanner.
//!
//! The point of this slice is to land the type definitions + the
//! pure migration function with full test coverage so the next session
//! can flip Config::load to call it without designing the data model
//! under pressure. F42 spec §6 calls this Day 1-2 of an 8-day sprint.
//!
//! See SPEC_F42_SOURCES_SIDEBAR.md §2 for the data model rationale,
//! §2.3 for the migration semantics, §5 for the fixture-test plan.

use crate::config::{ScanStats, WatchedFolder};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Discriminated union of every storage protocol Sery Link can
/// register as a source. Today's variants cover the v0.6.x reality
/// (Local, Https, S3, GoogleDrive); F43-F49 add Sftp, Webdav, B2,
/// Azure, Gcs, Dropbox, OneDrive following the same shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceKind {
    /// Local filesystem folder. Direct port of the original
    /// `WatchedFolder` shape.
    Local {
        path: PathBuf,
        recursive: bool,
        exclude_patterns: Vec<String>,
        max_file_size_mb: u64,
    },

    /// Public HTTPS URL pointing at one tabular file.
    Https { url: String },

    /// S3 bucket / prefix / glob. Region + access keys live in the
    /// keychain entry keyed on `source_id`.
    S3 { url: String },

    /// Google Drive account. `account_id` keys the keychain entry
    /// (today always the literal `"default"`; multi-account is a
    /// future feature).
    GoogleDrive { account_id: String },

    /// F43: SFTP server. Connection metadata (host / port / username
    /// / base_path) lives here; auth (password OR private-key path)
    /// lives in the keychain via `sftp_creds`, keyed on source_id.
    /// Files are downloaded to `~/.seryai/sftp-cache/<source_id>/`
    /// on rescan and the cache dir feeds the path-keyed scanner.
    Sftp {
        host: String,
        #[serde(default = "default_sftp_port")]
        port: u16,
        username: String,
        /// Absolute remote path to walk recursively. The cache
        /// mirrors this hierarchy locally.
        base_path: String,
    },

    /// F44: WebDAV server. Server URL + base_path live here;
    /// auth (Anonymous / Basic / Digest) lives in the keychain via
    /// `webdav_creds`, keyed on source_id. Files are downloaded to
    /// `~/.seryai/webdav-cache/<source_id>/` on rescan.
    WebDav {
        /// Server base URL — e.g. `https://nc.example.com/remote.php/dav/files/<user>/`
        /// for Nextcloud, `https://dav.example.com/` for generic.
        server_url: String,
        /// Path under server_url to walk recursively. Use `/` to
        /// walk the entire tree.
        base_path: String,
    },

    /// F48: Dropbox. v0.7.0 ships PAT auth only — the access token
    /// lives in the keychain via `dropbox_creds`, keyed on
    /// source_id. OAuth + refresh comes in a later slice. Files
    /// are downloaded to `~/.seryai/dropbox-cache/<source_id>/`
    /// on rescan.
    Dropbox {
        /// Path inside the user's Dropbox to walk recursively.
        /// Use `/` (or empty) for the root.
        base_path: String,
    },
    // F46, F47, F49 add new variants here (Azure, GCS native,
    // OneDrive). GCS via S3 interop already works through F45.
}

fn default_sftp_port() -> u16 {
    22
}

/// One bookmarked source in the Sources sidebar. Future surfaces
/// (rename, reorder, group) are spec'd in F51.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataSource {
    /// Stable, unique within this Sery Link install. Used as the
    /// keychain account key for credential lookup, the cache-key
    /// prefix in scan_cache.db, and the URL fragment for deep links.
    /// Generated as a UUIDv4 on add; never reused.
    pub id: String,

    /// User-editable display name. Defaults to a sensible per-kind
    /// derivation; user can rename via the sidebar.
    pub name: String,

    /// Protocol-specific configuration.
    pub kind: SourceKind,

    /// Whether this source is exposed via the MCP stdio mode.
    #[serde(default)]
    pub mcp_enabled: bool,

    /// Last successful scan timestamp (RFC3339).
    #[serde(default)]
    pub last_scan_at: Option<String>,

    /// Stats from the most recent scan.
    #[serde(default)]
    pub last_scan_stats: Option<ScanStats>,

    /// User-controlled ordering in the sidebar. Default: max(existing) + 1.
    #[serde(default)]
    pub sort_order: i32,

    /// Optional grouping. None = ungrouped (top level).
    #[serde(default)]
    pub group: Option<String>,
}

impl DataSource {
    /// Convenience constructor for the four kinds we ship today.
    /// New variants added in F43+ get their own helpers.
    pub fn new_local(path: PathBuf, recursive: bool) -> Self {
        let name = derive_local_name(&path);
        Self {
            id: Uuid::new_v4().to_string(),
            name,
            kind: SourceKind::Local {
                path,
                recursive,
                exclude_patterns: default_exclude_patterns(),
                max_file_size_mb: default_max_file_size_mb(),
            },
            mcp_enabled: false,
            last_scan_at: None,
            last_scan_stats: None,
            sort_order: 0,
            group: None,
        }
    }
}

/// Migrate a single legacy `WatchedFolder` to a `DataSource`. The
/// kind is inferred from the path string: `s3://` → S3, `http(s)://`
/// → Https, anything else → Local. Drive accounts aren't represented
/// as `WatchedFolder` today (they're tracked via gdrive_creds.rs
/// separately) so they don't appear here; future migration of
/// stored Drive watches into `DataSource` happens when the gdrive
/// adapter rewires through this abstraction.
pub fn migrate_watched_folder_to_source(wf: &WatchedFolder) -> DataSource {
    let trimmed = wf.path.trim_start().to_ascii_lowercase();
    let kind = if trimmed.starts_with("s3://") {
        SourceKind::S3 {
            url: wf.path.clone(),
        }
    } else if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        SourceKind::Https {
            url: wf.path.clone(),
        }
    } else {
        SourceKind::Local {
            path: PathBuf::from(&wf.path),
            recursive: wf.recursive,
            exclude_patterns: wf.exclude_patterns.clone(),
            max_file_size_mb: wf.max_file_size_mb,
        }
    };

    let name = derive_name_from_kind(&kind);

    DataSource {
        id: Uuid::new_v4().to_string(),
        name,
        kind,
        mcp_enabled: wf.mcp_enabled,
        last_scan_at: wf.last_scan_at.clone(),
        last_scan_stats: wf.last_scan_stats.clone(),
        sort_order: 0,
        group: None,
    }
}

/// Migrate a list of legacy WatchedFolders. Returns a Vec preserving
/// input order (callers can re-sort by `sort_order` if needed).
/// Idempotent at this layer; idempotency at the Config-load layer is
/// the caller's responsibility (skip migration if `sources` already
/// has an entry for the same path).
pub fn migrate_watched_folders_to_sources(folders: &[WatchedFolder]) -> Vec<DataSource> {
    folders
        .iter()
        .enumerate()
        .map(|(idx, wf)| {
            let mut src = migrate_watched_folder_to_source(wf);
            src.sort_order = idx as i32;
            src
        })
        .collect()
}

/// Default name derivation for each kind. Users can rename anytime
/// via F51's bookmark management UX.
fn derive_name_from_kind(kind: &SourceKind) -> String {
    match kind {
        SourceKind::Local { path, .. } => derive_local_name(path),
        SourceKind::Https { url } => derive_https_name(url),
        SourceKind::S3 { url } => derive_s3_name(url),
        SourceKind::GoogleDrive { account_id } => {
            if account_id == "default" {
                "Google Drive".to_string()
            } else {
                format!("Google Drive · {}", account_id)
            }
        }
        SourceKind::Sftp { host, base_path, .. } => {
            // Friendly default: "host:base_path", e.g.
            // "fileserver:/home/data". User can rename anytime.
            format!("{}:{}", host, base_path)
        }
        SourceKind::WebDav {
            server_url,
            base_path,
        } => {
            // Friendly default: just the host portion of the URL +
            // base_path, e.g. "nc.example.com:/Documents".
            let host = url::Url::parse(server_url)
                .ok()
                .and_then(|u| u.host_str().map(|s| s.to_string()))
                .unwrap_or_else(|| server_url.clone());
            format!("{}{}", host, base_path)
        }
        SourceKind::Dropbox { base_path } => {
            // Friendly default: "Dropbox · /path"
            if base_path.is_empty() || base_path == "/" {
                "Dropbox".to_string()
            } else {
                format!("Dropbox · {}", base_path)
            }
        }
    }
}

fn derive_local_name(path: &PathBuf) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .map(String::from)
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

fn derive_https_name(url: &str) -> String {
    // For an HTTPS URL, the friendly name is the host + last path
    // segment. e.g. "example.com/data.csv". Truncate at 40 chars
    // (chars, not bytes — the URL might be unicode).
    let trimmed = url.trim_start_matches("https://").trim_start_matches("http://");
    truncate_chars(trimmed, 40)
}

fn derive_s3_name(url: &str) -> String {
    // For S3, the friendly name is the bucket + first prefix segment.
    // e.g. "s3://my-bucket/data/" → "my-bucket/data". Truncate at 40.
    let stripped = url.trim_start_matches("s3://");
    let trimmed = stripped.trim_end_matches('/');
    truncate_chars(trimmed, 40)
}

/// Truncate a string to `max_chars` codepoints, appending a `…`
/// when truncation actually happens. Counts chars (not bytes) so
/// non-ASCII strings don't blow the visible width budget.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        s.to_string()
    } else {
        // Reserve 1 char for the ellipsis.
        let kept: String = s.chars().take(max_chars - 1).collect();
        format!("{}…", kept)
    }
}

// Defaults — these mirror the existing `default_exclude_patterns` +
// `default_max_file_size_mb` in config.rs. We duplicate them here so
// the sources module is self-contained; if config.rs ever exports
// them publicly, switch to the export.
fn default_exclude_patterns() -> Vec<String> {
    vec![
        ".DS_Store".to_string(),
        "__MACOSX".to_string(),
        ".git".to_string(),
        "node_modules".to_string(),
        ".venv".to_string(),
        "venv".to_string(),
        "target".to_string(),
        ".cache".to_string(),
        "~$*".to_string(),
        ".~lock*".to_string(),
    ]
}

fn default_max_file_size_mb() -> u64 {
    1024 // 1 GB
}

// ─── Tests ─────────────────────────────────────────────────────────────
//
// Per F42 SPEC §5.1, these cover the six fixture shapes any real user
// upgrade will hit. Each test exercises the migration end-to-end:
// build a representative WatchedFolder, migrate, assert the resulting
// DataSource has the expected kind / fields / friendly name.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchedFolder;

    fn make_wf(path: &str) -> WatchedFolder {
        WatchedFolder {
            path: path.to_string(),
            recursive: true,
            exclude_patterns: default_exclude_patterns(),
            max_file_size_mb: 1024,
            last_scan_at: None,
            last_scan_stats: None,
            mcp_enabled: false,
        }
    }

    #[test]
    fn fixture_1_empty_list_yields_empty_sources() {
        let result = migrate_watched_folders_to_sources(&[]);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn fixture_2_one_local_folder_becomes_local_source() {
        let wf = make_wf("/Users/me/Documents");
        let result = migrate_watched_folders_to_sources(&[wf]);
        assert_eq!(result.len(), 1);
        match &result[0].kind {
            SourceKind::Local { path, recursive, .. } => {
                assert_eq!(path, &PathBuf::from("/Users/me/Documents"));
                assert!(*recursive);
            }
            other => panic!("expected Local, got {:?}", other),
        }
        assert_eq!(result[0].name, "Documents");
        assert!(!result[0].id.is_empty());
        assert_eq!(result[0].sort_order, 0);
    }

    #[test]
    fn fixture_3_mixed_local_s3_https_get_correct_kinds() {
        let folders = vec![
            make_wf("/Users/me/Documents"),
            make_wf("s3://my-bucket/data/"),
            make_wf("https://example.com/sample.csv"),
        ];
        let result = migrate_watched_folders_to_sources(&folders);
        assert_eq!(result.len(), 3);

        assert!(matches!(result[0].kind, SourceKind::Local { .. }));
        assert!(matches!(result[1].kind, SourceKind::S3 { .. }));
        assert!(matches!(result[2].kind, SourceKind::Https { .. }));

        // sort_order preserves input position
        assert_eq!(result[0].sort_order, 0);
        assert_eq!(result[1].sort_order, 1);
        assert_eq!(result[2].sort_order, 2);

        // Each gets a unique ID
        let ids: std::collections::HashSet<&String> =
            result.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), 3, "all source IDs should be unique");
    }

    #[test]
    fn fixture_4_paths_with_unicode_and_spaces_preserve_exactly() {
        let wf = make_wf("/Users/me/Documentos prácticos/数据 folder");
        let result = migrate_watched_folders_to_sources(&[wf]);
        assert_eq!(result.len(), 1);
        match &result[0].kind {
            SourceKind::Local { path, .. } => {
                assert_eq!(
                    path,
                    &PathBuf::from("/Users/me/Documentos prácticos/数据 folder")
                );
            }
            other => panic!("expected Local, got {:?}", other),
        }
        // Friendly name uses the basename
        assert_eq!(result[0].name, "数据 folder");
    }

    #[test]
    fn fixture_5_s3_glob_url_becomes_s3_source() {
        let wf = make_wf("s3://my-bucket/year=2024/**/*.parquet");
        let result = migrate_watched_folders_to_sources(&[wf]);
        match &result[0].kind {
            SourceKind::S3 { url } => {
                assert_eq!(url, "s3://my-bucket/year=2024/**/*.parquet");
            }
            other => panic!("expected S3, got {:?}", other),
        }
    }

    #[test]
    fn fixture_6_settings_preserve_recursive_excludes_max_size_mcp() {
        let mut wf = make_wf("/Users/me/Documents");
        wf.recursive = false;
        wf.exclude_patterns = vec!["*.tmp".to_string(), "*.bak".to_string()];
        wf.max_file_size_mb = 512;
        wf.mcp_enabled = true;
        wf.last_scan_at = Some("2026-04-15T10:00:00Z".to_string());

        let result = migrate_watched_folders_to_sources(&[wf]);
        match &result[0].kind {
            SourceKind::Local {
                recursive,
                exclude_patterns,
                max_file_size_mb,
                ..
            } => {
                assert!(!recursive, "recursive flag should round-trip");
                assert_eq!(
                    exclude_patterns,
                    &vec!["*.tmp".to_string(), "*.bak".to_string()]
                );
                assert_eq!(*max_file_size_mb, 512);
            }
            other => panic!("expected Local, got {:?}", other),
        }

        assert!(result[0].mcp_enabled);
        assert_eq!(
            result[0].last_scan_at,
            Some("2026-04-15T10:00:00Z".to_string())
        );
    }

    #[test]
    fn https_friendly_name_truncates_at_40_chars() {
        let wf = make_wf(
            "https://very-long-subdomain.example.com/some/long/path/to/data.csv",
        );
        let result = migrate_watched_folders_to_sources(&[wf]);
        let name = &result[0].name;
        let chars = name.chars().count();
        assert!(chars <= 40, "name was {} chars: {}", chars, name);
        assert!(name.ends_with('…'));
    }

    #[test]
    fn s3_friendly_name_drops_scheme_and_trailing_slash() {
        let wf = make_wf("s3://my-bucket/data/");
        let result = migrate_watched_folders_to_sources(&[wf]);
        assert_eq!(result[0].name, "my-bucket/data");
    }

    #[test]
    fn serde_roundtrip_preserves_all_kinds() {
        // A DataSource serialized to JSON and back should equal itself
        // — the load-bearing property for Config persistence.
        let originals = vec![
            DataSource::new_local(PathBuf::from("/x"), true),
            DataSource {
                id: Uuid::new_v4().to_string(),
                name: "S3 source".to_string(),
                kind: SourceKind::S3 {
                    url: "s3://b/p/".to_string(),
                },
                mcp_enabled: false,
                last_scan_at: None,
                last_scan_stats: None,
                sort_order: 5,
                group: Some("Work".to_string()),
            },
            DataSource {
                id: Uuid::new_v4().to_string(),
                name: "Drive".to_string(),
                kind: SourceKind::GoogleDrive {
                    account_id: "default".to_string(),
                },
                mcp_enabled: true,
                last_scan_at: None,
                last_scan_stats: None,
                sort_order: 0,
                group: None,
            },
        ];

        for original in &originals {
            let json = serde_json::to_string(original).expect("serialize");
            let back: DataSource = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(original, &back);
        }
    }
}
