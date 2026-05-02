//! Google Drive REST API client — Phase 3c-1 of the cloud-connectors
//! migration. See `datalake/SPEC_CLOUD_CONNECTORS_MIGRATION.md`.
//!
//! Wraps the small slice of the Drive v3 API we actually need:
//!
//!   - `list_root_folders()`   — top-level folders for the picker UI
//!   - `list_folder()`         — children of a chosen folder, used by
//!                               the scan walker (Phase 3c-3)
//!   - `get_file_metadata()`   — single-file lookup, used during scan
//!   - `download_file_bytes()` — raw content fetch for query path
//!                               (Phase 3c-4)
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
use serde::{Deserialize, Serialize};

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

/// Fetch a single file's metadata by ID. Used during scans when we
/// have an ID but want full DriveFile fields.
pub async fn get_file_metadata(account_id: &str, file_id: &str) -> Result<DriveFile> {
    with_fresh_token(account_id, |access_token| async move {
        let client = reqwest::Client::new();
        let resp = client
            .get(format!("{}/files/{}", API_BASE, file_id))
            .bearer_auth(&access_token)
            .query(&[("fields", "id,name,mimeType,size,modifiedTime,parents")])
            .send()
            .await
            .map_err(|e| AgentError::Network(format!("drive get file: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Network(format!(
                "drive get file {}: {}",
                status, body
            )));
        }

        resp.json::<DriveFile>()
            .await
            .map_err(|e| AgentError::Serialization(format!("drive file parse: {}", e)))
    })
    .await
}

/// Download a file's bytes by ID. Used by the query path (Phase 3c-4)
/// to feed Drive content into DuckDB's in-memory readers. Streams in
/// memory — caller is responsible for size limits at the scan layer.
pub async fn download_file_bytes(account_id: &str, file_id: &str) -> Result<Vec<u8>> {
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

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AgentError::Network(format!("drive download body: {}", e)))?;
        Ok(bytes.to_vec())
    })
    .await
}

/// Download a Google-native file (Sheets, Docs, …) by exporting it
/// to a parseable format. Drive rejects `alt=media` for native types
/// and instead exposes `/export?mimeType=...` to convert them on the
/// fly. The Sheets export to .xlsx preserves all tabs; the older
/// CSV export was lossy (single sheet only) so we don't use it.
///
/// Picking which `export_mime` to request is the caller's job — see
/// `gdrive_cache::export_mime_for` for the canonical mapping.
pub async fn download_export_bytes(
    account_id: &str,
    file_id: &str,
    export_mime: &str,
) -> Result<Vec<u8>> {
    let export_mime = export_mime.to_string();
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

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| AgentError::Network(format!("drive export body: {}", e)))?;
        Ok(bytes.to_vec())
    })
    .await
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
