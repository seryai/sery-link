//! F48 — Dropbox connection + listing + download.
//!
//! v0.7.0 ships Personal Access Token (PAT) auth only — the user
//! generates a token at https://www.dropbox.com/developers/apps
//! and pastes it into the form. OAuth + refresh-token flow comes
//! in a later slice (mirrors gdrive_oauth.rs's PKCE pattern, just
//! pointed at api.dropboxapi.com/oauth2). PAT is a worse UX but a
//! complete and honest delivery in one slice — better than a half-
//! shipped OAuth skeleton.
//!
//! All endpoints are HTTPS Bearer-authenticated. Uses the project's
//! existing reqwest client. Response shapes are JSON; we parse only
//! the fields we need.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const RPC_BASE: &str = "https://api.dropboxapi.com/2";
const CONTENT_BASE: &str = "https://content.dropboxapi.com/2";

/// Connection target + auth payload. Lives in the OS keychain
/// (dropbox_creds.rs) keyed on source_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropboxCredentials {
    /// Personal Access Token from the user's Dropbox app
    /// configuration page. Bearer-style; no expiry on PATs (unlike
    /// OAuth access tokens).
    pub access_token: String,
}

impl DropboxCredentials {
    pub fn is_valid(&self) -> bool {
        !self.access_token.trim().is_empty()
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("dropbox reqwest client builder")
}

/// Pre-flight: hit /users/get_current_account. Cheapest authenticated
/// endpoint; returns 200 with the user's account info on a valid
/// token, 401 on invalid. The full account JSON is discarded — we
/// just want to know the token works.
pub async fn test_credentials(creds: &DropboxCredentials) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "Dropbox credentials need an access token".to_string(),
        ));
    }
    let resp = client()
        .post(format!("{RPC_BASE}/users/get_current_account"))
        .bearer_auth(creds.access_token.trim())
        // Even though there's no body, Dropbox's RPC endpoints
        // require the Content-Type to NOT be set (or be empty);
        // sending application/json with an empty body 400s.
        // reqwest's .body("null") does the right thing here.
        .header("Content-Type", "application/json")
        .body("null")
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("Dropbox auth check: {e}"))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "Dropbox token rejected (HTTP {status}): {body}"
        )));
    }
    Ok(())
}

/// One file entry from a recursive listing.
#[derive(Debug, Clone)]
pub struct DropboxFile {
    /// Path inside the user's Dropbox, starting with `/`.
    pub path_lower: String,
    pub size: u64,
    /// Server-side modification time as RFC 3339 string. Used for
    /// future incremental-sync manifest comparisons; unused today.
    pub server_modified: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ListFolderResponse {
    entries: Vec<ListFolderEntry>,
    cursor: String,
    has_more: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = ".tag", rename_all = "snake_case")]
enum ListFolderEntry {
    File {
        path_lower: String,
        size: u64,
        #[serde(default)]
        server_modified: Option<String>,
    },
    Folder {
        // We don't need the folder name; the recursive listing
        // surfaces files as separate File entries.
        #[allow(dead_code)]
        path_lower: String,
    },
    Deleted {
        // Deleted entries appear if recursive=true and the user has
        // deleted files we previously listed; ignore.
        #[allow(dead_code)]
        path_lower: String,
    },
}

#[derive(Debug, Serialize)]
struct ListFolderRequest<'a> {
    path: &'a str,
    recursive: bool,
    include_media_info: bool,
    include_deleted: bool,
    include_has_explicit_shared_members: bool,
    include_mounted_folders: bool,
    limit: u32,
}

#[derive(Debug, Serialize)]
struct ContinueRequest<'a> {
    cursor: &'a str,
}

