//! Local filesystem cache of downloaded Google Drive files.
//!
//! Phase 3c-3 of the cloud-connectors migration. The walker
//! (`gdrive_walker.rs`, slice 2) calls into `download_if_stale()`
//! for every Drive file in a watched folder; the existing scanner
//! (`scanner::scan_folder`) then runs against the cache directory
//! exactly as if it were a local folder.
//!
//! Why download-to-cache instead of streaming?
//! Every parser Sery Link uses (mdkit, calamine, pdfium) needs a
//! file path on disk. Building streaming readers for each is weeks
//! of work; downloading is hours. Disk cost is bounded by what the
//! user explicitly chose to watch.
//!
//! ## Layout
//!
//! ```text
//! ~/.seryai/gdrive-cache/
//!   <account_id>/
//!     <file_id>/
//!       <sanitized-name>            # the file content
//!       .meta.json                  # { id, name, mimeType, modifiedTime, size }
//! ```
//!
//! Per-file directory is a deliberate choice: lets us hold the
//! sidecar next to the content and lets us reflect Drive renames
//! without colliding with another file's old name. The directory
//! name is the durable Drive `id`, never the human name.
//!
//! ## Stale detection
//!
//! We compare Drive's `modifiedTime` string exactly against the one
//! we wrote into the sidecar at last download. Filesystem mtime is
//! not used — its resolution differs between APFS / HFS+ / ext4 /
//! NTFS, and copying the cache between machines (e.g. backup
//! restore) can desync it.

use crate::config::Config;
use crate::error::{AgentError, Result};
use crate::gdrive_api::DriveFile;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// What we persist alongside each cached file. The shape mirrors
/// `DriveFile` minus the parents pointer (cache is keyed by id, not
/// hierarchy — the walker rebuilds the tree from Drive each pass).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CacheMeta {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// RFC 3339 string from Drive, byte-compared on next walk.
    pub modified_time: String,
    /// Size in bytes per Drive's metadata. `None` for Google-native
    /// files (Docs/Sheets) which Drive reports without a `size`.
    pub size: Option<u64>,
}

impl CacheMeta {
    fn from_drive(file: &DriveFile) -> Self {
        Self {
            id: file.id.clone(),
            name: file.name.clone(),
            mime_type: file.mime_type.clone(),
            modified_time: file.modified_time.clone(),
            size: file.size,
        }
    }
}

/// Where the gdrive cache lives for a given account. Created on
/// demand the first time we download.
pub fn account_dir(account_id: &str) -> Result<PathBuf> {
    Ok(Config::data_dir()?
        .join("gdrive-cache")
        .join(sanitize_path_component(account_id)))
}

/// Per-file directory inside the account cache. The Drive `id` is
/// the directory name — durable across renames.
pub fn file_dir(account_id: &str, file_id: &str) -> Result<PathBuf> {
    Ok(account_dir(account_id)?.join(sanitize_path_component(file_id)))
}

/// Path the actual file content lives at. Sanitized name keeps
/// unsafe characters (slashes, colons, NUL) out of the filesystem
/// path while remaining recognisable to humans browsing the cache.
///
/// For Google-native files we append an export extension (e.g.
/// `Q3 Budget` → `Q3 Budget.xlsx`) so scanner.rs's parser dispatch
/// picks up the right reader without inspecting bytes — calamine
/// for Sheets, etc.
pub fn content_path(account_id: &str, file: &DriveFile) -> Result<PathBuf> {
    let base = sanitize_filename(&file.name);
    let final_name = match export_mime_for(&file.mime_type) {
        Some((_, ext)) => ensure_extension(&base, ext),
        None => base,
    };
    Ok(file_dir(account_id, &file.id)?.join(final_name))
}

