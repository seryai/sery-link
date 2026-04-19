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
/// source? We deliberately accept both `http://` and `https://` so users
/// can point at localhost / internal-network endpoints during testing;
/// the UI should visually warn about unencrypted HTTP.
pub fn is_remote_url(path: &str) -> bool {
    let lower = path.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
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
/// source. We're not trying to be a full URL parser — just enough to
/// reject common typos and non-HTTP schemes before hitting the network.
pub fn validate_url(raw: &str) -> UrlValidation {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return UrlValidation::Invalid {
            reason: "URL is empty".to_string(),
        };
    }
    let lower = trimmed.to_ascii_lowercase();
    let insecure = lower.starts_with("http://");
    if !lower.starts_with("http://") && !lower.starts_with("https://") {
        return UrlValidation::Invalid {
            reason: "URL must start with http:// or https://".to_string(),
        };
    }
    // Everything after the scheme needs at least a host — `http://` alone
    // is structurally invalid.
    let after_scheme = if insecure {
        &trimmed[7..]
    } else {
        &trimmed[8..]
    };
    if after_scheme.is_empty() || after_scheme.starts_with('/') {
        return UrlValidation::Invalid {
            reason: "URL is missing a host".to_string(),
        };
    }
    // Canonicalise by lower-casing the scheme but preserving path/query
    // case — many servers are case-sensitive on paths.
    let scheme_end = if insecure { 7 } else { 8 };
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
    fn is_remote_url_rejects_non_http() {
        assert!(!is_remote_url("/Users/x/file.csv"));
        assert!(!is_remote_url("file:///Users/x/file.csv"));
        assert!(!is_remote_url("s3://bucket/key"));
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
            "s3://bucket/key",
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
