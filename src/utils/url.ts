// TS mirror of `src-tauri/src/url.rs::is_remote_url`. Kept trivially
// in sync — both sides accept `http://` and `https://`, nothing else.
//
// Used by folder-shaped UI surfaces (FolderList, FolderDetail,
// FileDetail) to branch between local-filesystem rendering and
// URL-based rendering without plumbing a separate type all the way
// through.

export function isRemoteUrl(path: string): boolean {
  const lower = path.trim().toLowerCase();
  return (
    lower.startsWith('http://') ||
    lower.startsWith('https://') ||
    lower.startsWith('s3://')
  );
}

export function isS3Url(path: string): boolean {
  return path.trim().toLowerCase().startsWith('s3://');
}

/// Pull a user-meaningful display name out of a URL. Mirrors
/// `src-tauri/src/url.rs::infer_filename_from_url` closely enough for
/// rendering purposes — tiny edge cases (trailing percent-encoded
/// bytes) may differ but are fine for display.
export function filenameFromUrl(url: string): string {
  const withoutQuery = url.split('?')[0];
  const withoutFragment = withoutQuery.split('#')[0];
  const afterScheme = withoutFragment.includes('://')
    ? withoutFragment.split('://')[1] ?? ''
    : withoutFragment;
  const slashIdx = afterScheme.indexOf('/');
  const path = slashIdx >= 0 ? afterScheme.slice(slashIdx) : '';
  const segments = path.split('/').filter(Boolean);
  const last = segments.length > 0 ? segments[segments.length - 1] : '';
  if (!last) return 'remote';
  try {
    return decodeURIComponent(last);
  } catch {
    return last;
  }
}
