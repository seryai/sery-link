//! F46 — Azure Blob Storage connection + listing + download.
//!
//! v0.7.0 ships SAS-token auth only. The user generates a Shared
//! Access Signature in the Azure portal scoped to their container,
//! pastes the URL-with-SAS into the form, and we use it as-is.
//! Storage account keys + Azure AD OAuth follow in later slices if
//! demand materialises.
//!
//! Why SAS:
//!   - Long-lived (user picks expiry up to several years).
//!   - Scoped to a single container with read-only permissions —
//!     least-privilege for a tool that only reads.
//!   - URL-embedded auth: no Bearer header, no refresh logic. We
//!     just append the SAS query string to every API call.
//!
//! Listing format: Azure's "List Blobs" REST endpoint returns
//! XML (EnumerationResults). We parse with roxmltree — the schema
//! is tiny and stable enough that hand-extracting `<Name>` +
//! `<Content-Length>` + `<Last-Modified>` is fine.

use crate::error::{AgentError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

/// Auth payload. The SAS includes everything needed (account name,
/// container, permissions, expiry, signature), so the credential
/// shape is just a single token string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AzureBlobCredentials {
    /// Shared Access Signature query string. Either with leading
    /// `?` (as Azure portal copies it) or without — we normalise
    /// before use.
    pub sas_token: String,
}

