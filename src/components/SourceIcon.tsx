import type { SourceKind } from '../utils/url';

interface Props {
  kind: SourceKind | string;
  size?: 'sm' | 'md' | 'lg';
}

const SIZE_CLASS: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-6 w-6',
  md: 'h-8 w-8',
  lg: 'h-10 w-10',
};

// DB icons from the icon library have ~15-20% built-in padding in their viewBox.
// Scale them up so their visual weight matches the tight storage icons.
const DB_KINDS = new Set<SourceKind>([
  'mysql', 'postgresql', 'redis', 'mongodb', 'sqlite', 'snowflake', 'clickhouse',
  'oracle', 'oracle-legacy', 'oracle-10g', 'db2', 'informix', 'saphana', 'teradata',
  'vertica', 'databricks', 'trino', 'hive', 'bigquery', 'cassandra', 'neo4j',
  'firebird', 'exasol', 'h2', 'kylin', 'access', 'dameng', 'kingbase', 'highgo',
  'vastbase', 'goldendb', 'oceanbase-oracle', 'gbase', 'sundb', 'yashandb',
  'tdengine', 'xugu', 'mongodb-jdbc',
]);

const ICON_SRC: Record<SourceKind, string> = {
  local:             '/icons/storage/local.svg',
  s3:                '/icons/storage/s3.svg',
  http:              '/icons/storage/https.svg',
  gdrive:            '/icons/storage/gdrive.svg',
  sftp:              '/icons/storage/sftp.svg',
  webdav:            '/icons/storage/webdav.svg',
  dropbox:           '/icons/storage/dropbox.svg',
  azure:             '/icons/storage/azure.svg',
  onedrive:          '/icons/storage/onedrive.svg',
  mysql:             '/icons/db/mysql.svg',
  postgresql:        '/icons/db/postgres.svg',
  redis:             '/icons/db/redis.svg',
  mongodb:           '/icons/db/mongodb.svg',
  sqlite:            '/icons/db/sqlite.svg',
  snowflake:         '/icons/db/snowflake.svg',
  clickhouse:        '/icons/db/clickhouse.svg',
  oracle:            '/icons/db/oracle.svg',
  'oracle-legacy':   '/icons/db/oracle.svg',
  'oracle-10g':      '/icons/db/oracle.svg',
  db2:               '/icons/db/db2.svg',
  informix:          '/icons/db/informix.svg',
  saphana:           '/icons/db/saphana.webp',
  teradata:          '/icons/db/teradata.webp',
  vertica:           '/icons/db/vertica.webp',
  databricks:        '/icons/db/databricks.webp',
  trino:             '/icons/db/presto.svg',
  hive:              '/icons/db/hive.svg',
  bigquery:          '/icons/db/bigquery.svg',
  cassandra:         '/icons/db/cassandra.svg',
  neo4j:             '/icons/db/neo4j.svg',
  firebird:          '/icons/db/firebird.webp',
  exasol:            '/icons/db/exasol.webp',
  h2:                '/icons/db/h2.svg',
  kylin:             '/icons/db/apache_kylin.svg',
  access:            '/icons/db/access.png',
  dameng:            '/icons/db/dm.svg',
  kingbase:          '/icons/db/kingbase.svg',
  highgo:            '/icons/db/highgo.png',
  vastbase:          '/icons/db/vastbase.png',
  goldendb:          '/icons/db/goldendb.png',
  'oceanbase-oracle':'/icons/db/oceanbase.svg',
  gbase:             '/icons/db/gbase.webp',
  sundb:             '/icons/db/sundb.svg',
  yashandb:          '/icons/db/yashandb.png',
  tdengine:          '/icons/db/tdengine.svg',
  xugu:              '/icons/db/xugu.png',
  'mongodb-jdbc':    '/icons/db/mongodb.svg',
};

const ICON_ALT: Record<SourceKind, string> = {
  local:             'Local folder',
  s3:                'Amazon S3',
  http:              'HTTPS',
  gdrive:            'Google Drive',
  sftp:              'SFTP',
  webdav:            'WebDAV',
  dropbox:           'Dropbox',
  azure:             'Azure Blob Storage',
  onedrive:          'Microsoft OneDrive',
  mysql:             'MySQL',
  postgresql:        'PostgreSQL',
  redis:             'Redis',
  mongodb:           'MongoDB',
  sqlite:            'SQLite',
  snowflake:         'Snowflake',
  clickhouse:        'ClickHouse',
  oracle:            'Oracle',
  'oracle-legacy':   'Oracle Legacy',
  'oracle-10g':      'Oracle 10g',
  db2:               'IBM DB2',
  informix:          'IBM Informix',
  saphana:           'SAP HANA',
  teradata:          'Teradata',
  vertica:           'Vertica',
  databricks:        'Databricks SQL',
  trino:             'Trino (Presto)',
  hive:              'Apache Hive',
  bigquery:          'Google BigQuery',
  cassandra:         'Apache Cassandra',
  neo4j:             'Neo4j',
  firebird:          'Firebird',
  exasol:            'Exasol',
  h2:                'H2',
  kylin:             'Apache Kylin',
  access:            'Microsoft Access',
  dameng:            'Dameng DM8',
  kingbase:          'KingbaseES',
  highgo:            'HighGo',
  vastbase:          'Vastbase',
  goldendb:          'GoldenDB',
  'oceanbase-oracle':'OceanBase Oracle',
  gbase:             'GBase',
  sundb:             'SunDB',
  yashandb:          'YashanDB',
  tdengine:          'TDengine',
  xugu:              'XuguDB',
  'mongodb-jdbc':    'MongoDB (Legacy)',
};

export function SourceIcon({ kind, size = 'md' }: Props) {
  const cls = SIZE_CLASS[size];
  const src = (ICON_SRC as Record<string, string>)[kind] ?? '/icons/db/duckdb.svg';
  const alt = (ICON_ALT as Record<string, string>)[kind] ?? kind;
  return (
    <img
      src={src}
      alt={alt}
      className={`${cls} flex-shrink-0 object-contain`}
      style={DB_KINDS.has(kind as SourceKind) ? { transform: 'scale(1.25)' } : undefined}
    />
  );
}

/** Used by S3-compatible preset sources (B2, Wasabi, R2, GCS). */
export function PresetSourceIcon({
  preset,
  size = 'md',
}: {
  preset: 'backblaze' | 'wasabi' | 'cloudflare' | 'gcs';
  size?: 'sm' | 'md' | 'lg';
}) {
  const cls = SIZE_CLASS[size];
  const map: Record<typeof preset, [string, string]> = {
    backblaze: ['/icons/storage/b2.svg',     'Backblaze B2'],
    wasabi:    ['/icons/storage/wasabi.svg',  'Wasabi'],
    cloudflare:['/icons/storage/r2.svg',      'Cloudflare R2'],
    gcs:       ['/icons/storage/gcs.svg',     'Google Cloud Storage'],
  };
  const [src, alt] = map[preset];
  return <img src={src} alt={alt} className={`${cls} flex-shrink-0 object-contain`} />;
}

/** Background tint for the icon's containing chip. All icons now carry
 *  their own background, so callers that still use this can pass '' safely. */
export function sourceIconBgClass(_kind: SourceKind | string): string {
  return '';
}
