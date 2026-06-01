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
  | 'onedrive'
  | 'mysql'
  | 'postgresql'
  | 'redis'
  | 'mongodb'
  | 'sqlite'
  | 'snowflake'
  | 'clickhouse'
  // JDBC agent-based databases — driver_key values from agent_catalog.rs
  | 'oracle' | 'oracle-legacy' | 'oracle-10g'
  | 'db2' | 'informix' | 'saphana' | 'teradata' | 'vertica'
  | 'databricks' | 'trino' | 'hive' | 'bigquery'
  | 'cassandra' | 'neo4j' | 'firebird' | 'exasol' | 'h2'
  | 'kylin' | 'access' | 'dameng' | 'kingbase' | 'highgo'
  | 'vastbase' | 'goldendb' | 'oceanbase-oracle' | 'gbase'
  | 'sundb' | 'yashandb' | 'tdengine' | 'xugu' | 'mongodb-jdbc';

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
    case 'mysql':
      return 'MySQL';
    case 'postgresql':
      return 'PostgreSQL';
    case 'redis':
      return 'Redis';
    case 'mongodb':
      return 'MongoDB';
    case 'sqlite':
      return 'SQLite';
    case 'snowflake':
      return 'Snowflake';
    case 'clickhouse':
      return 'ClickHouse';
    case 'oracle': return 'Oracle';
    case 'oracle-legacy': return 'Oracle Legacy';
    case 'oracle-10g': return 'Oracle 10g';
    case 'db2': return 'IBM DB2';
    case 'informix': return 'IBM Informix';
    case 'saphana': return 'SAP HANA';
    case 'teradata': return 'Teradata';
    case 'vertica': return 'Vertica';
    case 'databricks': return 'Databricks SQL';
    case 'trino': return 'Trino (Presto)';
    case 'hive': return 'Apache Hive';
    case 'bigquery': return 'Google BigQuery';
    case 'cassandra': return 'Apache Cassandra';
    case 'neo4j': return 'Neo4j';
    case 'firebird': return 'Firebird';
    case 'exasol': return 'Exasol';
    case 'h2': return 'H2';
    case 'kylin': return 'Apache Kylin';
    case 'access': return 'Microsoft Access';
    case 'dameng': return 'Dameng DM8';
    case 'kingbase': return 'KingbaseES';
    case 'highgo': return 'HighGo';
    case 'vastbase': return 'Vastbase';
    case 'goldendb': return 'GoldenDB';
    case 'oceanbase-oracle': return 'OceanBase Oracle';
    case 'gbase': return 'GBase';
    case 'sundb': return 'SunDB';
    case 'yashandb': return 'YashanDB';
    case 'tdengine': return 'TDengine';
    case 'xugu': return 'XuguDB';
    case 'mongodb-jdbc': return 'MongoDB (Legacy)';
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
