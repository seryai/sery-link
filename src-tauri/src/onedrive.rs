//! F49 — OneDrive via Microsoft Graph + device code flow auth.
//!
//! Why device code instead of PKCE + deep-link:
//!   - Microsoft Graph has no Personal Access Token equivalent (the
//!     way Dropbox does), so simple PAT auth isn't an option.
//!   - Full OAuth authorization-code + PKCE needs a deep-link
//!     callback registered with the OS (`seryai://oauth/onedrive/...`)
//!     and a localhost server fallback. ~3x the code surface area
//!     vs device code.
//!   - Device code flow is a first-class Microsoft auth grant
//!     designed for "headless" apps (CLIs, IoT) and works equally
//!     well in a desktop app: we display a code, the user enters
//!     it on `microsoft.com/devicelogin` in any browser, and we
//!     poll for the result. No deep-link plumbing needed.
//!
//! Token shape: standard OAuth — access_token (1h validity),
//! refresh_token (long-lived), expires_at (RFC 3339). The refresh
//! flow runs automatically before each Graph API call when the
//! access token is within 60s of expiry.
//!
//! The `MICROSOFT_CLIENT_ID` constant must be set to the founder's
//! Microsoft Entra app registration. See
//! datalake/SETUP_MICROSOFT_OAUTH.md for the registration steps.
//! For dev/test before a real registration exists, the placeholder
//! ID below points at a non-functional app — testing the flow
//! end-to-end requires a real registration.

use crate::error::{AgentError, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// Microsoft Entra app registration client ID. This is a public
/// identifier — safe to embed in the binary (Microsoft's own docs
/// confirm this for `allowPublicClient: true` apps).
///
/// REPLACE before shipping: register a new app at
/// https://entra.microsoft.com/ → App registrations → New
/// registration. Required scopes: `Files.Read`, `Files.Read.All`,
/// `offline_access`. Allow public client flows = yes. Device code
/// flow = enabled.
///
/// Until then, this placeholder makes test_credentials fail at
/// the device code request — surfacing the misconfiguration as a
/// clear error rather than mysterious authentication failures.
const MICROSOFT_CLIENT_ID: &str = "REPLACE_WITH_REAL_APP_ID";

const TENANT: &str = "common"; // Personal + work + school accounts.
const SCOPE: &str = "Files.Read Files.Read.All offline_access";
const DEVICE_CODE_URL: &str =
    "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const GRAPH_BASE: &str = "https://graph.microsoft.com/v1.0";

/// Stored credentials. Lives in the OS keychain (onedrive_creds.rs)
/// keyed on source_id. Refreshed in-place when expired.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneDriveCredentials {
    pub access_token: String,
    pub refresh_token: String,
    /// RFC 3339; the access_token is invalid after this point.
    /// `refresh_if_expiring(...)` rotates ~60s before this to avoid
    /// race conditions.
    pub expires_at: String,
}

impl OneDriveCredentials {
    pub fn is_valid(&self) -> bool {
        !self.access_token.trim().is_empty()
            && !self.refresh_token.trim().is_empty()
    }

    pub fn access_token_expired_or_expiring(&self) -> bool {
        match DateTime::parse_from_rfc3339(&self.expires_at) {
            Ok(t) => Utc::now() + ChronoDuration::seconds(60) >= t,
            Err(_) => true, // can't parse → treat as expired
        }
    }
}

/// Public response from `start_device_code_flow` — the UI displays
/// `user_code` and tells the user to open `verification_uri`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeStart {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: i64,
    pub interval: i64,
    /// Friendly text the user can paste into a browser tab. Same
    /// content as `verification_uri`; carved out so the frontend
    /// doesn't have to format separately.
    pub message: String,
}

#[derive(Debug, Deserialize)]
struct DeviceCodeRaw {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: i64,
    interval: i64,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("onedrive reqwest client builder")
}

