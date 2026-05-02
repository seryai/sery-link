//! Google Drive REST API client — Phase 3c-1 of the cloud-connectors
//! migration. See `datalake/SPEC_CLOUD_CONNECTORS_MIGRATION.md`.
//!
//! Wraps the small slice of the Drive v3 API we actually need:
//!
//!   - `list_root_folders()`   — top-level folders for the picker UI
//!   - `list_folder()`         — children of a chosen folder, used by
//!                               the scan walker
//!   - `download_file_to()`    — stream a binary file to a path
//!   - `download_export_to()`  — same, but for Google-native exports
//!                               (Sheets → .xlsx, etc.)
//!
//! Auth: every method takes a *fresh* access token. Token refresh is
//! handled by `with_fresh_token()` here, which loads tokens from
//! `gdrive_creds`, refreshes via `gdrive_oauth::refresh_token` if
//! within 60 seconds of expiry, persists the rotation, and hands the
//! current access token to the closure. Callers shouldn't worry about
//! token freshness.
//!
//! ## What we DON'T do
//!
//! Drive's quota model is "per-user-per-100s." This client doesn't
//! implement client-side rate limiting; if Google returns 429, the
//! caller surfaces it. The scan walker (Phase 3c-3) will apply
//! exponential backoff at its layer.
//!
//! Drive supports many file types we don't index (Google Docs, Forms,
//! Sites). The scan filter for "what counts as a dataset" lives in
//! Phase 3c-3, not here. This client returns whatever Drive returns;
//! it's the caller's job to filter.

use crate::error::{AgentError, Result};
use crate::gdrive_creds;
use crate::gdrive_oauth;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::io::AsyncWriteExt;

const API_BASE: &str = "https://www.googleapis.com/drive/v3";

/// Mime type Drive uses for folders. Filter on this in `q=` queries
/// to distinguish folders from files.
pub const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

// ── Wire types ─────────────────────────────────────────────────────

/// One file or folder, as Drive returns it. The `fields=` param in
/// every request determines which keys are populated; we always
/// request the same set so deserialization is consistent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DriveFile {
    pub id: String,
    pub name: String,
    #[serde(default, rename = "mimeType")]
    pub mime_type: String,
    /// File size in bytes, as a string in the wire format. Drive
    /// returns this only for binary files; Google-native types
    /// (Docs, Sheets) have no size. Stored as `Option<u64>` after
    /// parsing.
    #[serde(default, rename = "size", deserialize_with = "deserialize_optional_u64")]
    pub size: Option<u64>,
    /// RFC 3339 timestamp of last modification. Stored as the raw
    /// string; the scanner parses to `chrono::DateTime` if needed.
    #[serde(default, rename = "modifiedTime")]
    pub modified_time: String,
    /// Parent folder IDs. A file usually has one parent; shared
    /// items can have multiple. Empty for top-level items in
    /// "My Drive."
    #[serde(default)]
    pub parents: Vec<String>,
}

impl DriveFile {
    pub fn is_folder(&self) -> bool {
        self.mime_type == FOLDER_MIME
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ListResponse {
    #[serde(default, rename = "nextPageToken")]
    next_page_token: Option<String>,
    #[serde(default)]
    files: Vec<DriveFile>,
}

/// Drive API returns numeric fields as strings ("12345" not 12345).
/// Custom deserializer handles both string and missing-field cases.
fn deserialize_optional_u64<'de, D>(deserializer: D) -> std::result::Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Unexpected};
    let raw: Option<String> = Option::deserialize(deserializer)?;
    match raw {
        Some(s) if !s.is_empty() => s.parse::<u64>().map(Some).map_err(|_| {
            de::Error::invalid_value(Unexpected::Str(&s), &"a u64 size")
        }),
        _ => Ok(None),
    }
}

// ── Token-refresh wrapper ──────────────────────────────────────────

/// Run a closure with a fresh access token. Loads stored tokens,
/// refreshes if expired, persists any rotation, and hands the
/// current access token to the closure. Closures should NOT keep
/// the token longer than one HTTP call — by the next call it may
/// have rotated.
async fn with_fresh_token<F, Fut, T>(account_id: &str, op: F) -> Result<T>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut tokens = gdrive_creds::load(account_id)?
        .ok_or_else(|| AgentError::Config(
            "No Google Drive account connected. Run start_gdrive_oauth first.".to_string()
        ))?;

    if !tokens.is_fresh() {
        let cid = gdrive_oauth::client_id().ok_or_else(|| {
            AgentError::Config(
                "Google Drive integration not configured for this build.".to_string(),
            )
        })?;
        let csecret = gdrive_oauth::client_secret().ok_or_else(|| {
            AgentError::Config(
                "Google Drive integration is missing GOOGLE_OAUTH_CLIENT_SECRET.".to_string(),
            )
        })?;
        let resp = gdrive_oauth::refresh_token(cid, csecret, &tokens.refresh_token).await?;
        tokens.merge_refresh_response(&resp);
        gdrive_creds::save(account_id, &tokens)?;
    }