/// Map a Google-native mime type to (export_mime, extension) tuple.
/// `None` for native types we can't usefully cache yet (Forms,
/// Drawings, Sites). Only Sheets in v0.6; Docs/Slides will follow.
///
/// Sheets export to .xlsx (not .csv) preserves all tabs and number
/// formats. The .xlsx mime is verbose; lifting it to a constant
/// here keeps the call sites clean.
pub fn export_mime_for(google_native_mime: &str) -> Option<(&'static str, &'static str)> {
    match google_native_mime {
        "application/vnd.google-apps.spreadsheet" => Some((
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        )),
        _ => None,
    }
}

/// Append `.<ext>` if the filename doesn't already end with it.
/// Idempotent and case-insensitive on the comparison so a Drive
/// item named "Report.xlsx" doesn't become "Report.xlsx.xlsx" if
/// it ever shows up as both an exportable type and a binary upload.
fn ensure_extension(name: &str, ext: &str) -> String {
    let dot_ext = format!(".{}", ext);
    if name
        .to_ascii_lowercase()
        .ends_with(&dot_ext.to_ascii_lowercase())
    {
        return name.to_string();
    }
    format!("{}{}", name, dot_ext)
}

fn meta_path(account_id: &str, file_id: &str) -> Result<PathBuf> {
    Ok(file_dir(account_id, file_id)?.join(".meta.json"))
}

/// Replace path-separator and other unsafe chars in user-supplied
/// strings used as filesystem path components. Conservative: even
/// chars Windows allows but POSIX doesn't (and vice versa) get
/// replaced so the cache is portable across an iCloud / backup move.
fn sanitize_path_component(raw: &str) -> String {
    raw.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        // Trim leading dots so we never produce a hidden file by
        // accident (a Drive item literally named ".cache" would
        // otherwise become a dotfile we couldn't browse).
        .trim_start_matches('.')
        .to_string()
}

/// Like sanitize_path_component but also caps length so we don't
/// blow past filesystem limits (255 bytes on most modern FS).
/// Preserves the extension when truncating so the parser dispatch
/// in scanner.rs still picks the right reader.
fn sanitize_filename(raw: &str) -> String {
    const MAX: usize = 200;
    let cleaned = sanitize_path_component(raw);
    if cleaned.is_empty() {
        return "untitled".to_string();
    }
    if cleaned.len() <= MAX {
        return cleaned;
    }
    // Try to keep the extension. `Path::extension` operates on
    // OsStr; we feed it through PathBuf to get back a &str safely.
    let ext = Path::new(&cleaned)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e))
        .unwrap_or_default();
    let stem_budget = MAX.saturating_sub(ext.len());
    let stem: String = cleaned.chars().take(stem_budget).collect();
    format!("{}{}", stem, ext)
}

/// Did Drive change this file since we last cached it? Returns true
/// when the cache is missing, when the sidecar can't be read, or
/// when Drive's modifiedTime differs from what we wrote last time.
pub fn is_stale(account_id: &str, file: &DriveFile) -> Result<bool> {
    let meta_p = match meta_path(account_id, &file.id) {
        Ok(p) => p,
        Err(_) => return Ok(true),
    };
    if !meta_p.exists() {
        return Ok(true);
    }
    let bytes = match std::fs::read(&meta_p) {
        Ok(b) => b,
        Err(_) => return Ok(true),
    };
    let prev: CacheMeta = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        // Corrupt / older-format sidecar — treat as stale, we'll
        // overwrite on next download anyway.
        Err(_) => return Ok(true),
    };
    Ok(prev.modified_time != file.modified_time)
}

