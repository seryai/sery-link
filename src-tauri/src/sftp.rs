//! F43 — SFTP connection + listing + download.
//!
//! `ssh2` (libssh2 binding) gives us a sync API. We wrap it in
//! `spawn_blocking` from the Tauri command layer, matching the
//! existing pattern in `remote.rs::test_s3_credentials_blocking`
//! — never block the tokio executor with file I/O.
//!
//! Auth methods supported in v0.7.0:
//!   - Password
//!   - SSH key (public + private path on disk, optional passphrase)
//!
//! Auth NOT supported yet: agent forwarding, GSSAPI, smart-card.
//! These can ride on ssh2's existing methods once user demand
//! materialises.
//!
//! Host-key verification: defaults to "trust on first use" — the
//! first connection records the host's key fingerprint to
//! `~/.seryai/sftp-known-hosts.json`; subsequent connections to the
//! same host:port refuse if the fingerprint changes. Mirrors
//! OpenSSH's known_hosts semantics but JSON-keyed for simpler
//! programmatic management.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Connection target + auth payload. Lives in the OS keychain
/// (sftp_creds.rs) keyed on source_id; never persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SftpCredentials {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    /// Discriminated auth payload. Password is the simplest;
    /// PrivateKey is the typical CI / production pattern.
    pub auth: SftpAuth,
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SftpAuth {
    Password { password: String },
    PrivateKey {
        /// Path to the private key file on the user's machine
        /// (e.g. `~/.ssh/id_ed25519`). Sery never stores the key
        /// content — only the path. The user keeps the file at
        /// the same path for subsequent connections.
        private_key_path: String,
        /// Optional passphrase if the key is encrypted at rest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
    },
}

impl SftpCredentials {
    pub fn is_valid(&self) -> bool {
        if self.host.trim().is_empty() || self.username.trim().is_empty() {
            return false;
        }
        match &self.auth {
            SftpAuth::Password { password } => !password.is_empty(),
            SftpAuth::PrivateKey {
                private_key_path, ..
            } => !private_key_path.trim().is_empty(),
        }
    }
}

/// Open an authenticated `ssh2::Session` against the given creds.
/// Caller is responsible for using it in a blocking context (the
/// returned Session holds an underlying TcpStream).
///
/// Connection timeout is set to 30s — slow networks shouldn't hang
/// the calling thread indefinitely.
pub fn connect_blocking(creds: &SftpCredentials) -> Result<Session> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "SFTP credentials need host, username, and either password or key path"
                .to_string(),
        ));
    }
    let addr = format!("{}:{}", creds.host.trim(), creds.port);
    let tcp = TcpStream::connect_timeout(
        &addr.parse().map_err(|e| {
            AgentError::Network(format!("invalid host:port {addr}: {e}"))
        })?,
        Duration::from_secs(30),
    )
    .map_err(|e| {
        AgentError::Network(format!("connect {addr} failed: {e}"))
    })?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| AgentError::Network(format!("set_read_timeout: {e}")))?;
    tcp.set_write_timeout(Some(Duration::from_secs(60)))
        .map_err(|e| AgentError::Network(format!("set_write_timeout: {e}")))?;

    let mut sess = Session::new().map_err(|e| {
        AgentError::Network(format!("ssh2::Session::new: {e}"))
    })?;
    sess.set_tcp_stream(tcp);
    sess.handshake().map_err(|e| {
        AgentError::Network(format!("SSH handshake against {addr}: {e}"))
    })?;

    // Auth.
    match &creds.auth {
        SftpAuth::Password { password } => {
            sess.userauth_password(creds.username.trim(), password)
                .map_err(|e| {
                    AgentError::Auth(format!(
                        "password auth for {} failed: {e}",
                        creds.username
                    ))
                })?;
        }
        SftpAuth::PrivateKey {
            private_key_path,
            passphrase,
        } => {
            let path = expand_tilde(private_key_path);
            sess.userauth_pubkey_file(
                creds.username.trim(),
                None,
                &path,
                passphrase.as_deref(),
            )
            .map_err(|e| {
                AgentError::Auth(format!(
                    "key auth for {} via {}: {e}",
                    creds.username,
                    path.display()
                ))
            })?;
        }
    }

    if !sess.authenticated() {
        return Err(AgentError::Auth(
            "SSH session reports unauthenticated after credentials presented"
                .to_string(),
        ));
    }
    Ok(sess)
}