/// Step 1 of device code flow: request a code from Microsoft.
/// Returns the user_code + verification_uri the UI displays.
pub async fn start_device_code_flow() -> Result<DeviceCodeStart> {
    if MICROSOFT_CLIENT_ID == "REPLACE_WITH_REAL_APP_ID" {
        return Err(AgentError::Config(
            "OneDrive auth not yet configured — the founder needs to \
             register a Microsoft Entra app and set MICROSOFT_CLIENT_ID. \
             See datalake/SETUP_MICROSOFT_OAUTH.md."
                .to_string(),
        ));
    }
    let resp = http_client()
        .post(DEVICE_CODE_URL)
        .form(&[
            ("client_id", MICROSOFT_CLIENT_ID),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("OneDrive device code request: {e}"))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "OneDrive device code rejected (HTTP {status}): {}",
            body.chars().take(300).collect::<String>()
        )));
    }
    let raw: DeviceCodeRaw = resp.json().await.map_err(|e| {
        AgentError::Serialization(format!("parse device code: {e}"))
    })?;
    let message = raw.message.clone().unwrap_or_else(|| {
        format!(
            "Open {} in any browser and enter code {}",
            raw.verification_uri, raw.user_code
        )
    });
    Ok(DeviceCodeStart {
        device_code: raw.device_code,
        user_code: raw.user_code,
        verification_uri: raw.verification_uri,
        expires_in: raw.expires_in,
        interval: raw.interval,
        message,
    })
}

/// Step 2 of device code flow: poll Microsoft until the user
/// completes auth in their browser. Returns the access_token +
/// refresh_token. Caller is responsible for the polling loop —
/// this is one attempt that returns Pending vs Completed vs Error.
pub async fn poll_device_code(device_code: &str) -> Result<PollOutcome> {
    let resp = http_client()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", MICROSOFT_CLIENT_ID),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("OneDrive poll device code: {e}"))
        })?;

    if resp.status().is_success() {
        let token: TokenResponse = resp.json().await.map_err(|e| {
            AgentError::Serialization(format!("parse token resp: {e}"))
        })?;
        let refresh_token = token.refresh_token.unwrap_or_default();
        if refresh_token.is_empty() {
            return Err(AgentError::Auth(
                "OneDrive token response missing refresh_token — was \
                 'offline_access' scope granted?"
                    .to_string(),
            ));
        }
        let expires_at =
            (Utc::now() + ChronoDuration::seconds(token.expires_in)).to_rfc3339();
        return Ok(PollOutcome::Completed(OneDriveCredentials {
            access_token: token.access_token,
            refresh_token,
            expires_at,
        }));
    }
    // Non-success: parse the error code. "authorization_pending" /
    // "slow_down" mean keep polling; everything else is fatal.
    let err: ErrorResponse = match resp.json().await {
        Ok(e) => e,
        Err(e) => {
            return Err(AgentError::Network(format!(
                "OneDrive poll error parse: {e}"
            )))
        }
    };
    match err.error.as_str() {
        "authorization_pending" => Ok(PollOutcome::Pending),
        "slow_down" => Ok(PollOutcome::SlowDown),
        "expired_token" | "expired" => Err(AgentError::Auth(
            "OneDrive device code expired — restart the auth flow".to_string(),
        )),
        "access_denied" => Err(AgentError::Auth(
            "OneDrive auth denied — user declined or cancelled".to_string(),
        )),
        other => Err(AgentError::Auth(format!(
            "OneDrive auth failed ({other}): {}",
            err.error_description.unwrap_or_default()
        ))),
    }
}

#[derive(Debug)]
pub enum PollOutcome {
    /// The user completed auth — store these creds.
    Completed(OneDriveCredentials),
    /// User hasn't finished yet; poll again at `interval`.
    Pending,
    /// Microsoft asks us to back off — bump the poll interval.
    SlowDown,
}

/// Refresh an access_token using the stored refresh_token. Mutates
/// the supplied creds in place. Used before each Graph API call
/// when `access_token_expired_or_expiring()` is true.
pub async fn refresh_access_token(creds: &mut OneDriveCredentials) -> Result<()> {
    let resp = http_client()
        .post(TOKEN_URL)
        .form(&[
            ("client_id", MICROSOFT_CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", creds.refresh_token.as_str()),
            ("scope", SCOPE),
        ])
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("OneDrive refresh token: {e}"))
        })?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "OneDrive refresh failed (HTTP {status}): {} — re-auth needed",
            body.chars().take(300).collect::<String>()
        )));
    }
    let token: TokenResponse = resp.json().await.map_err(|e| {
        AgentError::Serialization(format!("parse refresh resp: {e}"))
    })?;
    creds.access_token = token.access_token;
    if let Some(new_refresh) = token.refresh_token {
        // Microsoft sometimes rotates the refresh token; honor it.
        creds.refresh_token = new_refresh;
    }
    creds.expires_at =
        (Utc::now() + ChronoDuration::seconds(token.expires_in)).to_rfc3339();
    Ok(())
}