    op(tokens.access_token).await
}

// ── Public API ─────────────────────────────────────────────────────

/// Common `fields=` projection for list responses. Keep in sync with
/// `DriveFile` — adding a field there means adding it here too,
/// otherwise the wire response will lack it and we'll get default
/// values.
const LIST_FIELDS: &str = "nextPageToken,files(id,name,mimeType,size,modifiedTime,parents)";

/// List the user's top-level folders ("My Drive" root level). Used
/// to populate the folder-picker UI in Phase 3c-2.
///
/// "Top level" here means the folders directly under My Drive root.
/// Drive's API expresses this as `q='root' in parents` — the literal
/// string `"root"` is a Drive-recognized alias for the user's My
/// Drive folder ID.
pub async fn list_root_folders(account_id: &str) -> Result<Vec<DriveFile>> {
    list_folder_inner(account_id, "root", true).await
}

/// List the children of a specific folder. `include_folders=true`
/// returns both files and subfolders; the picker UI uses true,
/// the leaf scan walker uses false.
pub async fn list_folder(
    account_id: &str,
    folder_id: &str,
    include_folders: bool,
) -> Result<Vec<DriveFile>> {
    list_folder_inner(account_id, folder_id, include_folders).await
}

async fn list_folder_inner(
    account_id: &str,
    folder_id: &str,
    include_folders: bool,
) -> Result<Vec<DriveFile>> {
    with_fresh_token(account_id, |access_token| async move {
        let client = reqwest::Client::new();
        let mut all = Vec::new();
        let mut page_token: Option<String> = None;

        // `q=` filter. Trashed files are excluded; the include_folders
        // flag controls whether we also exclude folders.
        let folder_filter = if include_folders {
            String::new()
        } else {
            format!(" and mimeType != '{}'", FOLDER_MIME)
        };
        let q = format!("'{}' in parents and trashed = false{}", folder_id, folder_filter);

        loop {
            let mut req = client
                .get(format!("{}/files", API_BASE))
                .bearer_auth(&access_token)
                .query(&[
                    ("q", q.as_str()),
                    ("fields", LIST_FIELDS),
                    ("pageSize", "1000"),
                    ("orderBy", "name"),
                ]);
            if let Some(tok) = &page_token {
                req = req.query(&[("pageToken", tok.as_str())]);
            }

            let resp = req
                .send()
                .await
                .map_err(|e| AgentError::Network(format!("drive list folder: {}", e)))?;
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                return Err(AgentError::Network(format!(
                    "drive list folder {}: {}",
                    status, body
                )));
            }

            let parsed: ListResponse = resp
                .json()
                .await
                .map_err(|e| AgentError::Serialization(format!("drive list parse: {}", e)))?;
            all.extend(parsed.files);

            match parsed.next_page_token {
                Some(t) if !t.is_empty() => page_token = Some(t),
                _ => break,
            }
        }

        Ok(all)
    })
    .await
}

/// Stream a Drive file's bytes directly to disk. Returns the number
/// of bytes written.
///
/// The previous `download_file_bytes` returned a `Vec<u8>`, which
/// meant the entire file lived in memory before being written —
/// fine for a 100 KB CSV, fatal for a 5 GB video the user happens
/// to have in their Drive (jetsam OOM-kills the desktop process,
/// no Rust panic surfaces). This streams in 64 KiB chunks instead,
/// keeping the memory footprint bounded regardless of file size.
///
/// `max_bytes` lets the caller cap the download — when the response
/// streams past the cap we abort + delete the partial file and
/// return a Network error the caller can show to the user. Pass
/// `u64::MAX` to disable the cap.
pub async fn download_file_to(
    account_id: &str,
    file_id: &str,
    dest: &Path,
    max_bytes: u64,
) -> Result<u64> {
    let dest = dest.to_path_buf();
    with_fresh_token(account_id, |access_token| async move {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/files/{}", API_BASE, file_id))
            .bearer_auth(&access_token)
            .query(&[("alt", "media")])
            .send()
            .await
            .map_err(|e| AgentError::Network(format!("drive download: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Network(format!(
                "drive download {}: {}",
                status, body
            )));
        }

        stream_response_to_file(resp, &dest, max_bytes).await
    })
    .await
}

/// Stream a Google-native file's exported bytes to disk. Drive
/// rejects `alt=media` for native types and instead exposes
/// `/export?mimeType=...` to convert them on the fly. Sheets export
/// to .xlsx preserves all tabs; the older CSV export was lossy
/// (single sheet only) so we don't use it.
///
/// Picking which `export_mime` to request is the caller's job — see
/// `gdrive_cache::export_mime_for` for the canonical mapping.
pub async fn download_export_to(
    account_id: &str,
    file_id: &str,
    export_mime: &str,
    dest: &Path,
    max_bytes: u64,
) -> Result<u64> {
    let export_mime = export_mime.to_string();
    let dest = dest.to_path_buf();
    with_fresh_token(account_id, |access_token| async move {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/files/{}/export", API_BASE, file_id))
            .bearer_auth(&access_token)
            .query(&[("mimeType", export_mime.as_str())])
            .send()
            .await
            .map_err(|e| AgentError::Network(format!("drive export: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Network(format!(
                "drive export {}: {}",
                status, body
            )));
        }

        stream_response_to_file(resp, &dest, max_bytes).await
    })
    .await
}

