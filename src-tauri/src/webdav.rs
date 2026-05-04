//! F44 — WebDAV connection + listing + download.
//!
//! WebDAV is HTTP with extra verbs (PROPFIND for listing, GET for
//! download). DuckDB httpfs only does plain GET, so we maintain a
//! local cache mirror — same shape as SFTP. `reqwest_dav` handles
//! the PROPFIND XML parsing for us.
//!
//! Auth methods supported in v0.7.0:
//!   - Anonymous (public WebDAV — rare but exists)
//!   - Basic (username + password — typical for Nextcloud / ownCloud
//!     with app passwords; also works for generic WebDAV servers)
//!   - Digest (legacy but still encountered)
//!
//! Bearer / OAuth WebDAV (some providers) is NOT yet supported.
//! Add it as a new SftpAuth-style variant if user demand
//! materialises.

use crate::error::{AgentError, Result};
use reqwest_dav::{Auth, ClientBuilder, Depth};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Connection target + auth payload. Lives in the OS keychain
/// (webdav_creds.rs) keyed on source_id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebDavCredentials {
    /// Server base URL — typically `https://example.com/dav` or
    /// for Nextcloud `https://nc.example.com/remote.php/dav/files/<user>/`.
    /// We append `base_path` to this when listing.
    pub server_url: String,
    /// Discriminated auth payload.
    pub auth: WebDavAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebDavAuth {
    /// Public WebDAV — no creds. Rare but exists.
    Anonymous,
    /// Username + password. Most Nextcloud / ownCloud setups use
    /// app-specific passwords here; some legacy servers want the
    /// account password.
    Basic { username: String, password: String },
    /// HTTP Digest auth — older servers.
    Digest { username: String, password: String },
}

impl WebDavCredentials {
    pub fn is_valid(&self) -> bool {
        if self.server_url.trim().is_empty() {
            return false;
        }
        match &self.auth {
            WebDavAuth::Anonymous => true,
            WebDavAuth::Basic { username, password }
            | WebDavAuth::Digest { username, password } => {
                !username.trim().is_empty() && !password.is_empty()
            }
        }
    }
}

fn auth_to_dav_auth(auth: &WebDavAuth) -> Auth {
    match auth {
        WebDavAuth::Anonymous => Auth::Anonymous,
        WebDavAuth::Basic { username, password } => {
            Auth::Basic(username.clone(), password.clone())
        }
        WebDavAuth::Digest { username, password } => {
            Auth::Digest(username.clone(), password.clone())
        }
    }
}

/// Build a `reqwest_dav::Client` for these creds. The Client is
/// async (uses our existing reqwest under the hood), so the caller
/// must be in a tokio context.
fn build_client(creds: &WebDavCredentials) -> Result<reqwest_dav::Client> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "WebDAV credentials need server URL and (for Basic / Digest) \
             username + password"
                .to_string(),
        ));
    }
    ClientBuilder::new()
        .set_host(creds.server_url.trim().to_string())
        .set_auth(auth_to_dav_auth(&creds.auth))
        .build()
        .map_err(|e| {
            AgentError::Network(format!("WebDAV client build: {e}"))
        })
}

/// Pre-flight: build the client and run a PROPFIND with Depth 0
/// against the server root. Catches bad URL / bad creds / wrong
/// auth-type / TLS issues all at once. Used by the AddSourceModal
/// "Test connection" button.
pub async fn test_credentials(creds: &WebDavCredentials) -> Result<()> {
    let client = build_client(creds)?;
    // Depth 0 = the resource itself only — cheapest possible
    // PROPFIND. Just confirms the server speaks WebDAV with our
    // auth. Errors here surface bad URL / bad password / 401 / etc.
    let _entries = client
        .list("/", Depth::Number(0))
        .await
        .map_err(|e| {
            AgentError::Network(format!("WebDAV PROPFIND test: {e}"))
        })?;
    Ok(())
}

/// One file entry from a recursive listing.
#[derive(Debug, Clone)]
pub struct WebDavFile {
    /// Path relative to the server root. Normalised — leading /
    /// stripped; URL-encoded chars decoded.
    pub remote_href: String,
    pub size_bytes: u64,
    pub mtime_unix: Option<i64>,
}

/// List every file under `base_path` (recursive). Filters to files
/// only — folders are walked into but not emitted. Bounded by
/// `max_files` to prevent runaway listings on misconfigured paths.
pub async fn list_recursive(
    creds: &WebDavCredentials,
    base_path: &str,
    max_files: usize,
) -> Result<Vec<WebDavFile>> {
    use reqwest_dav::list_cmd::ListEntity;
    let client = build_client(creds)?;
    let entries = client
        .list(base_path, Depth::Infinity)
        .await
        .map_err(|e| {
            AgentError::Network(format!(
                "WebDAV PROPFIND on {base_path}: {e}"
            ))
        })?;

    let mut out: Vec<WebDavFile> = Vec::new();
    for entry in entries {
        if out.len() >= max_files {
            break;
        }
        if let ListEntity::File(file) = entry {
            out.push(WebDavFile {
                remote_href: file.href,
                size_bytes: file.content_length.max(0) as u64,
                mtime_unix: Some(file.last_modified.timestamp()),
            });
        }
        // Folders walked through silently — their files come back
        // as separate ListEntity::File entries via Depth::Infinity.
    }
    Ok(out)
}