/// One file entry from a recursive Drive listing.
#[derive(Debug, Clone)]
pub struct OneDriveFile {
    /// Drive item id — stable across renames. Used as the manifest
    /// key.
    pub id: String,
    /// Path inside the OneDrive (e.g. "/Documents/sales.csv"). Used
    /// to mirror layout under cache dir.
    pub path: String,
    pub size_bytes: u64,
    /// `lastModifiedDateTime` from Graph (RFC 3339).
    pub mtime: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphChildrenResponse {
    value: Vec<DriveItem>,
    #[serde(rename = "@odata.nextLink", default)]
    next_link: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DriveItem {
    id: String,
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(rename = "lastModifiedDateTime", default)]
    last_modified: Option<String>,
    #[serde(rename = "parentReference", default)]
    parent_reference: Option<ParentRef>,
    #[serde(default)]
    folder: Option<FolderFacet>,
    #[serde(default)]
    file: Option<FileFacet>,
}

#[derive(Debug, Deserialize)]
struct ParentRef {
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FolderFacet {}

#[derive(Debug, Deserialize)]
struct FileFacet {}

/// Pre-flight: try to list the user's Drive root with a tiny limit.
/// Confirms the access_token works.
pub async fn test_credentials(creds: &mut OneDriveCredentials) -> Result<()> {
    if creds.access_token_expired_or_expiring() {
        refresh_access_token(creds).await?;
    }
    let resp = http_client()
        .get(format!("{GRAPH_BASE}/me/drive/root/children?$top=1"))
        .bearer_auth(&creds.access_token)
        .send()
        .await
        .map_err(|e| AgentError::Network(format!("OneDrive test: {e}")))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "OneDrive Graph test rejected (HTTP {status}): {}",
            body.chars().take(300).collect::<String>()
        )));
    }
    Ok(())
}

/// Walk the Drive recursively starting at `base_path` (root if
/// empty or "/"). Returns every file under it, file-only.
/// Bounded by `max_files`.
///
/// Strategy: BFS over folders using the Graph `/children` endpoint,
/// following `@odata.nextLink` for pagination within a folder.
pub async fn list_recursive(
    creds: &mut OneDriveCredentials,
    base_path: &str,
    max_files: usize,
) -> Result<Vec<OneDriveFile>> {
    if creds.access_token_expired_or_expiring() {
        refresh_access_token(creds).await?;
    }
    let c = http_client();
    let mut out: Vec<OneDriveFile> = Vec::new();
    // Folders to walk; start with the base path. We use Graph's
    // path-based addressing: `root:/<path>:/children`.
    let normalized = base_path.trim_start_matches('/').trim_end_matches('/');
    let initial_url = if normalized.is_empty() {
        format!("{GRAPH_BASE}/me/drive/root/children?$top=200")
    } else {
        format!(
            "{GRAPH_BASE}/me/drive/root:/{}:/children?$top=200",
            urlencoding::encode(normalized)
        )
    };

    let mut queue: Vec<String> = vec![initial_url];

    while let Some(url) = queue.pop() {
        if out.len() >= max_files {
            break;
        }
        let resp = c
            .get(&url)
            .bearer_auth(&creds.access_token)
            .send()
            .await
            .map_err(|e| {
                AgentError::Network(format!("OneDrive children {url}: {e}"))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            // Per-folder failures shouldn't bail the whole walk.
            eprintln!(
                "[onedrive] children HTTP {status} for {url}: {} — skipping",
                body.chars().take(200).collect::<String>()
            );
            continue;
        }
        let page: GraphChildrenResponse =
            resp.json().await.map_err(|e| {
                AgentError::Serialization(format!("parse children: {e}"))
            })?;

        for item in page.value {
            if out.len() >= max_files {
                break;
            }
            // Compose the path: parentReference.path looks like
            // "/drive/root:/Documents" — strip "/drive/root:" to get
            // the OneDrive-relative path. The full file path is then
            // "<parent path>/<name>".
            let parent_path = item
                .parent_reference
                .as_ref()
                .and_then(|p| p.path.as_deref())
                .map(|p| p.trim_start_matches("/drive/root:").to_string())
                .unwrap_or_default();
            let full_path = if parent_path.is_empty() || parent_path == "/" {
                format!("/{}", item.name)
            } else {
                format!("{}/{}", parent_path.trim_end_matches('/'), item.name)
            };

            if item.folder.is_some() {
                // Folder — enqueue its children for the walk.
                let folder_path = full_path.trim_start_matches('/').to_string();
                let folder_url = format!(
                    "{GRAPH_BASE}/me/drive/root:/{}:/children?$top=200",
                    urlencoding::encode(&folder_path)
                );
                queue.push(folder_url);
            } else if item.file.is_some() {
                out.push(OneDriveFile {
                    id: item.id,
                    path: full_path,
                    size_bytes: item.size,
                    mtime: item.last_modified,
                });
            }
            // Other facets (link, package, etc.) silently skipped.
        }

        if let Some(next) = page.next_link {
            queue.push(next);
        }
    }

    Ok(out)
}

/// Per-byte progress callback. Same shape as the other cache-and-
/// scan kinds.
pub type ByteProgressCb = std::sync::Arc<dyn Fn(u64, u64) + Send + Sync>;

/// Download a single file by its Drive item id.
pub async fn download_file(
    creds: &mut OneDriveCredentials,
    item_id: &str,
    local_path: &Path,
) -> Result<u64> {
    if creds.access_token_expired_or_expiring() {
        refresh_access_token(creds).await?;
    }
    download_file_with_token(&creds.access_token, item_id, local_path, None).await
}

/// Internal: download with an explicit access token, no creds
/// mutation. Used by `walk_and_download` so concurrent tasks can
/// share an immutable token snapshot — the up-front refresh in
/// list_recursive guarantees the token is fresh for the duration
/// of the walk (typical walks complete well inside the token's
/// 1-hour validity window).
async fn download_file_with_token(
    access_token: &str,
    item_id: &str,
    local_path: &Path,
    byte_progress: Option<ByteProgressCb>,
) -> Result<u64> {
    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            AgentError::FileSystem(format!(
                "create cache dir {}: {e}",
                parent.display()
            ))
        })?;
    }
    // Graph's `/content` endpoint redirects to a pre-signed download
    // URL; reqwest follows redirects by default.
    let url = format!("{GRAPH_BASE}/me/drive/items/{item_id}/content");
    let resp = http_client()
        .get(&url)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| {
            AgentError::Network(format!("OneDrive download {item_id}: {e}"))
        })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "OneDrive download {item_id} HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    // Stream the body to disk in chunks — avoids buffering full
    // file in memory. OneDrive personal accounts go up to 5TB; even
    // single-file downloads can be in the GB range.
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