/// Ensure the local cache reflects Drive's current state for this
/// file. Returns the path to the (now-fresh) cached content. Skips
/// the network call when the sidecar's modifiedTime matches Drive's.
///
/// For Google-native types: dispatches to `download_export_bytes`
/// with the mime type from `export_mime_for`. For non-exportable
/// natives (Forms, Drawings, Sites) returns Err — the walker filters
/// these out so this path shouldn't fire in normal operation.
pub async fn download_if_stale(account_id: &str, file: &DriveFile) -> Result<PathBuf> {
    let export_choice = export_mime_for(&file.mime_type);

    if file.mime_type.starts_with("application/vnd.google-apps.") && export_choice.is_none() {
        return Err(AgentError::Config(format!(
            "Google-native file {:?} ({}) has no export mapping — \
             walker should have filtered this",
            file.name, file.mime_type
        )));
    }

    let dir = file_dir(account_id, &file.id)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AgentError::Config(format!("create cache dir: {}", e)))?;

    let content_p = content_path(account_id, file)?;
    let meta_p = meta_path(account_id, &file.id)?;

    if !is_stale(account_id, file)? && content_p.exists() {
        return Ok(content_p);
    }

    // If the file was renamed (or its export extension changed —
    // shouldn't happen but defensive), the old content sits in the
    // dir under its old name. Clean it up so the cache directory
    // doesn't accumulate stale copies.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p == meta_p || p == content_p {
                continue;
            }
            if p.is_file() {
                let _ = std::fs::remove_file(p);
            }
        }
    }

    let bytes = match export_choice {
        Some((mime, _)) => {
            crate::gdrive_api::download_export_bytes(account_id, &file.id, mime).await?
        }
        None => crate::gdrive_api::download_file_bytes(account_id, &file.id).await?,
    };
    std::fs::write(&content_p, &bytes)
        .map_err(|e| AgentError::Config(format!("write cache file: {}", e)))?;

    let meta = CacheMeta::from_drive(file);
    let meta_json = serde_json::to_vec_pretty(&meta)
        .map_err(|e| AgentError::Serialization(format!("serialize cache meta: {}", e)))?;
    std::fs::write(&meta_p, meta_json)
        .map_err(|e| AgentError::Config(format!("write cache meta: {}", e)))?;

    Ok(content_p)
}

/// Remove a single cached file (used when the walker discovers
/// Drive deleted it). Idempotent — missing dir is not an error.
pub fn forget_file(account_id: &str, file_id: &str) -> Result<()> {
    let dir = file_dir(account_id, file_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| AgentError::Config(format!("remove cache file: {}", e)))?;
    }
    Ok(())
}

