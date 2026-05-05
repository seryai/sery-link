//! URL helpers for remote data sources.
//!
//! Sery Link treats a public HTTP/HTTPS URL as a one-file "folder" so every
//! downstream code path (scan cache, FolderDetail, FileDetail, search) keeps
//! working unchanged. This module centralises URL detection and parsing so
//! the branch points are consistent and unit-testable.
//!
//! Scope (Phase A):
//!   * `http://` and `https://` URLs pointing at a single CSV / Parquet /
//!     XLSX file.
//!   * No credentials, no bucket listing, no wildcards.
//!   * Other schemes (ftp, file, s3, etc.) are rejected at the input boundary.

/// Does this string look like a URL Sery Link can handle as a remote
/// source? Accepts both `http(s)://` (Phase A) and `s3://` (Phase B).
/// We deliberately allow unencrypted HTTP so users can point at
/// localhost / internal-network endpoints during testing — the UI
/// visually warns.
pub fn is_remote_url(path: &str) -> bool {
    let lower = path.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("s3://")
}

/// True only for `s3://…` URLs. S3 sources need credentials stored in
/// the keyring; plain HTTP(S) sources don't. Callers branch on this
/// before running scanner / profile queries so they know whether to
/// load creds into the DuckDB session.
pub fn is_s3_url(path: &str) -> bool {
    path.trim_start().to_ascii_lowercase().starts_with("s3://")
}

/// True when an `s3://` URL denotes a listing (bucket root, prefix,
/// or explicit glob) rather than a single object. The scanner branches
/// on this: listing URLs fan out via `glob(...)` and produce one
/// `DatasetMetadata` per matching object; object URLs fetch one file
/// directly.
///
/// Heuristics:
///   * Ends with `/` → prefix (includes bare-bucket `s3://bucket/`)
///   * Contains `*` → explicit glob
///   * Otherwise → single object
pub fn is_s3_listing(url: &str) -> bool {
    if !is_s3_url(url) {
        return false;
    }
    let trimmed = url.trim();
    trimmed.ends_with('/') || trimmed.contains('*')
}

/// Resolve a `(folder_url, relative_path)` pair back into the full
/// object URL. Inverse of `scanner::relative_key`. Used by the
/// preview / profile dispatch when the source is remote: scans
/// store the LISTING URL as folder_path and the per-object key
/// (relative to the listing prefix) as relative_path, but the
/// DuckDB read functions need the full URL.
///
/// Rules:
///   * If folder_url is a glob pattern (`s3://b/p/*.parquet`),
///     strip back to the prefix (`s3://b/p/`) before joining.
///   * Else ensure folder_url ends with `/`.
///   * If relative_path is empty (single-file source) or already
///     absolute, return folder_url unchanged.
pub fn join_remote_url(folder_url: &str, relative_path: &str) -> String {
    let rel = relative_path.trim_start_matches('/');
    if rel.is_empty() {
        return folder_url.to_string();
    }
    // If the relative happens to already be a full URL (paranoia
    // guard against double-joining), pass through.
    if is_remote_url(rel) {
        return rel.to_string();
    }
    let base = if folder_url.contains('*') {
        match folder_url.rsplit_once('/') {
            Some((root, _)) => format!("{}/", root),
            None => folder_url.to_string(),
        }
    } else if folder_url.ends_with('/') {
        folder_url.to_string()
    } else {
        format!("{}/", folder_url)
    };
    format!("{}{}", base, rel)
}

/// Expand a bucket-prefix listing URL to the glob pattern we'll hand
/// to DuckDB. Only callers should be the scanner's listing path.
///
///   * `s3://bucket/`              → `s3://bucket/**/*`
///   * `s3://bucket/prefix/`       → `s3://bucket/prefix/**/*`
///   * `s3://bucket/**/*.parquet`  → unchanged (already a glob)
///   * `s3://bucket/prefix/*.csv`  → unchanged
///
/// Recursive by default — most user buckets are partitioned
/// (`year=2024/month=01/...`) and a one-level-deep listing missed
/// every dataset, producing the "added with nothing" UX.
///
/// Extension filtering happens Rust-side in `list_s3_blocking`
/// rather than via a glob brace pattern (`*.{csv,parquet}`).
/// DuckDB-httpfs brace expansion against S3 listings was observed
/// returning empty even when matching files existed, so the filter
/// runs against the raw object URLs after the listing comes back.
pub fn expand_s3_listing_pattern(url: &str) -> String {
    if url.contains('*') {
        return url.to_string();
    }
    let base = if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{}/", url)
    };
    format!("{}**/*", base)
}