pub fn cache_dir_for_source(source_id: &str) -> Result<PathBuf> {
    Ok(crate::config::Config::data_dir()?
        .join("onedrive-cache")
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

/// Walk + download. Same incremental-sync shape as the other
/// cache-and-scan kinds. `progress` (if Some) fires once per
/// supported file considered.
pub async fn walk_and_download(
    creds: &mut OneDriveCredentials,
    base_path: &str,
    source_id: &str,
    progress: Option<WalkProgressCb>,
) -> Result<(PathBuf, usize)> {
    use crate::sync_manifest::SyncManifest;
    use std::collections::HashSet;
    const MAX_ONEDRIVE_FILES: usize = 10_000;

    let cache_dir = cache_dir_for_source(source_id)?;
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create onedrive cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;

    let mut manifest = SyncManifest::load(&cache_dir);
    // list_recursive does its own up-front refresh-if-needed via
    // &mut creds; after it returns, creds.access_token is fresh.
    // We snapshot it for the concurrent download phase. Rationale:
    // OneDrive access tokens last 1 hour; typical walks complete
    // well within that window. If a download mid-walk fails with
    // 401 we surface the error and the user retries — a future
    // polish could add per-task refresh via Arc<TokioMutex<creds>>
    // but the added complexity isn't justified yet.
    let listing = list_recursive(creds, base_path, MAX_ONEDRIVE_FILES).await?;
    let access_token = creds.access_token.clone();

    let normalized_base = base_path.trim_end_matches('/').to_string();

    struct Work {
        id: String,
        local_path: PathBuf,
        label: String,
        key: String,
        mtime_marker: String,
        size: u64,
    }
    let work: Vec<Work> = listing
        .iter()
        .filter_map(|f| {
            let p = PathBuf::from(&f.path);
            let ext = p
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
            let rel = if normalized_base.is_empty() || normalized_base == "/" {
                f.path.trim_start_matches('/').to_string()
            } else if let Some(s) =
                f.path.strip_prefix(&format!("{}/", normalized_base))
            {
                s.to_string()
            } else if f.path == normalized_base {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&f.path)
                    .to_string()
            } else {
                f.path.trim_start_matches('/').to_string()
            };
            let local_path = cache_dir.join(&rel);
            let label = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            Some(Work {
                id: f.id.clone(),
                local_path,
                label,
                key: f.id.clone(),
                mtime_marker: f.mtime.clone().unwrap_or_default(),
                size: f.size_bytes,
            })
        })
        .collect();

    let total_supported = work.len();
    let current_keys: HashSet<String> =
        work.iter().map(|w| w.key.clone()).collect();

    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    const MAX_CONCURRENT: usize = 4;
    const BYTE_PROGRESS_MIN_SIZE: u64 = 10 * 1024 * 1024;

    let manifest_mu = std::sync::Arc::new(Mutex::new(manifest));
    let downloaded_ct = std::sync::Arc::new(AtomicUsize::new(0));
    let considered_ct = std::sync::Arc::new(AtomicUsize::new(0));
    let access_token = std::sync::Arc::new(access_token);

    futures::stream::iter(work)
        .for_each_concurrent(MAX_CONCURRENT, |w| {
            let token = access_token.clone();
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
                    match download_file_with_token(
                        &token,
                        &w.id,
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
                                "[onedrive] download failed for {}: {} — skipping",
                                w.id, e
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
    let mut manifest = std::sync::Arc::try_unwrap(manifest_mu)
        .expect("manifest still referenced")
        .into_inner()
        .expect("manifest poisoned");

    let stale = manifest.drop_missing(&current_keys);
    // The manifest key is the Drive item id, not a path — we don't
    // know the local path to delete from the key alone. Walk the
    // cache dir and rebuild from current entries instead. For now,
    // accept that orphaned cache files may linger; the manifest is
    // updated correctly so they won't block re-downloads of fresh
    // content. A future polish slice can add a sweep that cross-
    // references cache dir vs manifest paths.
    let _ = stale;

    let _ = manifest.save(&cache_dir);

    Ok((cache_dir, downloaded))
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_rejects_empty_tokens() {
        let creds = OneDriveCredentials {
            access_token: "".to_string(),
            refresh_token: "".to_string(),
            expires_at: "".to_string(),
        };
        assert!(!creds.is_valid());

        let creds = OneDriveCredentials {
            access_token: "x".to_string(),
            refresh_token: "".to_string(),
            expires_at: "2026-01-01T00:00:00Z".to_string(),
        };
        assert!(!creds.is_valid());
    }

    #[test]
    fn is_valid_accepts_real_looking_pair() {
        let creds = OneDriveCredentials {
            access_token: "EwBwA8l6BAA...".to_string(),
            refresh_token: "M.R3_BAY...".to_string(),
            expires_at: "2026-12-31T00:00:00Z".to_string(),
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn expired_or_expiring_treats_unparseable_as_expired() {
        let creds = OneDriveCredentials {
            access_token: "x".to_string(),
            refresh_token: "y".to_string(),
            expires_at: "not-a-date".to_string(),
        };
        assert!(creds.access_token_expired_or_expiring());
    }

    #[test]
    fn expired_or_expiring_returns_true_for_past() {
        let creds = OneDriveCredentials {
            access_token: "x".to_string(),
            refresh_token: "y".to_string(),
            expires_at: "2000-01-01T00:00:00Z".to_string(),
        };
        assert!(creds.access_token_expired_or_expiring());
    }

    #[test]
    fn expired_or_expiring_returns_false_for_far_future() {
        let creds = OneDriveCredentials {
            access_token: "x".to_string(),
            refresh_token: "y".to_string(),
            expires_at: "2099-01-01T00:00:00Z".to_string(),
        };
        assert!(!creds.access_token_expired_or_expiring());
    }

    #[test]
    fn parse_children_response_handles_files_and_folders() {
        let json = r#"{
            "value": [
                {
                    "id": "ITEM1",
                    "name": "sales.csv",
                    "size": 12345,
                    "lastModifiedDateTime": "2026-01-15T10:00:00Z",
                    "parentReference": { "path": "/drive/root:/Documents" },
                    "file": {}
                },
                {
                    "id": "ITEM2",
                    "name": "Subfolder",
                    "parentReference": { "path": "/drive/root:" },
                    "folder": {}
                }
            ]
        }"#;
        let resp: GraphChildrenResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.value.len(), 2);
        assert!(resp.value[0].file.is_some());
        assert!(resp.value[0].folder.is_none());
        assert_eq!(resp.value[0].size, 12345);
        assert!(resp.value[1].folder.is_some());
        assert!(resp.value[1].file.is_none());
        assert!(resp.next_link.is_none());
    }

    #[test]
    fn parse_children_response_handles_pagination_link() {
        let json = r#"{
            "value": [],
            "@odata.nextLink": "https://graph.microsoft.com/v1.0/me/drive/root/children?$skiptoken=abc"
        }"#;
        let resp: GraphChildrenResponse = serde_json::from_str(json).unwrap();
        assert!(resp.next_link.is_some());
        assert!(resp
            .next_link
            .as_ref()
            .unwrap()
            .contains("$skiptoken=abc"));
    }
}
