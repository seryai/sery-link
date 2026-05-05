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
///
/// Two auth shapes share this struct:
///   - PAT: just `access_token`; `refresh_token` + `expires_at` are
///     None. PATs don't expire, so refresh is a no-op.
///   - OAuth: `access_token` (4-hour validity) + `refresh_token`
///     (long-lived) + `expires_at` (RFC 3339). `ensure_fresh` rotates
///     the access_token ~60s before expiry.
///
/// `#[serde(default)]` on the OAuth fields keeps older PAT-only
/// entries in the keychain backward-compatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropboxCredentials {
    pub access_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

impl DropboxCredentials {
    pub fn is_valid(&self) -> bool {
        !self.access_token.trim().is_empty()
    }

    /// True if this entry was obtained via OAuth (and therefore can
    /// be refreshed). PAT entries have no refresh_token.
    pub fn is_oauth(&self) -> bool {
        self.refresh_token
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
    }

    /// True for OAuth entries within 60s of expiry. Always false for
    /// PAT entries (they don't expire).
    pub fn is_expiring(&self) -> bool {
        if !self.is_oauth() {
            return false;
        }
        match self.expires_at.as_deref() {
            Some(s) => match chrono::DateTime::parse_from_rfc3339(s) {
                Ok(t) => {
                    chrono::Utc::now() + chrono::Duration::seconds(60) >= t
                }
                Err(_) => true,
            },
            None => true,
        }
    }
}

/// If this is an OAuth entry that's about to expire, refresh the
/// access_token in place and persist the new tokens to the keychain.
/// PAT entries are a no-op.
pub async fn ensure_fresh(
    creds: &mut DropboxCredentials,
    source_id: &str,
) -> Result<()> {
    if !creds.is_expiring() {
        return Ok(());
    }
    let mut tokens = crate::dropbox_oauth::DropboxOAuthTokens {
        access_token: creds.access_token.clone(),
        refresh_token: creds.refresh_token.clone().unwrap_or_default(),
        expires_at: creds.expires_at.clone().unwrap_or_default(),
    };
    crate::dropbox_oauth::refresh_access_token(&mut tokens).await?;
    creds.access_token = tokens.access_token;
    creds.refresh_token = Some(tokens.refresh_token);
    creds.expires_at = Some(tokens.expires_at);
    // Persist refreshed tokens so we don't refresh again on the next
    // call. Best-effort: a keychain write failure here just means
    // we'll refresh again next time, which is harmless.
    let _ = crate::dropbox_creds::save(source_id, creds);
    Ok(())
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

/// Per-byte progress for a single download. Fires per chunk with
/// (bytes_so_far, total_bytes_hint). `total_bytes_hint` is best-
/// effort: it's the Content-Length the server reported, or 0 if
/// the server didn't send one. Callers can use a separately-known
/// size (e.g., the listing entry) and ignore the hint.
pub type ByteProgressCb = std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Download a single Dropbox file to a local path.
pub async fn download_file(
    creds: &DropboxCredentials,
    path_lower: &str,
    local_path: &Path,
    byte_progress: Option<ByteProgressCb>,
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

    // Stream the body to disk in chunks. Avoids buffering full
    // file in memory — important for users with multi-GB Dropbox
    // datasets that would otherwise OOM the app.
    let mut local = tokio::fs::File::create(local_path).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;
    let total_hint = resp.content_length().unwrap_or(0);
    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AgentError::Network(format!("read chunk: {e}"))
        })?;
        local.write_all(&chunk).await.map_err(|e| {
            AgentError::FileSystem(format!("write local: {e}"))
        })?;
        total += chunk.len() as u64;
        if let Some(cb) = byte_progress.as_ref() {
            cb(total, total_hint);
        }
    }
    local.flush().await.map_err(|e| {
        AgentError::FileSystem(format!("flush local: {e}"))
    })?;
    Ok(total)
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

