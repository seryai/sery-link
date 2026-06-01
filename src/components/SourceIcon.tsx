import type { SourceKind } from '../utils/url';

interface Props {
  kind: SourceKind;
  size?: 'sm' | 'md' | 'lg';
}

const SIZE_CLASS: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-6 w-6',
  md: 'h-8 w-8',
  lg: 'h-10 w-10',
};

// DB icons from the icon library have ~15-20% built-in padding in their viewBox.
// Scale them up so their visual weight matches the tight storage icons.
const DB_KINDS = new Set<SourceKind>(['mysql', 'postgresql', 'redis', 'mongodb', 'sqlite', 'snowflake', 'clickhouse']);

const ICON_SRC: Record<SourceKind, string> = {
  local:      '/icons/storage/local.svg',
  s3:         '/icons/storage/s3.svg',
  http:       '/icons/storage/https.svg',
  gdrive:     '/icons/storage/gdrive.svg',
  sftp:       '/icons/storage/sftp.svg',
  webdav:     '/icons/storage/webdav.svg',
  dropbox:    '/icons/storage/dropbox.svg',
  azure:      '/icons/storage/azure.svg',
  onedrive:   '/icons/storage/onedrive.svg',
  mysql:      '/icons/db/mysql.svg',
  postgresql: '/icons/db/postgres.svg',
  redis:      '/icons/db/redis.svg',
  mongodb:    '/icons/db/mongodb.svg',
  sqlite:     '/icons/db/sqlite.svg',
  snowflake:  '/icons/db/snowflake.svg',
  clickhouse: '/icons/db/clickhouse.svg',
};

const ICON_ALT: Record<SourceKind, string> = {
  local:      'Local folder',
  s3:         'Amazon S3',
  http:       'HTTPS',
  gdrive:     'Google Drive',
  sftp:       'SFTP',
  webdav:     'WebDAV',
  dropbox:    'Dropbox',
  azure:      'Azure Blob Storage',
  onedrive:   'Microsoft OneDrive',
  mysql:      'MySQL',
  postgresql: 'PostgreSQL',
  redis:      'Redis',
  mongodb:    'MongoDB',
  sqlite:     'SQLite',
  snowflake:  'Snowflake',
  clickhouse: 'ClickHouse',
};

export function SourceIcon({ kind, size = 'md' }: Props) {
  const cls = SIZE_CLASS[size];
  return (
    <img
      src={ICON_SRC[kind]}
      alt={ICON_ALT[kind]}
      className={`${cls} flex-shrink-0 object-contain`}
      style={DB_KINDS.has(kind) ? { transform: 'scale(1.25)' } : undefined}
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