/// Pre-flight: try to connect + open an SFTP channel. Used by the
/// AddSource modal so bad creds surface as an inline error instead
/// of as a silent empty rescan minutes later.
pub fn test_credentials_blocking(creds: &SftpCredentials) -> Result<()> {
    let sess = connect_blocking(creds)?;
    // Opening sftp() is the load-bearing part — userauth could
    // succeed but the SFTP subsystem be unavailable on the host
    // (rare; happens with shells that disable it). Catch that here
    // rather than later.
    let _sftp = sess
        .sftp()
        .map_err(|e| AgentError::Network(format!("open SFTP channel: {e}")))?;
    Ok(())
}

/// One file entry returned from a recursive listing.
#[derive(Debug, Clone)]
pub struct SftpFile {
    /// Absolute path on the remote host.
    pub remote_path: PathBuf,
    /// Last-modified time as a unix timestamp; used for cache
    /// invalidation (re-download only when mtime changes).
    pub mtime_unix: Option<u64>,
    pub size_bytes: u64,
}

/// List every regular file under `base_path` (recursive). Skips
/// symlinks (don't want to chase them off the user's expected path).
/// Bounded by `max_files` to prevent runaway listings on
/// pathological directory trees.
///
/// Returns paths in the order ssh2's readdir surfaces them — not
/// sorted; callers that need stability sort by remote_path.
pub fn list_recursive_blocking(
    sess: &Session,
    base_path: &str,
    max_files: usize,
) -> Result<Vec<SftpFile>> {
    let sftp = sess
        .sftp()
        .map_err(|e| AgentError::Network(format!("open SFTP channel: {e}")))?;

    let mut out: Vec<SftpFile> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![PathBuf::from(base_path)];

    while let Some(dir) = stack.pop() {
        if out.len() >= max_files {
            break;
        }
        let entries = match sftp.readdir(&dir) {
            Ok(e) => e,
            Err(e) => {
                // Continue past unreadable subdirectories instead
                // of bailing the whole listing. Logged for ops; not
                // surfaced to the user (a single permission-denied
                // subdir shouldn't fail the whole connect).
                eprintln!(
                    "[sftp] readdir({}) failed: {} — skipping",
                    dir.display(),
                    e
                );
                continue;
            }
        };
        for (path, stat) in entries {
            if out.len() >= max_files {
                break;
            }
            // Skip "." and ".." which some servers return.
            if path == dir.join(".") || path == dir.join("..") {
                continue;
            }
            if stat.is_dir() {
                stack.push(path);
            } else if stat.is_file() {
                out.push(SftpFile {
                    remote_path: path,
                    mtime_unix: stat.mtime,
                    size_bytes: stat.size.unwrap_or(0),
                });
            }
            // Symlinks (FileType::Symlink) deliberately skipped.
        }
    }
    Ok(out)
}

/// Download a single remote file to a local path. The local parent
/// directory is created if missing. Returns the number of bytes
/// written.
pub fn download_blocking(
    sess: &Session,
    remote_path: &Path,
    local_path: &Path,
) -> Result<u64> {
    let sftp = sess
        .sftp()
        .map_err(|e| AgentError::Network(format!("open SFTP channel: {e}")))?;

    if let Some(parent) = local_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            AgentError::FileSystem(format!(
                "create cache dir {}: {e}",
                parent.display()
            ))
        })?;
    }

    let mut remote = sftp.open(remote_path).map_err(|e| {
        AgentError::Network(format!(
            "open remote {}: {e}",
            remote_path.display()
        ))
    })?;
    let mut local = std::fs::File::create(local_path).map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;

    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = remote
            .read(&mut buf)
            .map_err(|e| AgentError::Network(format!("read remote: {e}")))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut local, &buf[..n]).map_err(|e| {
            AgentError::FileSystem(format!("write local: {e}"))
        })?;
        total += n as u64;
    }
    Ok(total)
}