/// Per-file progress callback. Same shape as the other cache-and-
/// scan kinds.
pub type WalkProgressCb = std::sync::Arc<dyn Fn(usize, usize, &str) + Send + Sync>;

/// Walk + download every supported file under `base_path`. Skips
/// files whose size + server_modified match the previous walk's
/// manifest (incremental sync). `progress` (if Some) fires once
/// per supported file considered.
pub async fn walk_and_download(
    creds: &DropboxCredentials,
    base_path: &str,
    source_id: &str,
    progress: Option<WalkProgressCb>,
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

    // Pre-pass: filter to supported files + compute the work list.
    // Each work item carries the per-file values the task needs;
    // doing this up front means the concurrent loop is purely
    // I/O — no decisions, no shared mutable state for keys.
    let base_normalized = if base_path == "/" || base_path.is_empty() {
        "".to_string()
    } else {
        base_path.trim_end_matches('/').to_lowercase()
    };

    struct Work {
        path_lower: String,
        local_path: PathBuf,
        label: String,
        key: String,
        mtime_marker: String,
        size: u64,
    }
    let work: Vec<Work> = listing
        .iter()
        .filter_map(|f| {
            let path = PathBuf::from(&f.path_lower);
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase());
            if !ext
                .as_deref()
                .map(crate::scanner::is_supported_ext)
                .unwrap_or(false)
            {
                return None;
            }
            let rel_str = if base_normalized.is_empty() {
                f.path_lower.trim_start_matches('/').to_string()
            } else if let Some(s) = f.path_lower.strip_prefix(&base_normalized) {
                s.trim_start_matches('/').to_string()
            } else {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|s| s.to_string())?
            };
            let local_path = cache_dir.join(&rel_str);
            let label = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            Some(Work {
                path_lower: f.path_lower.clone(),
                local_path,
                label,
                key: f.path_lower.clone(),
                mtime_marker: f.server_modified.clone().unwrap_or_default(),
                size: f.size,
            })
        })
        .collect();

    let total_supported = work.len();
    let current_keys: HashSet<String> =
        work.iter().map(|w| w.key.clone()).collect();

    // Concurrent execution: up to MAX_CONCURRENT downloads in flight
    // at once. Dropbox's API tolerates parallelism up to its
    // per-app rate limit (~600 req/min for free accounts); 4
    // concurrent saturates a typical home connection without
    // tripping that. Manifest + counters live behind shared locks.
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    const MAX_CONCURRENT: usize = 4;
    // Files smaller than this don't need per-byte progress — the
    // existing per-file count + label gives plenty of feedback. Above
    // this threshold a single download can take long enough that the
    // UI looks frozen without intermediate updates.
    const BYTE_PROGRESS_MIN_SIZE: u64 = 10 * 1024 * 1024;

    let manifest_mu = std::sync::Arc::new(Mutex::new(manifest));
    let downloaded_ct = std::sync::Arc::new(AtomicUsize::new(0));
    let considered_ct = std::sync::Arc::new(AtomicUsize::new(0));

    futures::stream::iter(work)
        .for_each_concurrent(MAX_CONCURRENT, |w| {
            let creds = creds.clone();
            let manifest = manifest_mu.clone();
            let downloaded = downloaded_ct.clone();
            let considered = considered_ct.clone();
            let progress = progress.clone();
            async move {
                let needs = {
                    let m = manifest.lock().expect("manifest poisoned");
                    m.needs_download(&w.key, w.size, &w.mtime_marker)
                        || !w.local_path.exists()
                };
                if needs {
                    // Build a per-file byte-progress callback that
                    // emits walk-progress events with a "label (45%)"
                    // suffix at 5% boundaries. Skips the wiring for
                    // small files where per-file granularity is fine.
                    let byte_cb: Option<ByteProgressCb> = if w.size
                        > BYTE_PROGRESS_MIN_SIZE
                        && progress.is_some()
                    {
                        let walk_cb = progress.clone();
                        let label = w.label.clone();
                        let size = w.size;
                        let n_in_flight = considered.load(Ordering::Relaxed) + 1;
                        let last_pct = std::sync::Arc::new(AtomicUsize::new(0));
                        Some(std::sync::Arc::new(move |bytes, _hint| {
                            let pct = ((bytes.saturating_mul(20)) / size.max(1))
                                as usize;
                            let prev = last_pct.load(Ordering::Relaxed);
                            if pct > prev
                                && last_pct
                                    .compare_exchange(
                                        prev,
                                        pct,
                                        Ordering::Relaxed,
                                        Ordering::Relaxed,
                                    )
                                    .is_ok()
                            {
                                if let Some(cb) = walk_cb.as_ref() {
                                    cb(
                                        n_in_flight,
                                        total_supported,
                                        &format!("{label} ({}%)", pct * 5),
                                    );
                                }
                            }
                        }))
                    } else {
                        None
                    };
                    match download_file(
                        &creds,
                        &w.path_lower,
                        &w.local_path,
                        byte_cb,
                    )
                    .await
                    {
                        Ok(_) => {
                            downloaded.fetch_add(1, Ordering::Relaxed);
                            let mut m = manifest.lock().expect("manifest poisoned");
                            m.record(w.key.clone(), w.size, w.mtime_marker.clone());
                        }
                        Err(e) => {
                            eprintln!(
                                "[dropbox] download failed for {}: {} — skipping",
                                w.path_lower, e
                            );
                        }
                    }
                }
                let n = considered.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(cb) = progress.as_ref() {
                    cb(n, total_supported, &w.label);
                }
            }
        })
        .await;

    let downloaded = downloaded_ct.load(Ordering::Relaxed);
    // Reclaim the manifest by replacing the Arc<Mutex<>> wrapper
    // with the inner value. All tasks have completed by this point
    // so the lock is uncontended.
    let mut manifest = std::sync::Arc::try_unwrap(manifest_mu)
        .expect("manifest still referenced — task didn't drop")
        .into_inner()
        .expect("manifest poisoned");

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
            refresh_token: None,
            expires_at: None,
        };
        assert!(!creds.is_valid());

        let creds_ws = DropboxCredentials {
            access_token: "  ".to_string(),
            refresh_token: None,
            expires_at: None,
        };
        assert!(!creds_ws.is_valid());
    }

    #[test]
    fn is_valid_accepts_real_looking_token() {
        let creds = DropboxCredentials {
            access_token: "sl.B1234567890abcdef".to_string(),
            refresh_token: None,
            expires_at: None,
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn pat_is_not_oauth_and_never_expires() {
        let creds = DropboxCredentials {
            access_token: "sl.PAT".to_string(),
            refresh_token: None,
            expires_at: None,
        };
        assert!(!creds.is_oauth());
        assert!(!creds.is_expiring());
    }

    #[test]
    fn oauth_with_far_future_expiry_is_not_expiring() {
        let creds = DropboxCredentials {
            access_token: "sl.OAUTH".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_at: Some("2099-01-01T00:00:00Z".to_string()),
        };
        assert!(creds.is_oauth());
        assert!(!creds.is_expiring());
    }

    #[test]
    fn oauth_with_past_expiry_is_expiring() {
        let creds = DropboxCredentials {
            access_token: "sl.OAUTH".to_string(),
            refresh_token: Some("rt".to_string()),
            expires_at: Some("2000-01-01T00:00:00Z".to_string()),
        };
        assert!(creds.is_expiring());
    }

    #[test]
    fn legacy_pat_only_json_deserializes() {
        // PAT-only entries written before OAuth landed must still
        // load — this is the backward-compat guarantee.
        let json = r#"{"access_token": "sl.LEGACY"}"#;
        let creds: DropboxCredentials = serde_json::from_str(json).unwrap();
        assert_eq!(creds.access_token, "sl.LEGACY");
        assert!(creds.refresh_token.is_none());
        assert!(creds.expires_at.is_none());
        assert!(!creds.is_oauth());
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