/// Download a single remote file to a local path.
///
/// Uses our project-level reqwest (which has the `stream` feature
/// enabled) directly rather than reqwest_dav's re-exported one. The
/// list+PROPFIND path still goes through reqwest_dav for its XML
/// parsing; we only bypass it here to get bytes_stream() so multi-GB
/// WebDAV files don't OOM the app.
///
/// Auth construction is built manually (Basic / Digest header for
/// the appropriate WebDavAuth variant) since we're not going through
/// reqwest_dav's Auth abstraction here. Anonymous mode just sends
/// no auth header.
pub async fn download_file(
    creds: &WebDavCredentials,
    remote_href: &str,
    local_path: &Path,
) -> Result<u64> {
    if let Some(parent) = local_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            AgentError::FileSystem(format!(
                "create cache dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    // The href returned by PROPFIND can be either a full URL or
    // an absolute path; resolve relative to the server_url either way.
    let url = resolve_dav_url(&creds.server_url, remote_href);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| AgentError::Network(format!("reqwest build: {e}")))?;

    let mut req = client.get(&url);
    match &creds.auth {
        WebDavAuth::Anonymous => {}
        WebDavAuth::Basic { username, password } => {
            req = req.basic_auth(username, Some(password));
        }
        WebDavAuth::Digest { .. } => {
            // Digest auth requires a challenge round-trip (server
            // sends WWW-Authenticate; client computes response).
            // reqwest doesn't have a built-in Digest auth; for now,
            // fall back through reqwest_dav for Digest sources by
            // calling the original buffered path. Most modern
            // Nextcloud / ownCloud installs use Basic with app
            // passwords, so this affects a small minority.
            return download_file_buffered(creds, remote_href, local_path).await;
        }
    }

    let response = req.send().await.map_err(|e| {
        AgentError::Network(format!("WebDAV GET {url}: {e}"))
    })?;
    if !response.status().is_success() {
        return Err(AgentError::Network(format!(
            "WebDAV GET {url}: HTTP {}",
            response.status()
        )));
    }

    let mut local = tokio::fs::File::create(local_path).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut total: u64 = 0;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AgentError::Network(format!("read chunk: {e}"))
        })?;
        local.write_all(&chunk).await.map_err(|e| {
            AgentError::FileSystem(format!("write local: {e}"))
        })?;
        total += chunk.len() as u64;
    }
    local.flush().await.map_err(|e| {
        AgentError::FileSystem(format!("flush local: {e}"))
    })?;
    Ok(total)
}

/// Resolve an href returned by PROPFIND against the server base URL.
/// Servers may return absolute URLs (`https://nc.example/foo/bar`),
/// absolute paths (`/foo/bar`), or relative paths (`bar`); handle
/// each.
fn resolve_dav_url(server_url: &str, href: &str) -> String {
    if href.starts_with("http://") || href.starts_with("https://") {
        return href.to_string();
    }
    let trimmed_server = server_url.trim_end_matches('/');
    if href.starts_with('/') {
        // Absolute path — replace the path on the server URL.
        match url::Url::parse(server_url) {
            Ok(u) => format!(
                "{}://{}{}",
                u.scheme(),
                u.host_str().unwrap_or(""),
                href
            ),
            Err(_) => format!("{}{}", trimmed_server, href),
        }
    } else {
        format!("{}/{}", trimmed_server, href)
    }
}

/// Buffered fallback used for WebDavAuth::Digest, which our
/// stream path doesn't yet support (reqwest has no built-in
/// Digest helper). reqwest_dav handles Digest via its own
/// challenge-response logic, so we keep using it for those
/// sources at the cost of buffering the whole body.
async fn download_file_buffered(
    creds: &WebDavCredentials,
    remote_href: &str,
    local_path: &Path,
) -> Result<u64> {
    let client = build_client(creds)?;
    let response = client.get(remote_href).await.map_err(|e| {
        AgentError::Network(format!("WebDAV GET {remote_href}: {e}"))
    })?;
    if !response.status().is_success() {
        return Err(AgentError::Network(format!(
            "WebDAV GET {remote_href}: HTTP {}",
            response.status()
        )));
    }
    let body = response.bytes().await.map_err(|e| {
        AgentError::Network(format!("read body: {e}"))
    })?;
    let mut local = tokio::fs::File::create(local_path).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;
    local.write_all(&body).await.map_err(|e| {
        AgentError::FileSystem(format!("write local: {e}"))
    })?;
    local.flush().await.map_err(|e| {
        AgentError::FileSystem(format!("flush local: {e}"))
    })?;
    Ok(body.len() as u64)
}