impl AzureBlobCredentials {
    pub fn is_valid(&self) -> bool {
        let trimmed = self.sas_token.trim();
        // Minimum reasonable length — Azure SAS tokens are always
        // at least a few hundred chars (multiple required params).
        // Catches obvious empty / typo cases without false positives.
        !trimmed.is_empty() && trimmed.len() > 16
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .expect("azure_blob reqwest client builder")
}

/// Normalize the SAS — strip a leading `?` if the user pasted the
/// token with it. Internal endpoints append it back consistently.
fn normalize_sas(sas: &str) -> String {
    let s = sas.trim();
    s.strip_prefix('?').unwrap_or(s).to_string()
}

/// Build the URL the List Blobs API expects for a container scope.
/// `account_url` is the user-supplied base
/// (e.g. `https://myacct.blob.core.windows.net/mycontainer`),
/// `prefix` is the optional path prefix to scope the listing.
fn list_url(account_url: &str, sas: &str, prefix: &str, marker: Option<&str>) -> String {
    let base = account_url.trim_end_matches('/');
    let mut q = format!("?restype=container&comp=list&{}", normalize_sas(sas));
    if !prefix.is_empty() {
        q.push_str(&format!("&prefix={}", urlencoding::encode(prefix)));
    }
    if let Some(m) = marker {
        q.push_str(&format!("&marker={}", urlencoding::encode(m)));
    }
    format!("{}{}", base, q)
}

/// Build the URL for downloading a single blob.
fn blob_url(account_url: &str, sas: &str, blob_name: &str) -> String {
    // Per Azure docs, blob names are URL-encoded except for `/`.
    let encoded: String = blob_name
        .split('/')
        .map(|seg| urlencoding::encode(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/");
    format!(
        "{}/{}?{}",
        account_url.trim_end_matches('/'),
        encoded,
        normalize_sas(sas)
    )
}

/// Pre-flight: hit List Blobs with `maxresults=1`. Cheapest possible
/// authenticated call against the container — confirms the URL is
/// well-formed and the SAS is valid.
pub async fn test_credentials(
    account_url: &str,
    creds: &AzureBlobCredentials,
) -> Result<()> {
    if !creds.is_valid() {
        return Err(AgentError::Config(
            "Azure SAS token is missing or too short".to_string(),
        ));
    }
    let mut url = list_url(account_url, &creds.sas_token, "", None);
    url.push_str("&maxresults=1");
    let resp = client().get(&url).send().await.map_err(|e| {
        AgentError::Network(format!("Azure list test: {e}"))
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Auth(format!(
            "Azure pre-flight rejected (HTTP {status}): {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    Ok(())
}

/// One blob entry from a recursive listing.
#[derive(Debug, Clone)]
pub struct AzureBlobFile {
    /// Blob name relative to the container. Slashes act like
    /// directory separators in object naming convention.
    pub name: String,
    pub size_bytes: u64,
    pub last_modified: Option<String>,
}

/// List every blob in the container under `prefix`. Paginates via
/// the `<NextMarker>` element. Bounded by `max_files`.
pub async fn list_recursive(
    account_url: &str,
    creds: &AzureBlobCredentials,
    prefix: &str,
    max_files: usize,
) -> Result<Vec<AzureBlobFile>> {
    let c = client();
    let mut out: Vec<AzureBlobFile> = Vec::new();
    let mut marker: Option<String> = None;

    loop {
        let url = list_url(account_url, &creds.sas_token, prefix, marker.as_deref());
        let resp = c.get(&url).send().await.map_err(|e| {
            AgentError::Network(format!("Azure list_blobs: {e}"))
        })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(AgentError::Network(format!(
                "Azure list_blobs HTTP {status}: {}",
                body.chars().take(200).collect::<String>()
            )));
        }
        let body = resp.text().await.map_err(|e| {
            AgentError::Network(format!("Azure list_blobs body: {e}"))
        })?;

        let (page_files, next_marker) = parse_list_xml(&body)?;
        for f in page_files {
            if out.len() >= max_files {
                break;
            }
            out.push(f);
        }
        if out.len() >= max_files || next_marker.is_none() {
            break;
        }
        marker = next_marker;
    }
    Ok(out)
}

/// Parse the EnumerationResults XML body into (files, next_marker).
/// Public for unit testing the parser against captured payloads.
pub fn parse_list_xml(body: &str) -> Result<(Vec<AzureBlobFile>, Option<String>)> {
    let doc = roxmltree::Document::parse(body).map_err(|e| {
        AgentError::Serialization(format!("Azure list XML parse: {e}"))
    })?;
    let root = doc.root_element();
    if root.tag_name().name() != "EnumerationResults" {
        return Err(AgentError::Serialization(format!(
            "Azure list XML: unexpected root <{}>",
            root.tag_name().name()
        )));
    }

    let mut files: Vec<AzureBlobFile> = Vec::new();
    let mut next_marker: Option<String> = None;

    for child in root.children() {
        if !child.is_element() {
            continue;
        }
        match child.tag_name().name() {
            "Blobs" => {
                for blob in child.children().filter(|n| n.is_element()) {
                    if blob.tag_name().name() != "Blob" {
                        // Skip BlobPrefix entries — they appear when
                        // delimiter is set, which we don't use.
                        continue;
                    }
                    let mut name: Option<String> = None;
                    let mut size: u64 = 0;
                    let mut last_modified: Option<String> = None;
                    for field in blob.children().filter(|n| n.is_element()) {
                        match field.tag_name().name() {
                            "Name" => {
                                name = field.text().map(|s| s.to_string());
                            }
                            "Properties" => {
                                for prop in
                                    field.children().filter(|n| n.is_element())
                                {
                                    match prop.tag_name().name() {
                                        "Content-Length" => {
                                            size = prop
                                                .text()
                                                .and_then(|s| s.parse().ok())
                                                .unwrap_or(0);
                                        }
                                        "Last-Modified" => {
                                            last_modified =
                                                prop.text().map(|s| s.to_string());
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if let Some(n) = name {
                        files.push(AzureBlobFile {
                            name: n,
                            size_bytes: size,
                            last_modified,
                        });
                    }
                }
            }
            "NextMarker" => {
                let txt = child.text().unwrap_or("").trim();
                if !txt.is_empty() {
                    next_marker = Some(txt.to_string());
                }
            }
            _ => {}
        }
    }
    Ok((files, next_marker))
}

/// Download a single blob to a local path.
pub async fn download_blob(
    account_url: &str,
    creds: &AzureBlobCredentials,
    blob_name: &str,
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
    let url = blob_url(account_url, &creds.sas_token, blob_name);
    let resp = client().get(&url).send().await.map_err(|e| {
        AgentError::Network(format!("Azure download {blob_name}: {e}"))
    })?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(AgentError::Network(format!(
            "Azure download {blob_name}: HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        )));
    }
    // Stream the body to disk in chunks — avoids buffering full
    // blob in memory for multi-GB Azure files.
    let mut local = tokio::fs::File::create(local_path).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create local {}: {e}",
            local_path.display()
        ))
    })?;
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
    }
    local.flush().await.map_err(|e| {
        AgentError::FileSystem(format!("flush local: {e}"))
    })?;
    Ok(total)
}

/// Where the Azure Blob cache lives for a given source.
pub fn cache_dir_for_source(source_id: &str) -> Result<PathBuf> {
    Ok(crate::config::Config::data_dir()?
        .join("azure-cache")
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

/// Walk + download every supported blob under `prefix`. Skips
/// blobs whose size + Last-Modified match the manifest from the
/// previous walk (incremental sync). `progress` (if Some) fires
/// once per supported blob considered.
pub async fn walk_and_download(
    account_url: &str,
    creds: &AzureBlobCredentials,
    prefix: &str,
    source_id: &str,
    progress: Option<WalkProgressCb>,
) -> Result<(PathBuf, usize)> {
    use crate::sync_manifest::SyncManifest;
    use std::collections::HashSet;
    const MAX_AZURE_FILES: usize = 10_000;
    let cache_dir = cache_dir_for_source(source_id)?;
    tokio::fs::create_dir_all(&cache_dir).await.map_err(|e| {
        AgentError::FileSystem(format!(
            "create azure cache dir {}: {e}",
            cache_dir.display()
        ))
    })?;

    let mut manifest = SyncManifest::load(&cache_dir);

    let listing = list_recursive(
        account_url,
        creds,
        prefix.trim_start_matches('/'),
        MAX_AZURE_FILES,
    )
    .await?;

    let normalized_prefix = prefix.trim_start_matches('/').trim_end_matches('/').to_string();

    struct Work {
        name: String,
        local_path: PathBuf,
        label: String,
        key: String,
        mtime_marker: String,
        size: u64,
    }
    let work: Vec<Work> = listing
        .iter()
        .filter_map(|b| {
            let path = PathBuf::from(&b.name);
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
            let rel = if normalized_prefix.is_empty() {
                b.name.clone()
            } else if let Some(s) = b
                .name
                .strip_prefix(&format!("{}/", normalized_prefix))
            {
                s.to_string()
            } else if b.name == normalized_prefix {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&b.name)
                    .to_string()
            } else {
                b.name.clone()
            };
            let local_path = cache_dir.join(&rel);
            let label = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();
            Some(Work {
                name: b.name.clone(),
                local_path,
                label,
                key: b.name.clone(),
                mtime_marker: b.last_modified.clone().unwrap_or_default(),
                size: b.size_bytes,
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

    let manifest_mu = std::sync::Arc::new(Mutex::new(manifest));
    let downloaded_ct = std::sync::Arc::new(AtomicUsize::new(0));
    let considered_ct = std::sync::Arc::new(AtomicUsize::new(0));
    let account_url = account_url.to_string();

    futures::stream::iter(work)
        .for_each_concurrent(MAX_CONCURRENT, |w| {
            let account_url = account_url.clone();
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
                    match download_blob(&account_url, &creds, &w.name, &w.local_path).await {
                        Ok(_) => {
                            downloaded.fetch_add(1, Ordering::Relaxed);
                            let mut m = manifest.lock().expect("manifest poisoned");
                            m.record(w.key.clone(), w.size, w.mtime_marker.clone());
                        }
                        Err(e) => {
                            eprintln!(
                                "[azure] download failed for {}: {} — skipping",
                                w.name, e
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

    // Drop stale entries + their cached files.
    let stale = manifest.drop_missing(&current_keys);
    for stale_key in &stale {
        let rel = if normalized_prefix.is_empty() {
            stale_key.clone()
        } else if let Some(s) = stale_key.strip_prefix(&format!("{}/", normalized_prefix)) {
            s.to_string()
        } else {
            continue;
        };
        let local = cache_dir.join(&rel);
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
    fn is_valid_rejects_empty_or_short() {
        assert!(!AzureBlobCredentials {
            sas_token: "".to_string(),
        }
        .is_valid());
        assert!(!AzureBlobCredentials {
            sas_token: "tooshort".to_string(),
        }
        .is_valid());
    }

    #[test]
    fn is_valid_accepts_realistic_sas() {
        // A typical Azure SAS is several hundred chars; 17 is the
        // floor we set just to catch obvious typos.
        let token = "?sv=2021-08-06&ss=b&srt=co&sp=rl&se=2030-01-01T00%3A00%3A00Z&sig=abcdef";
        assert!(AzureBlobCredentials {
            sas_token: token.to_string(),
        }
        .is_valid());
    }

    #[test]
    fn normalize_strips_leading_question_mark() {
        assert_eq!(normalize_sas("?sv=2021&sig=abc"), "sv=2021&sig=abc");
        assert_eq!(normalize_sas("sv=2021&sig=abc"), "sv=2021&sig=abc");
        assert_eq!(normalize_sas("  ?sv=x  "), "sv=x");
    }

    #[test]
    fn list_url_includes_required_query_params() {
        let url = list_url(
            "https://acct.blob.core.windows.net/cont",
            "?sv=x&sig=y",
            "data/",
            None,
        );
        assert!(url.contains("restype=container"));
        assert!(url.contains("comp=list"));
        assert!(url.contains("sv=x"));
        assert!(url.contains("sig=y"));
        assert!(url.contains("prefix=data%2F"));
    }

    #[test]
    fn list_url_includes_marker_when_paginating() {
        let url = list_url(
            "https://a.blob.core.windows.net/c",
            "sig=x",
            "",
            Some("nextmark"),
        );
        assert!(url.contains("marker=nextmark"));
    }

    #[test]
    fn blob_url_encodes_path_segments_but_keeps_slashes() {
        let url = blob_url(
            "https://a.blob.core.windows.net/c",
            "sig=x",
            "data/sub dir/file with spaces.csv",
        );
        assert!(url.contains("/data/"));
        // Spaces should be encoded
        assert!(url.contains("sub%20dir") || url.contains("sub+dir"));
        assert!(url.contains("file%20with%20spaces.csv") || url.contains("file+with+spaces.csv"));
    }

    #[test]
    fn parse_list_xml_extracts_blobs() {
        // Trimmed real-shape Azure response.
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<EnumerationResults ServiceEndpoint="https://a.blob.core.windows.net/" ContainerName="c">
  <Blobs>
    <Blob>
      <Name>data/sales.csv</Name>
      <Properties>
        <Last-Modified>Tue, 01 Jan 2026 00:00:00 GMT</Last-Modified>
        <Content-Length>12345</Content-Length>
        <Content-Type>text/csv</Content-Type>
      </Properties>
    </Blob>
    <Blob>
      <Name>data/inventory.parquet</Name>
      <Properties>
        <Last-Modified>Wed, 02 Jan 2026 00:00:00 GMT</Last-Modified>
        <Content-Length>67890</Content-Length>
      </Properties>
    </Blob>
  </Blobs>
  <NextMarker/>
</EnumerationResults>"#;
        let (files, next) = parse_list_xml(xml).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].name, "data/sales.csv");
        assert_eq!(files[0].size_bytes, 12345);
        assert!(files[0].last_modified.is_some());
        assert_eq!(files[1].name, "data/inventory.parquet");
        assert_eq!(files[1].size_bytes, 67890);
        assert!(next.is_none());
    }

    #[test]
    fn parse_list_xml_pagination_marker() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<EnumerationResults ServiceEndpoint="x" ContainerName="c">
  <Blobs>
    <Blob>
      <Name>x.csv</Name>
      <Properties>
        <Last-Modified>now</Last-Modified>
        <Content-Length>10</Content-Length>
      </Properties>
    </Blob>
  </Blobs>
  <NextMarker>2!160!MDAwMDIw</NextMarker>
</EnumerationResults>"#;
        let (files, next) = parse_list_xml(xml).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(next, Some("2!160!MDAwMDIw".to_string()));
    }

    #[test]
    fn parse_list_xml_empty_result() {
        let xml = r#"<?xml version="1.0"?>
<EnumerationResults ServiceEndpoint="x" ContainerName="c">
  <Blobs/>
  <NextMarker/>
</EnumerationResults>"#;
        let (files, next) = parse_list_xml(xml).unwrap();
        assert!(files.is_empty());
        assert!(next.is_none());
    }

    #[test]
    fn parse_list_xml_rejects_bad_root() {
        let xml = r#"<?xml version="1.0"?><NotIt/>"#;
        assert!(parse_list_xml(xml).is_err());
    }
}
