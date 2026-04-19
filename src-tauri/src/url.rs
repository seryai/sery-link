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

/// Expand a bucket-prefix listing URL to the glob pattern we'll hand
/// to DuckDB. Only callers should be the scanner's listing path.
///
///   * `s3://bucket/`              → `s3://bucket/*.{csv,parquet}`
///   * `s3://bucket/prefix/`       → `s3://bucket/prefix/*.{csv,parquet}`
///   * `s3://bucket/**/*.parquet`  → unchanged (already a glob)
///   * `s3://bucket/prefix/*.csv`  → unchanged
///
/// We deliberately default to one-level-deep listing. Users who want
/// recursive can paste `**/` explicitly — matches DuckDB's glob
/// semantics and avoids surprise bills on buckets with millions of
/// nested objects.
pub fn expand_s3_listing_pattern(url: &str) -> String {
    if url.contains('*') {
        return url.to_string();
    }
    let base = if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{}/", url)
    };
    format!("{}*.{{csv,parquet}}", base)
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

/// Sanity-check a user-supplied URL before adding it as a watched
/// source. Accepts `http(s)://` and `s3://`. We're not trying to be a
/// full URL parser — just enough to reject common typos and unsupported
/// schemes before hitting the network.
pub fn validate_url(raw: &str) -> UrlValidation {
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
pub fn extension_from_url(url: &str) -> String {
    let filename = infer_filename_from_url(url);
    match filename.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
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
    fn expand_s3_listing_pattern_defaults_one_level_deep() {
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/"),
            "s3://bucket/*.{csv,parquet}"
        );
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/prefix/"),
            "s3://bucket/prefix/*.{csv,parquet}"
        );
        // Already a glob — left alone.
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/**/*.parquet"),
            "s3://bucket/**/*.parquet"
        );
        // Bare prefix with no trailing slash — append one.
        assert_eq!(
            expand_s3_listing_pattern("s3://bucket/prefix"),
            "s3://bucket/prefix/*.{csv,parquet}"
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
}