/// List every file under `base_path` (recursive). Bounded by
/// `max_files` via early termination of the cursor pagination.
pub async fn list_recursive(
    creds: &DropboxCredentials,
    base_path: &str,
    max_files: usize,
) -> Result<Vec<DropboxFile>> {
    let c = client();
    let mut out: Vec<DropboxFile> = Vec::new();

    // Dropbox quirk: the root folder is the empty string, NOT "/".
    // Other paths are absolute starting with "/" but no trailing "/".
    let normalized = if base_path == "/" || base_path.is_empty() {
        "".to_string()
    } else {
        base_path.trim_end_matches('/').to_string()
    };

    // First page.
    let body = serde_json::to_string(&ListFolderRequest {
        path: &normalized,
        recursive: true,
        include_media_info: false,
        include_deleted: false,
        include_has_explicit_shared_members: false,
        include_mounted_folders: true,
        limit: 2000,
    })
    .map_err(|e| AgentError::Serialization(format!("encode list req: {e}")))?;

    let mut page: ListFolderResponse = c
        .post(format!("{RPC_BASE}/files/list_folder"))
        .bearer_auth(creds.access_token.trim())
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("Dropbox list_folder: {e}")))?
        .error_for_status()
        .map_err(|e| AgentError::Network(format!("Dropbox list_folder: {e}")))?
        .json::<ListFolderResponse>()
        .await
        .map_err(|e| AgentError::Serialization(format!("parse list resp: {e}")))?;

    loop {
        for entry in page.entries {
            if out.len() >= max_files {
                break;
            }
            if let ListFolderEntry::File {
                path_lower,
                size,
                server_modified,
            } = entry
            {
                out.push(DropboxFile {
                    path_lower,
                    size,
                    server_modified,
                });
            }
        }
        if !page.has_more || out.len() >= max_files {
            break;
        }
        // Continue.
        let cont_body = serde_json::to_string(&ContinueRequest {
            cursor: &page.cursor,
        })
        .map_err(|e| AgentError::Serialization(format!("encode cont req: {e}")))?;
        page = c
            .post(format!("{RPC_BASE}/files/list_folder/continue"))
            .bearer_auth(creds.access_token.trim())
            .header("Content-Type", "application/json")
            .body(cont_body)
            .send()
            .await
            .map_err(|e| {
                AgentError::Network(format!("Dropbox list_folder/continue: {e}"))
            })?
            .error_for_status()
            .map_err(|e| {
                AgentError::Network(format!("Dropbox list_folder/continue: {e}"))
            })?
            .json::<ListFolderResponse>()
            .await
            .map_err(|e| {
                AgentError::Serialization(format!("parse cont resp: {e}"))
            })?;
    }

    Ok(out)
}

/// Download a single Dropbox file to a local path.
pub async fn download_file(
    creds: &DropboxCredentials,
    path_lower: &str,
    local_path: &Path,
) -> Result<u64> {
    let c = client();

    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            AgentError::FileSystem(format!(
                "create cache dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    // Dropbox content endpoints take the args as a header (NOT a
    // body) — JSON-encoded. Body is empty for downloads.
    let arg = serde_json::json!({ "path": path_lower }).to_string();
    let resp = c
        .post(format!("{CONTENT_BASE}/files/download"))
        .bearer_auth(creds.access_token.trim())
        .header("Dropbox-API-Arg", arg)
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("Dropbox download {path_lower}: {e}"))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "Dropbox download {path_lower}: HTTP {status}: {body}"
        )));
    }

    let bytes = resp.bytes().await.map_err(|e| {
        AgentError::Network(format!("read body: {e}"))
    })?;
    let mut local = tokio::fs::File::create(local_path).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;
    local.write_all(&bytes).await.map_err(|e| {
        AgentError::FileSystem(format!("write local: {e}"))
    })?;
    local.flush().await.map_err(|e| {
        AgentError::FileSystem(format!("flush local: {e}"))
    })?;
    Ok(bytes.len() as u64)
}

/// Where the Dropbox cache lives for a given source. Mirror of
/// sftp::cache_dir_for_source / webdav::cache_dir_for_source.
pub fn cache_dir_for_source(source_id: &str) -> Result<PathBuf> {
    Ok(crate::config::Config::data_dir()?
        .join("dropbox-cache")
        .join(sanitize_path_component(source_id)))
}

fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' => '_',
            _ => c,
        })
        .collect()
}