/// Result of sanitising a user-pasted URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UrlValidation {
    /// URL looks reasonable. Includes the normalised form (trimmed,
    /// scheme lower-cased) that should be persisted to config.
    Ok { normalised: String, insecure: bool },
    /// URL is structurally invalid or uses an unsupported scheme.
    Invalid { reason: String },
}

/// Detect a Google Sheets share URL. Matches the standard share-link
/// shape `https://docs.google.com/spreadsheets/d/{id}/...` — anything
/// with a Sheets file id in the canonical position. Doesn't try to
/// parse pubhtml URLs (`/d/e/{id}/pubhtml`) — those are the older
/// "Publish to web" mechanism and serve their own format; rare enough
/// to defer.
pub fn is_google_sheets_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    (lower.starts_with("https://docs.google.com/spreadsheets/d/")
        || lower.starts_with("http://docs.google.com/spreadsheets/d/"))
        // Reject the pubhtml form; we don't handle it yet and the
        // export-URL rewrite below would be wrong for it.
        && !lower.contains("/d/e/")
}

/// Rewrite a Sheets share URL into the public CSV export endpoint
/// (`.../export?format=csv[&gid=N]`). The scan path then treats the
/// resulting URL like any other public CSV. Only the first tab is
/// returned by CSV export — if the user pasted a URL with `#gid=N`
/// (the URL fragment Sheets uses to deep-link to a tab) we forward
/// that as the `gid` query param so the user gets the tab they were
/// looking at when they copied the link.
///
/// Limitations to flag in the UI:
///   - CSV export = first tab only unless `#gid=N` is in the input
///   - Private sheets return Google's login HTML, not CSV — the
///     remote scanner will error during DuckDB parse; future polish
///     could detect this via Content-Type for a friendlier message.
fn rewrite_google_sheets_url(url: &str) -> Option<String> {
    if !is_google_sheets_url(url) {
        return None;
    }
    let trimmed = url.trim();

    // Pull the file id out of `/spreadsheets/d/{id}/...`. We accept
    // the id as ANY chars between the `/d/` segment and the next `/`
    // or end-of-string; Sheets ids are alphanumeric + - / _, so a
    // permissive cut is safe.
    let after_d = trimmed.find("/d/")?;
    let id_start = after_d + 3;
    let rest = &trimmed[id_start..];
    let id_end_offset = rest
        .find(|c: char| c == '/' || c == '?' || c == '#')
        .unwrap_or(rest.len());
    let id = &rest[..id_end_offset];
    if id.is_empty() {
        return None;
    }

    // Extract gid from the URL fragment if present (Sheets writes it
    // as `#gid=42`). Falls back to None → first tab.
    let gid = trimmed
        .split_once('#')
        .and_then(|(_, frag)| {
            frag.split('&').find_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                if k.eq_ignore_ascii_case("gid") {
                    Some(v.to_string())
                } else {
                    None
                }
            })
        });

    let mut out = format!(
        "https://docs.google.com/spreadsheets/d/{}/export?format=csv",
        id
    );
    if let Some(g) = gid {
        out.push_str("&gid=");
        out.push_str(&g);
    }
    Some(out)
}