/// Shared streaming sink for `download_file_to` /
/// `download_export_to`. Reads `resp` chunk-by-chunk and writes to
/// `dest`. Aborts + removes the partial file when the byte count
/// crosses `max_bytes`.
async fn stream_response_to_file(
    resp: reqwest::Response,
    dest: &Path,
    max_bytes: u64,
) -> Result<u64> {
    let mut file = tokio::fs::File::create(dest)
        .await
        .map_err(|e| AgentError::Config(format!("create cache file: {}", e)))?;

    let mut total: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk =
            chunk.map_err(|e| AgentError::Network(format!("drive chunk: {}", e)))?;
        total = total.saturating_add(chunk.len() as u64);
        if total > max_bytes {
            // Best-effort cleanup so we don't leave a partial file
            // behind. Closing first matters on Windows where the
            // open handle would block delete.
            drop(file);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(AgentError::Network(format!(
                "drive file exceeded {} byte cap (got {}+), skipped",
                max_bytes, total
            )));
        }
        file.write_all(&chunk)
            .await
            .map_err(|e| AgentError::Config(format!("write chunk: {}", e)))?;
    }

    file.flush()
        .await
        .map_err(|e| AgentError::Config(format!("flush cache file: {}", e)))?;
    Ok(total)
}

// ── Unit tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_typical_list_response() {
        let json = r#"{
            "nextPageToken": "next-page-1",
            "files": [
                {
                    "id": "1abc",
                    "name": "Folder A",
                    "mimeType": "application/vnd.google-apps.folder",
                    "modifiedTime": "2026-04-01T12:00:00.000Z",
                    "parents": ["root"]
                },
                {
                    "id": "2xyz",
                    "name": "data.csv",
                    "mimeType": "text/csv",
                    "size": "12345",
                    "modifiedTime": "2026-04-02T15:30:00.000Z",
                    "parents": ["1abc"]
                }
            ]
        }"#;
        let parsed: ListResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(parsed.next_page_token, Some("next-page-1".to_string()));
        assert_eq!(parsed.files.len(), 2);

        let folder = &parsed.files[0];
        assert!(folder.is_folder());
        assert_eq!(folder.size, None); // folders have no size
        assert_eq!(folder.parents, vec!["root"]);

        let file = &parsed.files[1];
        assert!(!file.is_folder());
        assert_eq!(file.size, Some(12_345));
        assert_eq!(file.modified_time, "2026-04-02T15:30:00.000Z");
    }

    #[test]
    fn parses_response_without_next_page_token() {
        let json = r#"{"files": []}"#;
        let parsed: ListResponse = serde_json::from_str(json).expect("parse");
        assert!(parsed.next_page_token.is_none());
        assert!(parsed.files.is_empty());
    }

    #[test]
    fn parses_response_with_missing_optional_fields() {
        // Drive sometimes omits `size` for binary files in the trash,
        // and `parents` for top-level items. Make sure those still
        // parse cleanly via #[serde(default)].
        let json = r#"{
            "files": [
                {
                    "id": "x",
                    "name": "y",
                    "mimeType": "text/plain",
                    "modifiedTime": "2026-01-01T00:00:00.000Z"
                }
            ]
        }"#;
        let parsed: ListResponse = serde_json::from_str(json).expect("parse");
        let f = &parsed.files[0];
        assert_eq!(f.size, None);
        assert!(f.parents.is_empty());
    }

    #[test]
    fn parses_size_as_string_or_missing() {
        let cases = vec![
            (r#"{"id":"a","name":"b","mimeType":"text/csv","modifiedTime":"x","size":"100"}"#, Some(100)),
            (r#"{"id":"a","name":"b","mimeType":"text/csv","modifiedTime":"x","size":""}"#, None),
            (r#"{"id":"a","name":"b","mimeType":"text/csv","modifiedTime":"x"}"#, None),
        ];
        for (json, expected) in cases {
            let f: DriveFile = serde_json::from_str(json).expect("parse");
            assert_eq!(f.size, expected, "input: {}", json);
        }
    }

    #[test]
    fn folder_mime_constant_matches_drive() {
        // Sanity: the constant we use to detect folders must match
        // Drive's actual mime type. Locked here so a typo doesn't
        // silently make every folder look like a file.
        assert_eq!(FOLDER_MIME, "application/vnd.google-apps.folder");
    }
}
