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

/** Categorise a watched-folder path so the UI can render the right
 *  icon + label. Drive lives under ~/.seryai/gdrive-cache/ on disk
 *  but conceptually is its own source type — we don't want it
 *  showing up labeled "local folder" with a cryptic cache path. */
export type SourceKind =
  | 'local'
  | 's3'
  | 'http'
  | 'gdrive'
  // F43-F49 cache-and-scan kinds. classifySource (used by FolderList
  // legacy UI) doesn't return these — they only come from the
  // SourcesSidebar via legacyKindStringOf in utils/sources.ts.
  | 'sftp'
  | 'webdav'
  | 'dropbox'
  | 'azure'
  | 'onedrive';

export function classifySource(path: string): SourceKind {
  const lower = path.trim().toLowerCase();
  if (lower.startsWith('s3://')) return 's3';
  if (lower.startsWith('http://') || lower.startsWith('https://')) return 'http';
  // Drive cache root: /<home>/.seryai/gdrive-cache/<account>. Match
  // on the suffix segment so we work whether ~ has been expanded
  // (it usually has by the time path lands in config).
  if (path.includes('/.seryai/gdrive-cache/')) return 'gdrive';
  return 'local';
}

/** Human label for the source-type pill / subtitle. The legacy
 *  FolderList UI only ever passes one of the original 4 kinds (its
 *  classifySource doesn't return the F43-F49 variants). The new
 *  kinds are listed for type exhaustiveness; the new SourcesSidebar
 *  uses utils/sources.ts:sourceKindLabel which reads the structured
 *  DataSource. */
export function sourceKindLabel(kind: SourceKind): string {
  switch (kind) {
    case 'gdrive':
      return 'Google Drive';
    case 's3':
      return 'Amazon S3';
    case 'http':
      return 'Web URL';
    case 'local':
      return 'Local folder';
    case 'sftp':
      return 'SFTP';
    case 'webdav':
      return 'WebDAV';
    case 'dropbox':
      return 'Dropbox';
    case 'azure':
      return 'Azure Blob';
    case 'onedrive':
      return 'OneDrive';
  }
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