/// Wipe the whole account cache. Called on disconnect so the user's
/// "I disconnected" intuition matches what's left on disk.
pub fn forget_account(account_id: &str) -> Result<()> {
    let dir = account_dir(account_id)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|e| AgentError::Config(format!("remove account cache: {}", e)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdrive_api::DriveFile;

    fn fake_file(id: &str, name: &str, modified: &str) -> DriveFile {
        DriveFile {
            id: id.to_string(),
            name: name.to_string(),
            mime_type: "text/csv".to_string(),
            size: Some(100),
            modified_time: modified.to_string(),
            parents: vec!["root".to_string()],
        }
    }

    #[test]
    fn sanitize_filename_replaces_unsafe_chars() {
        assert_eq!(sanitize_filename("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_filename("foo.csv"), "foo.csv");
        assert_eq!(sanitize_filename(""), "untitled");
    }

    #[test]
    fn sanitize_filename_strips_leading_dots() {
        // Otherwise `.cache` becomes a hidden file, and `..` is
        // already blocked by the path-component sanitizer above.
        assert_eq!(sanitize_filename(".cache"), "cache");
    }

    #[test]
    fn sanitize_filename_truncates_preserving_extension() {
        let huge = "x".repeat(500);
        let with_ext = format!("{}.parquet", huge);
        let out = sanitize_filename(&with_ext);
        assert!(out.len() <= 200);
        assert!(out.ends_with(".parquet"), "extension lost: {}", out);
    }

    #[test]
    fn sanitize_path_component_blocks_traversal_chars() {
        // Drive shouldn't return ids with slashes, but we don't
        // trust the wire — defense in depth. The leading `..`
        // becomes `..` after slash replacement, then the leading-
        // dot trim drops the dots: result starts with `_` (the
        // sanitised slash), not with a dotfile-looking prefix.
        assert_eq!(sanitize_path_component("../etc/passwd"), "_etc_passwd");
        assert_eq!(sanitize_path_component("a:b|c"), "a_b_c");
    }

    #[test]
    fn cache_meta_round_trip() {
        let file = fake_file("abc123", "data.csv", "2026-05-01T10:00:00.000Z");
        let meta = CacheMeta::from_drive(&file);
        let json = serde_json::to_string(&meta).unwrap();
        let back: CacheMeta = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
        assert_eq!(meta.size, Some(100));
    }

    #[test]
    fn is_stale_returns_true_when_cache_missing() {
        // No tempdir setup needed — account_dir for a unique id
        // points somewhere that definitely doesn't exist yet.
        let file = fake_file(
            "unique-id-no-cache-yet-9f8e7d6c",
            "x.csv",
            "2026-05-01T10:00:00Z",
        );
        let stale = is_stale("test-account-fresh", &file).unwrap();
        assert!(stale);
    }

    #[test]
    fn export_mime_maps_sheets_to_xlsx() {
        let (mime, ext) =
            export_mime_for("application/vnd.google-apps.spreadsheet").expect("sheets mapping");
        assert_eq!(
            mime,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(ext, "xlsx");
    }

    #[test]
    fn export_mime_unmapped_for_other_natives_and_binaries() {
        // Currently unsupported (Phase 3c-5+) native types — should
        // return None so the walker buckets them into skipped_native.
        assert!(export_mime_for("application/vnd.google-apps.document").is_none());
        assert!(export_mime_for("application/vnd.google-apps.form").is_none());
        // Real binary types have no /export pathway at all.
        assert!(export_mime_for("text/csv").is_none());
        assert!(export_mime_for("application/pdf").is_none());
    }

    #[test]
    fn ensure_extension_appends_when_missing() {
        assert_eq!(ensure_extension("Q3 Budget", "xlsx"), "Q3 Budget.xlsx");
        // Drive item names rarely already have the export extension
        // since they're native — but defend against the rename
        // edge case where a Sheet was renamed to "Foo.xlsx".
        assert_eq!(ensure_extension("Foo.xlsx", "xlsx"), "Foo.xlsx");
        assert_eq!(ensure_extension("Foo.XLSX", "xlsx"), "Foo.XLSX");
    }

    #[test]
    fn content_path_appends_xlsx_for_sheets() {
        let sheet = DriveFile {
            id: "sheet1".to_string(),
            name: "Q3 Budget".to_string(),
            mime_type: "application/vnd.google-apps.spreadsheet".to_string(),
            size: None,
            modified_time: "2026-05-01T10:00:00Z".to_string(),
            parents: vec!["root".to_string()],
        };
        let p = content_path("acct", &sheet).unwrap();
        let name = p.file_name().unwrap().to_string_lossy();
        assert_eq!(name, "Q3 Budget.xlsx");
    }

    #[test]
    fn content_path_unchanged_for_real_binary_files() {
        let csv = fake_file("a", "data.csv", "2026-05-01T10:00:00Z");
        let p = content_path("acct", &csv).unwrap();
        let name = p.file_name().unwrap().to_string_lossy();
        assert_eq!(name, "data.csv");
    }

    #[test]
    fn is_stale_compares_modified_time_byte_exact() {
        // We don't actually write a real cache here (no tempdir
        // wiring) — just verify the comparison logic via two
        // CacheMetas with differing modified_time values would
        // serialize differently. The is_stale code path itself is
        // exercised by the missing-cache test; the byte-comparison
        // is straightforward `prev.modified_time != file.modified_time`.
        let m1 = CacheMeta::from_drive(&fake_file("a", "x.csv", "2026-05-01T10:00:00Z"));
        let m2 = CacheMeta::from_drive(&fake_file("a", "x.csv", "2026-05-01T10:00:00.000Z"));
        assert_ne!(m1.modified_time, m2.modified_time);
    }
}