/// Walk + download every supported file under `base_path`. Skips
/// files whose size + server_modified match the previous walk's
/// manifest (incremental sync).
pub async fn walk_and_download(
    creds: &DropboxCredentials,
    base_path: &str,
    source_id: &str,
) -> Result<(PathBuf, usize)> {
    use crate::sync_manifest::SyncManifest;
    use std::collections::HashSet;
    const MAX_DROPBOX_FILES: usize = 10_000;

    let cache_dir = cache_dir_for_source(source_id)?;
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create dropbox cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;

    let mut manifest = SyncManifest::load(&cache_dir);

    let listing = list_recursive(creds, base_path, MAX_DROPBOX_FILES).await?;
    let mut downloaded = 0usize;
    let mut current_keys: HashSet<String> = HashSet::new();

    let base_normalized = if base_path == "/" || base_path.is_empty() {
        "".to_string()
    } else {
        base_path.trim_end_matches('/').to_lowercase()
    };

    for file in listing.iter() {
        let path = PathBuf::from(&file.path_lower);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let supported = ext
            .as_deref()
            .map(crate::scanner::is_supported_ext)
            .unwrap_or(false);
        if !supported {
            continue;
        }

        let rel_str = if base_normalized.is_empty() {
            file.path_lower.trim_start_matches('/').to_string()
        } else if let Some(s) = file.path_lower.strip_prefix(&base_normalized) {
            s.trim_start_matches('/').to_string()
        } else {
            match path.file_name().and_then(|n| n.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            }
        };
        let local_path = cache_dir.join(&rel_str);

        // Key = path_lower (Dropbox guarantees stability).
        // Mtime marker = server_modified (RFC 3339 string from API);
        // empty sentinel when missing.
        let key = file.path_lower.clone();
        let mtime_marker = file
            .server_modified
            .clone()
            .unwrap_or_else(|| "".to_string());
        current_keys.insert(key.clone());

        if !manifest.needs_download(&key, file.size, &mtime_marker)
            && local_path.exists()
        {
            continue;
        }

        match download_file(creds, &file.path_lower, &local_path).await {
            Ok(_) => {
                downloaded += 1;
                manifest.record(key, file.size, mtime_marker);
            }
            Err(e) => {
                eprintln!(
                    "[dropbox] download failed for {}: {} — skipping",
                    file.path_lower, e
                );
            }
        }
    }

    // Drop stale entries + their cached files. Use the same path-
    // resolution rule we used on the way in.
    let stale = manifest.drop_missing(&current_keys);
    for stale_key in &stale {
        let rel_str = if base_normalized.is_empty() {
            stale_key.trim_start_matches('/').to_string()
        } else if let Some(s) = stale_key.strip_prefix(&base_normalized) {
            s.trim_start_matches('/').to_string()
        } else {
            continue;
        };
        let local = cache_dir.join(&rel_str);
        let _ = tokio::fs::remove_file(&local).await;
    }

    let _ = manifest.save(&cache_dir);

    Ok((cache_dir, downloaded))
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_rejects_empty_token() {
        let creds = DropboxCredentials {
            access_token: "".to_string(),
        };
        assert!(!creds.is_valid());

        let creds_ws = DropboxCredentials {
            access_token: "  ".to_string(),
        };
        assert!(!creds_ws.is_valid());
    }

    #[test]
    fn is_valid_accepts_real_looking_token() {
        let creds = DropboxCredentials {
            access_token: "sl.B1234567890abcdef".to_string(),
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn list_response_parses_file_entry() {
        // Real-shape Dropbox API JSON, trimmed to the fields we use.
        let json = r#"{
            "entries": [
                {
                    ".tag": "file",
                    "path_lower": "/data/sales.csv",
                    "size": 12345,
                    "server_modified": "2026-01-15T10:00:00Z"
                },
                {
                    ".tag": "folder",
                    "path_lower": "/data/subdir"
                }
            ],
            "cursor": "AAB...",
            "has_more": false
        }"#;
        let resp: ListFolderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.entries.len(), 2);
        assert!(matches!(resp.entries[0], ListFolderEntry::File { .. }));
        assert!(matches!(resp.entries[1], ListFolderEntry::Folder { .. }));
        assert!(!resp.has_more);
    }

    #[test]
    fn list_response_handles_deleted_entries() {
        // Dropbox can return deleted entries in some configurations;
        // they need to deserialize but we filter them out.
        let json = r#"{
            "entries": [
                {
                    ".tag": "deleted",
                    "path_lower": "/old.csv"
                }
            ],
            "cursor": "X",
            "has_more": false
        }"#;
        let resp: ListFolderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.entries.len(), 1);
        assert!(matches!(resp.entries[0], ListFolderEntry::Deleted { .. }));
    }
}