/// Where the WebDAV cache lives for a given source. Mirror of
/// sftp::cache_dir_for_source — keeps the on-disk layout
/// predictable across remote-source kinds.
pub fn cache_dir_for_source(source_id: &str) -> Result<PathBuf> {
    Ok(crate::config::Config::data_dir()?
        .join("webdav-cache")
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

/// Per-file progress callback. Same signature as
/// `sftp::WalkProgressCb`.
pub type WalkProgressCb = Box<dyn Fn(usize, usize, &str) + Send + Sync>;

/// Walk the remote `base_path` and download every supported tabular
/// / document file under it to the local cache dir. Mirrors the
/// remote hierarchy. Skips files whose remote size + mtime match
/// the manifest from the previous walk (incremental sync).
///
/// Bounded by `MAX_WEBDAV_FILES` (10k). `progress` (if Some) fires
/// once per supported file considered.
pub async fn walk_and_download(
    creds: &WebDavCredentials,
    base_path: &str,
    source_id: &str,
    progress: Option<WalkProgressCb>,
) -> Result<(PathBuf, usize)> {
    use crate::sync_manifest::SyncManifest;
    use std::collections::HashSet;
    const MAX_WEBDAV_FILES: usize = 10_000;

    let cache_dir = cache_dir_for_source(source_id)?;
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create webdav cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;

    let mut manifest = SyncManifest::load(&cache_dir);

    let listing = list_recursive(creds, base_path, MAX_WEBDAV_FILES).await?;
    let base_pb = PathBuf::from(base_path);
    let total_supported = listing
        .iter()
        .filter(|f| {
            PathBuf::from(&f.remote_href)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| crate::scanner::is_supported_ext(&e.to_ascii_lowercase()))
                .unwrap_or(false)
        })
        .count();
    let mut downloaded = 0usize;
    let mut considered = 0usize;
    let mut current_keys: HashSet<String> = HashSet::new();

    for file in listing.iter() {
        let path = PathBuf::from(&file.remote_href);
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

        let relative = match path.strip_prefix(&base_pb) {
            Ok(r) => r.to_path_buf(),
            Err(_) => match path.file_name() {
                Some(name) => PathBuf::from(name),
                None => continue,
            },
        };
        let local_path = cache_dir.join(&relative);

        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Manifest key = remote href. Mtime marker = the Unix
        // timestamp from the WebDAV multistatus response (already
        // an i64; stringify for byte-comparison).
        let key = file.remote_href.clone();
        let mtime_marker = file
            .mtime_unix
            .map(|t| t.to_string())
            .unwrap_or_else(|| "0".to_string());
        current_keys.insert(key.clone());

        if manifest.needs_download(&key, file.size_bytes, &mtime_marker)
            || !local_path.exists()
        {
            match download_file(creds, &file.remote_href, &local_path).await {
                Ok(_) => {
                    downloaded += 1;
                    manifest.record(key, file.size_bytes, mtime_marker);
                }
                Err(e) => {
                    eprintln!(
                        "[webdav] download failed for {}: {} — skipping",
                        file.remote_href, e
                    );
                }
            }
        }
        considered += 1;
        if let Some(cb) = progress.as_ref() {
            cb(considered, total_supported, label);
        }
    }

    let stale = manifest.drop_missing(&current_keys);
    for stale_key in &stale {
        let stale_path = PathBuf::from(stale_key);
        if let Ok(rel) = stale_path.strip_prefix(&base_pb) {
            let local = cache_dir.join(rel);
            let _ = tokio::fs::remove_file(&local).await;
        }
    }

    let _ = manifest.save(&cache_dir);

    Ok((cache_dir, downloaded))
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_anonymous() {
        let creds = WebDavCredentials {
            server_url: "https://dav.example.com".to_string(),
            auth: WebDavAuth::Anonymous,
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn is_valid_basic() {
        let creds = WebDavCredentials {
            server_url: "https://dav.example.com".to_string(),
            auth: WebDavAuth::Basic {
                username: "alice".to_string(),
                password: "hunter2".to_string(),
            },
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn is_valid_rejects_empty_url() {
        let creds = WebDavCredentials {
            server_url: "".to_string(),
            auth: WebDavAuth::Anonymous,
        };
        assert!(!creds.is_valid());
    }

    #[test]
    fn is_valid_rejects_basic_with_empty_username() {
        let creds = WebDavCredentials {
            server_url: "https://dav.example.com".to_string(),
            auth: WebDavAuth::Basic {
                username: "  ".to_string(),
                password: "hunter2".to_string(),
            },
        };
        assert!(!creds.is_valid());
    }

    #[test]
    fn auth_serializes_with_tagged_type_field() {
        let creds = WebDavCredentials {
            server_url: "https://dav.example.com".to_string(),
            auth: WebDavAuth::Basic {
                username: "alice".to_string(),
                password: "hunter2".to_string(),
            },
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains("\"type\":\"basic\""));
    }
}