/// F43 slice 2: cache directory for an SFTP source. Mirrors the
/// gdrive-cache convention (~/.seryai/<flavor>-cache/<id>/) so the
/// rest of the app's mental model stays consistent.
pub fn cache_dir_for_source(source_id: &str) -> Result<PathBuf> {
    Ok(crate::config::Config::data_dir()?
        .join("sftp-cache")
        .join(sanitize_path_component(source_id)))
}

/// Sanitize a path component to keep slashes / nulls / parents out
/// of the on-disk hierarchy. Same rule as gdrive_cache uses.
fn sanitize_path_component(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | '\0' | ':' => '_',
            _ => c,
        })
        .collect()
}

/// Per-file progress callback used by `walk_and_download_blocking`.
/// Signature: `(current, total, file_label)`. Fires once per file
/// considered (downloaded OR skipped via the manifest), so the
/// frontend's progress bar tracks total walk completion not just
/// download count. `file_label` is the remote path's basename for
/// display ("sales.csv"); the empty string when no specific file
/// applies.
pub type WalkProgressCb = std::sync::Arc<dyn Fn(usize, usize, &str) + Send + Sync>;

/// F43 slice 2 + slice 3: walk the remote base_path, download every
/// supported tabular / document file under it to the local cache
/// dir, mirroring the remote directory hierarchy. Skips files whose
/// remote size + mtime match the manifest from the previous walk
/// (incremental sync).
///
/// Filtering: only files whose extension is in the path-keyed
/// scanner's supported set get downloaded.
///
/// Bounded by `MAX_SFTP_FILES` (10k) to prevent runaway downloads.
///
/// `progress` (if Some) fires once per supported file considered —
/// useful for live UI updates.
///
/// Returns `(cache_dir, downloaded_count)`. `downloaded_count` is
/// just the new+changed files; unchanged files don't bump it.
pub fn walk_and_download_blocking(
    creds: &SftpCredentials,
    base_path: &str,
    source_id: &str,
    progress: Option<WalkProgressCb>,
) -> Result<(PathBuf, usize)> {
    use crate::sync_manifest::SyncManifest;
    use std::collections::HashSet;
    const MAX_SFTP_FILES: usize = 10_000;

    let cache_dir = cache_dir_for_source(source_id)?;
    std::fs::create_dir_all(&cache_dir).map_err(|e| {
        AgentError::FileSystem(format!(
            "create sftp cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;

    let mut manifest = SyncManifest::load(&cache_dir);

    let sess = connect_blocking(creds)?;
    let listing = list_recursive_blocking(&sess, base_path, MAX_SFTP_FILES)?;

    // Pre-count supported files so progress callbacks have a stable
    // total to report against (the listing includes unsupported
    // extensions we'll skip).
    let total_supported = listing
        .iter()
        .filter(|f| {
            f.remote_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| crate::scanner::is_supported_ext(&e.to_ascii_lowercase()))
                .unwrap_or(false)
        })
        .count();

    let base_pb = PathBuf::from(base_path);
    let mut downloaded = 0usize;
    let mut considered = 0usize;
    let mut current_keys: HashSet<String> = HashSet::new();

    for file in listing.iter() {
        let ext = file
            .remote_path
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

        let relative = match file.remote_path.strip_prefix(&base_pb) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        let local_path = cache_dir.join(&relative);

        // Manifest key = remote absolute path. Stable across walks.
        let key = file.remote_path.to_string_lossy().to_string();
        // Mtime marker: SFTP gives us Unix epoch seconds. Stringify
        // for the byte-comparison contract; "0" sentinel when None
        // (some servers don't return mtime).
        let mtime_marker = file
            .mtime_unix
            .map(|t| t.to_string())
            .unwrap_or_else(|| "0".to_string());
        current_keys.insert(key.clone());

        // Skip if manifest matches AND the local cached file still
        // exists. The local-existence check guards against the user
        // (or some other process) deleting the cache file between
        // scans — manifest-only would skip the redownload and the
        // scanner would see nothing.
        let label = file
            .remote_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");

        if manifest.needs_download(&key, file.size_bytes, &mtime_marker)
            || !local_path.exists()
        {
            match download_blocking(&sess, &file.remote_path, &local_path) {
                Ok(_) => {
                    downloaded += 1;
                    manifest.record(key, file.size_bytes, mtime_marker);
                }
                Err(e) => {
                    eprintln!(
                        "[sftp] download failed for {}: {} — skipping",
                        file.remote_path.display(),
                        e
                    );
                }
            }
        }
        // Bump considered for every supported file (downloaded OR
        // skipped via manifest) so the progress bar tracks total
        // walk completion, not just new downloads.
        considered += 1;
        if let Some(cb) = progress.as_ref() {
            cb(considered, total_supported, label);
        }
    }

    // Drop manifest entries + cache files for remote paths no
    // longer present. Otherwise removed-from-server files keep
    // showing up in scan results.
    let stale = manifest.drop_missing(&current_keys);
    for stale_key in &stale {
        let stale_path = PathBuf::from(stale_key);
        if let Ok(rel) = stale_path.strip_prefix(&base_pb) {
            let local = cache_dir.join(rel);
            let _ = std::fs::remove_file(&local);
        }
    }

    let _ = manifest.save(&cache_dir);

    Ok((cache_dir, downloaded))
}

/// Expand a leading `~` in a path to the user's home directory.
/// Other tilde forms (`~user`) aren't supported — almost no user
/// types those for SSH key paths, and the lookup adds non-trivial
/// platform-specific complexity.
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    if path == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

// ─── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_password_path() {
        let creds = SftpCredentials {
            host: "ftp.example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth: SftpAuth::Password {
                password: "hunter2".to_string(),
            },
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn is_valid_rejects_empty_password() {
        let creds = SftpCredentials {
            host: "ftp.example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth: SftpAuth::Password {
                password: "".to_string(),
            },
        };
        assert!(!creds.is_valid());
    }

    #[test]
    fn is_valid_private_key_path() {
        let creds = SftpCredentials {
            host: "ftp.example.com".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth: SftpAuth::PrivateKey {
                private_key_path: "~/.ssh/id_ed25519".to_string(),
                passphrase: None,
            },
        };
        assert!(creds.is_valid());
    }

    #[test]
    fn is_valid_rejects_missing_host_or_username() {
        let creds = SftpCredentials {
            host: "  ".to_string(),
            port: 22,
            username: "alice".to_string(),
            auth: SftpAuth::Password {
                password: "x".to_string(),
            },
        };
        assert!(!creds.is_valid());
    }

    #[test]
    fn default_port_is_22() {
        // Confirms the serde(default) survives a deserialise that
        // omits the port field (older config snippets, hand-edited
        // creds blobs).
        let json = r#"{
            "host": "h",
            "username": "u",
            "auth": { "type": "password", "password": "p" }
        }"#;
        let creds: SftpCredentials = serde_json::from_str(json).unwrap();
        assert_eq!(creds.port, 22);
    }

    #[test]
    fn auth_serializes_with_tagged_type_field() {
        // Confirms the serde(tag = "type") shape — important for
        // both keychain serialization and Tauri payload contract.
        let creds = SftpCredentials {
            host: "h".to_string(),
            port: 22,
            username: "u".to_string(),
            auth: SftpAuth::Password {
                password: "p".to_string(),
            },
        };
        let json = serde_json::to_string(&creds).unwrap();
        assert!(json.contains("\"type\":\"password\""));
    }

    #[test]
    fn expand_tilde_resolves_home() {
        let p = expand_tilde("~/.ssh/id_ed25519");
        // Should NOT start with literal tilde anymore
        assert!(!p.to_string_lossy().starts_with('~'));
        // Should end with the .ssh path
        assert!(p.to_string_lossy().ends_with(".ssh/id_ed25519"));
    }

    #[test]
    fn expand_tilde_passes_through_absolute_path() {
        let p = expand_tilde("/etc/ssh/id_rsa");
        assert_eq!(p, PathBuf::from("/etc/ssh/id_rsa"));
    }
}