/// Sanity-check a user-supplied URL before adding it as a watched
/// source. Accepts `http(s)://` and `s3://`. We're not trying to be a
/// full URL parser — just enough to reject common typos and unsupported
/// schemes before hitting the network.
///
/// Special case: Google Sheets share URLs are rewritten to the CSV
/// export form so the remote scanner can read them as plain CSV
/// without the user having to know the export URL incantation.
pub fn validate_url(raw: &str) -> UrlValidation {
    // Auto-rewrite Sheets share URLs to their CSV export form
    // BEFORE the rest of validation runs. Both the original and the
    // rewritten URL are https:// so we don't bypass any guard.
    let rewritten = rewrite_google_sheets_url(raw);
    let raw = rewritten.as_deref().unwrap_or(raw);

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return UrlValidation::Invalid {
            reason: "URL is empty".to_string(),
        };
    }
    let lower = trimmed.to_ascii_lowercase();

    let (scheme_end, insecure) = if lower.starts_with("https://") {
        (8usize, false)
    } else if lower.starts_with("http://") {
        (7, true)
    } else if lower.starts_with("s3://") {
        (5, false)
    } else {
        return UrlValidation::Invalid {
            reason: "URL must start with https://, http://, or s3://".to_string(),
        };
    };

    // Everything after the scheme needs at least a host (or bucket for
    // s3://). `s3://` or `http://` alone is structurally invalid.
    let after_scheme = &trimmed[scheme_end..];
    if after_scheme.is_empty() || after_scheme.starts_with('/') {
        return UrlValidation::Invalid {
            reason: if lower.starts_with("s3://") {
                "URL is missing a bucket".to_string()
            } else {
                "URL is missing a host".to_string()
            },
        };
    }

    // s3:// URLs must include a bucket name (the part after the scheme).
    // Object key, prefix, or glob are all valid forms — the scanner
    // branches on shape at scan time.
    //
    // Accepted shapes:
    //   * s3://bucket/key.parquet          — single object (B1)
    //   * s3://bucket/                     — bucket listing (B2)
    //   * s3://bucket/prefix/              — prefix listing (B2)
    //   * s3://bucket/prefix/*.parquet     — explicit glob (B2)
    //   * s3://bucket/**/*.parquet         — recursive glob (B2)
    if lower.starts_with("s3://") && after_scheme.split_once('/').is_none()
        && after_scheme.is_empty()
    {
        // Unreachable given the earlier `after_scheme.is_empty()`
        // guard — kept as an explicit invariant for future edits.
        return UrlValidation::Invalid {
            reason: "s3:// URL is missing a bucket".to_string(),
        };
    }

    // Canonicalise by lower-casing the scheme but preserving path/query
    // case — S3 keys and many HTTP paths are case-sensitive.
    let mut normalised = String::with_capacity(trimmed.len());
    normalised.push_str(&lower[..scheme_end]);
    normalised.push_str(&trimmed[scheme_end..]);
    UrlValidation::Ok {
        normalised,
        insecure,
    }
}

/// Try to guess a user-meaningful filename from a URL. Strips the query
/// string, takes the last path segment, decodes common percent-encoded
/// characters. Falls back to `"remote"` if nothing useful can be
/// extracted — the caller usually has a user-supplied label to prefer.
pub fn infer_filename_from_url(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    // Drop the scheme + host so the path is what remains.
    let after_scheme = match without_fragment.find("://") {
        Some(i) => &without_fragment[i + 3..],
        None => without_fragment,
    };
    let path = match after_scheme.find('/') {
        Some(i) => &after_scheme[i..],
        None => "",
    };
    let last = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("remote")
        .to_string();
    if last.is_empty() {
        "remote".to_string()
    } else {
        percent_decode(&last)
    }
}

/// Extract the file extension from a URL, lower-cased, without the dot.
/// Returns `""` if none — the caller should then treat it as an
/// unknown / unsupported format.
///
/// As a fallback for URLs whose path has no `.ext` (Google Sheets
/// export, certain APIs), we honour a `?format=...` query parameter.
/// That makes `…/export?format=csv` resolve to `csv` for the remote
/// scanner's read-function dispatch.
pub fn extension_from_url(url: &str) -> String {
    let filename = infer_filename_from_url(url);
    if let Some((_, ext)) = filename.rsplit_once('.') {
        if !ext.is_empty() {
            return ext.to_ascii_lowercase();
        }
    }
    // Fall back to ?format=... query param.
    if let Some((_, query)) = url.split_once('?') {
        for kv in query.split('&') {
            if let Some((k, v)) = kv.split_once('=') {
                if k.eq_ignore_ascii_case("format") && !v.is_empty() {
                    return v.to_ascii_lowercase();
                }
            }
        }
    }
    String::new()
}

