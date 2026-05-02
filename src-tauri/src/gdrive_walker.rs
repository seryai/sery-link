//! Recursive walker over Google Drive folders.
//!
//! Slice 2 of v0.6 / Phase 3c-3. Pure enumeration: takes a Drive
//! folder id, recurses through subfolders, returns a flat list of
//! files that are eligible for caching + scanning. The watch
//! command (slice 3) wraps this with progress events and the
//! download/scan orchestration.
//!
//! Filtering rules:
//!
//!   - **Folders**: recursed into, not returned. Only leaf files
//!     end up in `WalkResult::files`.
//!   - **Google-native types** (`application/vnd.google-apps.*`,
//!     except the folder type itself): skipped, listed in
//!     `WalkResult::skipped_native` so the UI can tell the user
//!     "12 Google Docs were skipped — export support arrives in
//!     a future release." Drive rejects `alt=media` for these.
//!   - **Trashed items**: already filtered out by the underlying
//!     `gdrive_api::list_folder` query.
//!
//! Cycle protection: a Drive item can have multiple parents (shared
//! folders, "Add to My Drive"), so a recursion that walked from any
//! ancestor could revisit a folder. We keep a `HashSet<String>` of
//! visited folder ids and skip on second hit. This also bounds the
//! traversal cost for users with extensively shared trees.
//!
//! Rate limiting: none here. Drive's quota is "1B requests/day,
//! 12k/100s/user" — generous enough that the sequential walk pattern
//! used here doesn't trip in practice. If users with very large
//! Drives hit 429, slice 5 of v0.6 (release polish) will add
//! exponential backoff.

use crate::error::Result;
use crate::gdrive_api::{self, DriveFile, FOLDER_MIME};
use crate::gdrive_cache;
use futures::future::BoxFuture;
use std::collections::HashSet;

/// Output of a `walk_folder` call. The watch command consumes
/// `files` directly; `skipped_native` and `folder_count` are
/// surfaced to the user in the progress UI.
#[derive(Debug, Default, Clone)]
pub struct WalkResult {
    /// Leaf files eligible for caching + scanning. Order is
    /// folder-major (depth-first by name); the watch command sorts
    /// however it likes after.
    pub files: Vec<DriveFile>,
    /// Files we found but couldn't cache because Drive doesn't
    /// expose their bytes via `alt=media` (Docs, Sheets, Forms,
    /// Sites, Drawings, etc.). Surfaced for "X items skipped" copy.
    pub skipped_native: Vec<DriveFile>,
    /// How many subfolders the walker entered (including the root).
    /// Used by progress UI to size the progress bar before walking
    /// can finish.
    pub folder_count: usize,
}

/// Recursively enumerate every file under `root_folder_id`. Returns
/// a flat `WalkResult`, with folders consumed during traversal and
/// Google-native types peeled out separately.
///
/// The function is sequential — one Drive list call at a time. This
/// is plenty for typical Drives (≤500 folders) and keeps memory
/// bounded; concurrent fan-out would race on the cycle-detection
/// set without much real-world benefit.
pub async fn walk_folder(account_id: &str, root_folder_id: &str) -> Result<WalkResult> {
    let mut result = WalkResult::default();
    let mut visited: HashSet<String> = HashSet::new();
    walk_recursive(account_id, root_folder_id, &mut result, &mut visited).await?;
    Ok(result)
}

/// Async recursion in Rust requires explicit `Box::pin` because the
/// future's size isn't statically known otherwise. `BoxFuture` is
/// the standard wrapper for this pattern.
fn walk_recursive<'a>(
    account_id: &'a str,
    folder_id: &'a str,
    result: &'a mut WalkResult,
    visited: &'a mut HashSet<String>,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        if !visited.insert(folder_id.to_string()) {
            // Already walked — Drive's multi-parent semantics could
            // otherwise loop us through a shared folder forever.
            return Ok(());
        }
        result.folder_count += 1;

        // Single Drive call per folder — we want both folders (to
        // recurse) and files (to keep) in one round-trip.
        let entries = gdrive_api::list_folder(account_id, folder_id, true).await?;

        for entry in entries {
            if entry.is_folder() {
                walk_recursive(account_id, &entry.id, result, visited).await?;
            } else if is_google_native(&entry.mime_type) {
                // Google-native types we have an /export mapping for
                // (Sheets in v0.6) flow through the regular cache
                // path — `gdrive_cache::download_if_stale` dispatches
                // on mime type. Native types without a mapping
                // (Forms, Drawings, Sites) get bucketed into
                // skipped_native so the UI can surface "X items
                // skipped" copy.
                if gdrive_cache::export_mime_for(&entry.mime_type).is_some() {
                    result.files.push(entry);
                } else {
                    result.skipped_native.push(entry);
                }
            } else {
                result.files.push(entry);
            }
        }

        Ok(())
    })
}

/// True for the Google-native mime types that Drive can't return
/// raw bytes for. The folder mime is in this namespace too but
/// folders are handled separately by the caller — this fn returns
/// false for FOLDER_MIME so the filter is `is_folder OR is_native`
/// rather than overlapping.
pub fn is_google_native(mime_type: &str) -> bool {
    mime_type.starts_with("application/vnd.google-apps.") && mime_type != FOLDER_MIME
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_google_native_doc_types() {
        assert!(is_google_native("application/vnd.google-apps.document"));
        assert!(is_google_native("application/vnd.google-apps.spreadsheet"));
        assert!(is_google_native("application/vnd.google-apps.presentation"));
        assert!(is_google_native("application/vnd.google-apps.form"));
        assert!(is_google_native("application/vnd.google-apps.drawing"));
    }

    #[test]
    fn sheets_have_export_mapping_so_walker_keeps_them() {
        // Sheets are still vnd.google-apps.* but cache knows how to
        // export them to .xlsx — so the walker MUST route them into
        // `files`, not `skipped_native`. This test pins that
        // contract; if export_mime_for ever loses the spreadsheet
        // entry, the walker behavior would silently regress.
        assert!(gdrive_cache::export_mime_for("application/vnd.google-apps.spreadsheet").is_some());
        assert!(gdrive_cache::export_mime_for("application/vnd.google-apps.form").is_none());
    }

    #[test]
    fn folder_mime_is_not_treated_as_native() {
        // The walker handles folders via `is_folder()`; if the
        // native filter also matched FOLDER_MIME we'd skip them
        // before recursing. Keep these branches disjoint.
        assert!(!is_google_native(FOLDER_MIME));
    }

    #[test]
    fn real_office_files_are_not_native() {
        // Office formats and parquet/csv/etc. are NOT
        // vnd.google-apps.*, so they must pass through.
        assert!(!is_google_native(
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        ));
        assert!(!is_google_native("text/csv"));
        assert!(!is_google_native("application/pdf"));
        assert!(!is_google_native("application/x-parquet"));
        assert!(!is_google_native(""));
    }

    #[test]
    fn walk_result_default_is_empty() {
        let r = WalkResult::default();
        assert!(r.files.is_empty());
        assert!(r.skipped_native.is_empty());
        assert_eq!(r.folder_count, 0);
    }
}