/// Minimal percent-decode for filenames. We only handle the ASCII range
/// — good enough for the "my-file.csv" case and avoids pulling in a
/// full URL-decoding dep for one line of UI.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_remote_url_detects_http_and_https() {
        assert!(is_remote_url("https://example.com/a.csv"));
        assert!(is_remote_url("http://localhost:8000/a.csv"));
        assert!(is_remote_url("HTTPS://example.com/UPPER"));
        assert!(is_remote_url("  https://example.com/a.csv  "));
    }

    #[test]
    fn is_remote_url_rejects_unsupported_schemes() {
        assert!(!is_remote_url("/Users/x/file.csv"));
        assert!(!is_remote_url("file:///Users/x/file.csv"));
        assert!(!is_remote_url("ftp://example.com/file.csv"));
        assert!(!is_remote_url("gs://bucket/key"));
        assert!(!is_remote_url(""));
    }

    #[test]
    fn validate_url_happy_path() {
        match validate_url("  https://example.com/data.csv  ") {
            UrlValidation::Ok {
                normalised,
                insecure,
            } => {
                assert_eq!(normalised, "https://example.com/data.csv");
                assert!(!insecure);
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn validate_url_flags_insecure_http() {
        match validate_url("http://localhost:9000/x") {
            UrlValidation::Ok { insecure, .. } => assert!(insecure),
            other => panic!("expected Ok(insecure=true), got {:?}", other),
        }
    }

    #[test]
    fn validate_url_rejects_unsupported_schemes() {
        for bad in &[
            "ftp://example.com/data.csv",
            "file:///tmp/data.csv",
            "gs://bucket/key",
            "data.csv",
            "",
            "  ",
            "  \t  ",
        ] {
            match validate_url(bad) {
                UrlValidation::Invalid { .. } => {}
                UrlValidation::Ok { .. } => panic!("should have rejected: {:?}", bad),
            }
        }
    }

    #[test]
    fn is_s3_url_detects_scheme() {
        assert!(is_s3_url("s3://bucket/key.parquet"));
        assert!(is_s3_url("S3://bucket/key"));
        assert!(!is_s3_url("https://example.com/file.csv"));
        assert!(!is_s3_url("/local/path"));
    }

    #[test]
    fn is_remote_url_accepts_s3() {
        assert!(is_remote_url("s3://bucket/key"));
    }

    #[test]
    fn validate_url_accepts_s3_object() {
        match validate_url("s3://my-bucket/path/to/file.parquet") {
            UrlValidation::Ok {
                normalised,
                insecure,
            } => {
                assert_eq!(normalised, "s3://my-bucket/path/to/file.parquet");
                assert!(!insecure);
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn validate_url_accepts_s3_prefix_and_glob_forms() {
        for ok in &[
            "s3://mybucket/",                          // bucket listing
            "s3://mybucket/prefix/",                   // prefix listing
            "s3://mybucket/prefix/*.parquet",          // explicit glob
            "s3://mybucket/**/*.parquet",              // recursive glob
            "s3://mybucket/prefix/file.parquet",       // single object
        ] {
            match validate_url(ok) {
                UrlValidation::Ok { .. } => {}
                other => panic!("should have accepted: {:?} → {:?}", ok, other),
            }
        }
    }

    #[test]
    fn validate_url_rejects_bare_s3_without_bucket() {
        for bad in &["s3://", "s3:///key"] {
            match validate_url(bad) {
                UrlValidation::Invalid { .. } => {}
                other => panic!("should have rejected: {:?} → {:?}", bad, other),
            }
        }
    }

    #[test]
    fn is_s3_listing_detects_prefix_and_glob() {
        assert!(is_s3_listing("s3://bucket/"));
        assert!(is_s3_listing("s3://bucket/prefix/"));
        assert!(is_s3_listing("s3://bucket/*.parquet"));
        assert!(is_s3_listing("s3://bucket/**/*.parquet"));
        assert!(!is_s3_listing("s3://bucket/prefix/file.parquet"));
        assert!(!is_s3_listing("https://example.com/path/"));
    }

    #[test]
    fn expand_s3_listing_pattern_defaults_to_recursive_all() {
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/"),
            "s3://bucket/**/*"
        );
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/prefix/"),
            "s3://bucket/prefix/**/*"
        );
        // Already a glob — left alone.
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/**/*.parquet"),
            "s3://bucket/**/*.parquet"
        );
        // Bare prefix with no trailing slash — append one.
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/prefix"),
            "s3://bucket/prefix/**/*"
        );
    }

    #[test]
    fn validate_url_requires_host() {
        match validate_url("https://") {
            UrlValidation::Invalid { .. } => {}
            other => panic!("expected Invalid, got {:?}", other),
        }
        match validate_url("https:///just/a/path") {
            UrlValidation::Invalid { .. } => {}
            other => panic!("expected Invalid, got {:?}", other),
        }
    }

    #[test]
    fn infer_filename_from_url_various() {
        assert_eq!(
            infer_filename_from_url("https://example.com/data.csv"),
            "data.csv"
        );
        assert_eq!(
            infer_filename_from_url("https://example.com/path/to/sales-2024.parquet"),
            "sales-2024.parquet"
        );
        // Query string / fragment stripped.
        assert_eq!(
            infer_filename_from_url("https://example.com/foo.csv?token=abc&x=1"),
            "foo.csv"
        );
        assert_eq!(
            infer_filename_from_url("https://example.com/bar.csv#section"),
            "bar.csv"
        );
        // Trailing slash — fall back to a reasonable label.
        assert_eq!(
            infer_filename_from_url("https://example.com/"),
            "remote"
        );
        assert_eq!(infer_filename_from_url("https://example.com"), "remote");
        // Percent-encoded path segment.
        assert_eq!(
            infer_filename_from_url("https://example.com/my%20file.csv"),
            "my file.csv"
        );
    }

    #[test]
    fn extension_from_url_various() {
        assert_eq!(extension_from_url("https://x.com/a.csv"), "csv");
        assert_eq!(extension_from_url("https://x.com/a.Parquet"), "parquet");
        assert_eq!(extension_from_url("https://x.com/a.csv?foo=1"), "csv");
        assert_eq!(extension_from_url("https://x.com/no_extension"), "");
        assert_eq!(extension_from_url("https://x.com/"), "");
    }

    #[test]
    fn join_remote_url_handles_listing_shapes() {
        // S3 listing with trailing slash + nested key — the bug that
        // surfaced as "can't profile remote unknown files" before the
        // join was wired into profile_blocking.
        assert_eq!(
            join_remote_url("s3://bucket/data/", "2024/sales.parquet"),
            "s3://bucket/data/2024/sales.parquet"
        );
        // No trailing slash — caller forgot it; we add one.
        assert_eq!(
            join_remote_url("s3://bucket/data", "sales.parquet"),
            "s3://bucket/data/sales.parquet"
        );
        // Glob pattern as base — strip back to the prefix before
        // joining, matching scanner::relative_key's inverse.
        assert_eq!(
            join_remote_url("s3://bucket/data/*.parquet", "2024/sales.parquet"),
            "s3://bucket/data/2024/sales.parquet"
        );
        // Single-file source: empty relative_path returns folder
        // unchanged (synthetic display filename was dropped).
        assert_eq!(
            join_remote_url("https://example.com/data.parquet", ""),
            "https://example.com/data.parquet"
        );
        // Defensive: relative is already a full URL (paranoia guard
        // against double-joining if a caller mis-passes).
        assert_eq!(
            join_remote_url("s3://bucket/data/", "s3://bucket/data/sales.parquet"),
            "s3://bucket/data/sales.parquet"
        );
        // Leading slash on the relative gets normalized.
        assert_eq!(
            join_remote_url("s3://bucket/data/", "/2024/sales.parquet"),
            "s3://bucket/data/2024/sales.parquet"
        );
    }

    #[test]
    fn extension_from_url_falls_back_to_format_query() {
        // The Sheets export endpoint has no extension in the path —
        // the format lives in `?format=csv`. Without this fallback,
        // the remote scanner errors with "unsupported file type".
        assert_eq!(
            extension_from_url(
                "https://docs.google.com/spreadsheets/d/abc/export?format=csv"
            ),
            "csv"
        );
        assert_eq!(
            extension_from_url(
                "https://api.example.com/data?token=x&format=parquet"
            ),
            "parquet"
        );
    }

    #[test]
    fn detects_google_sheets_share_urls() {
        assert!(is_google_sheets_url(
            "https://docs.google.com/spreadsheets/d/abc123/edit"
        ));
        assert!(is_google_sheets_url(
            "https://docs.google.com/spreadsheets/d/abc123/edit#gid=42"
        ));
        assert!(is_google_sheets_url(
            "https://docs.google.com/spreadsheets/d/abc123"
        ));
        // Case-insensitive on the host.
        assert!(is_google_sheets_url(
            "HTTPS://docs.google.com/spreadsheets/d/abc/edit"
        ));
    }

    #[test]
    fn rejects_non_sheets_google_urls() {
        // Drive file viewer
        assert!(!is_google_sheets_url(
            "https://drive.google.com/file/d/abc/view"
        ));
        // Pubhtml (older publish-to-web) — different shape, deferred
        assert!(!is_google_sheets_url(
            "https://docs.google.com/spreadsheets/d/e/2PACX/pubhtml"
        ));
        // Plain remote CSV
        assert!(!is_google_sheets_url("https://example.com/data.csv"));
    }

    #[test]
    fn validate_url_rewrites_sheets_to_csv_export() {
        match validate_url("https://docs.google.com/spreadsheets/d/abc123/edit") {
            UrlValidation::Ok { normalised, .. } => {
                assert_eq!(
                    normalised,
                    "https://docs.google.com/spreadsheets/d/abc123/export?format=csv"
                );
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn validate_url_preserves_sheets_gid_via_fragment() {
        // User pasted a URL with the tab fragment — the export URL
        // should carry the gid forward so they get the tab they were
        // looking at, not the first tab.
        match validate_url(
            "https://docs.google.com/spreadsheets/d/abc/edit?usp=sharing#gid=789",
        ) {
            UrlValidation::Ok { normalised, .. } => {
                assert_eq!(
                    normalised,
                    "https://docs.google.com/spreadsheets/d/abc/export?format=csv&gid=789"
                );
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }
}
