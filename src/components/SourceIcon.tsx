// Brand-recognisable icons for each source kind.
//
// Google Drive and Dropbox use simple-icons via @icons-pack/react-simple-icons
// — correct brand geometry, single-color fill with the official hex.
//
// Amazon S3, Azure Blob, and OneDrive were removed from simple-icons at
// Amazon's / Microsoft's trademark request, so those use hand-drawn SVGs
// styled in official brand colors but without copying the trademarked marks.
//
// SFTP, WebDAV, local folder, and HTTPS fall back to lucide icons / custom
// shapes since they represent protocols, not branded products.

import { Folder as FolderIcon, Globe, KeyRound } from 'lucide-react';
import {
  SiGoogledrive,
  SiDropbox,
  SiBackblaze,
  SiWasabi,
  SiCloudflare,
  SiGooglecloudstorage,
  SiMysql,
  SiPostgresql,
  SiRedis,
  SiMongodb,
  SiSqlite,
  SiSnowflake,
  SiClickhouse,
} from '@icons-pack/react-simple-icons';
import type { SourceKind } from '../utils/url';

interface Props {
  kind: SourceKind;
  size?: 'sm' | 'md' | 'lg';
}

const SIZE_PX: Record<NonNullable<Props['size']>, number> = {
  sm: 16,
  md: 20,
  lg: 32,
};

const SIZE_CLASS: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-4 w-4',
  md: 'h-5 w-5',
  lg: 'h-8 w-8',
};

export function SourceIcon({ kind, size = 'md' }: Props) {
  const px = SIZE_PX[size];
  const cls = SIZE_CLASS[size];
  // Inner icon size for DB chips: ~65 % of the chip so there's visible padding.
  const dbPx = Math.max(8, Math.round(px * 0.65));

  switch (kind) {
    case 'gdrive':
      return <SiGoogledrive size={px} color="default" title="Google Drive" />;
    case 'dropbox':
      return <SiDropbox size={px} color="default" title="Dropbox" />;
    case 's3':
      return <AwsS3Mark className={cls} />;
    case 'azure':
      return <AzureBlobMark className={cls} />;
    case 'onedrive':
      return <OneDriveMark className={cls} />;
    case 'http':
      return <Globe className={`${cls} text-slate-500 dark:text-slate-400`} />;
    case 'local':
      return <FolderIcon className={`${cls} text-purple-600 dark:text-purple-300`} />;
    case 'sftp':
      return <KeyRound className={`${cls} text-slate-700 dark:text-slate-300`} />;
    case 'webdav':
      return <WebDavMark className={cls} />;
    // Database types: solid brand-color chip, white icon — self-contained app-icon style.
    case 'mysql':
      return <DbChip cls={cls} bg="#4479A1"><SiMysql size={dbPx} color="#ffffff" title="MySQL" /></DbChip>;
    case 'postgresql':
      return <DbChip cls={cls} bg="#336791"><SiPostgresql size={dbPx} color="#ffffff" title="PostgreSQL" /></DbChip>;
    case 'redis':
      return <DbChip cls={cls} bg="#DC382D"><SiRedis size={dbPx} color="#ffffff" title="Redis" /></DbChip>;
    case 'mongodb':
      return <DbChip cls={cls} bg="#47A248"><SiMongodb size={dbPx} color="#ffffff" title="MongoDB" /></DbChip>;
    case 'sqlite':
      return <DbChip cls={cls} bg="#003B57"><SiSqlite size={dbPx} color="#ffffff" title="SQLite" /></DbChip>;
    case 'snowflake':
      return <DbChip cls={cls} bg="#29B5E8"><SiSnowflake size={dbPx} color="#ffffff" title="Snowflake" /></DbChip>;
    case 'clickhouse':
      return <DbChip cls={cls} bg="#FFCC01"><SiClickhouse size={dbPx} color="#1a1a1a" title="ClickHouse" /></DbChip>;
    default:
      return <FolderIcon className={`${cls} text-slate-400 dark:text-slate-500`} />;
  }
}

/** Solid colored rounded chip that makes thin simple-icons legible at small sizes. */
function DbChip({ cls, bg, children }: { cls: string; bg: string; children: React.ReactNode }) {
  return (
    <div
      className={`${cls} flex flex-shrink-0 items-center justify-center rounded-md`}
      style={{ backgroundColor: bg }}
    >
      {children}
    </div>
  );
}

/** Used by S3-compatible preset sources (B2, Wasabi, R2, GCS) when the
 *  UI knows which provider is behind the S3 endpoint. Not used by the
 *  generic s3 SourceKind directly — that always shows AwsS3Mark. */
export function PresetSourceIcon({
  preset,
  size = 'md',
}: {
  preset: 'backblaze' | 'wasabi' | 'cloudflare' | 'gcs';
  size?: 'sm' | 'md' | 'lg';
}) {
  const px = SIZE_PX[size];
  switch (preset) {
    case 'backblaze':
      return <SiBackblaze size={px} color="default" title="Backblaze B2" />;
    case 'wasabi':
      return <SiWasabi size={px} color="default" title="Wasabi" />;
    case 'cloudflare':
      return <SiCloudflare size={px} color="default" title="Cloudflare R2" />;
    case 'gcs':
      return <SiGooglecloudstorage size={px} color="default" title="Google Cloud Storage" />;
  }
}

/** Background tint for the icon's containing chip. */
export function sourceIconBgClass(kind: SourceKind | string): string {
  switch (kind) {
    case 'gdrive':
      return 'bg-blue-100 dark:bg-blue-950/40';
    case 's3':
      return 'bg-orange-100 dark:bg-orange-950/40';
    case 'http':
      return 'bg-slate-100 dark:bg-slate-800';
    case 'local':
      return 'bg-purple-100 dark:bg-purple-900/40';
    case 'sftp':
      return 'bg-slate-100 dark:bg-slate-800';
    case 'webdav':
      return 'bg-sky-100 dark:bg-sky-950/40';
    case 'dropbox':
      return 'bg-blue-100 dark:bg-blue-950/40';
    case 'azure':
      return 'bg-sky-100 dark:bg-sky-950/40';
    case 'onedrive':
      return 'bg-blue-100 dark:bg-blue-950/40';
    // DB icons render their own solid chip — no outer bg needed.
    case 'mysql':
    case 'postgresql':
    case 'redis':
    case 'mongodb':
    case 'sqlite':
    case 'snowflake':
    case 'clickhouse':
      return '';
    default:
      return 'bg-slate-100 dark:bg-slate-800';
  }
}

// ── Custom SVGs for brands not in simple-icons ─────────────────────────────

/** Amazon S3 — full-bleed orange (#FF9900) cylinder.
 *  simple-icons removed the Amazon trademark; brand color only. */
function AwsS3Mark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" aria-label="Amazon S3">
      {/* Cylinder body — sides */}
      <rect x="2" y="4" width="20" height="16" fill="#FF9900" opacity="0.9" />
      {/* Bottom cap (drawn first so top cap renders on top) */}
      <ellipse cx="12" cy="20" rx="10" ry="3.5" fill="#E68A00" />
      {/* Mid separator ring */}
      <ellipse cx="12" cy="12" rx="10" ry="3.5" fill="#E68A00" opacity="0.5" />
      {/* Top cap */}
      <ellipse cx="12" cy="4" rx="10" ry="3.5" fill="#FFAD33" />
    </svg>
  );
}

/** Azure Blob Storage — full-bleed blue (#0078D4) card with
 *  three storage-row stripes suggesting blob containers. */
function AzureBlobMark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" aria-label="Azure Blob Storage">
      <rect x="2" y="2" width="20" height="20" rx="3" fill="#0078D4" />
      <rect x="5" y="6"    width="14" height="3.5" rx="1.5" fill="white" opacity="0.35" />
      <rect x="5" y="11.5" width="14" height="3.5" rx="1.5" fill="white" opacity="0.35" />
      <rect x="5" y="17"   width="9"  height="3.5" rx="1.5" fill="white" opacity="0.35" />
    </svg>
  );
}

/** Microsoft OneDrive — two overlapping filled clouds in #0078D4.
 *  Microsoft trademark removed from simple-icons; brand color only. */
function OneDriveMark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" aria-label="Microsoft OneDrive">
      {/* Back cloud — lighter, shifted upper-left */}
      <path
        d="M14.5 15H5a4 4 0 0 1-.44-7.97A5 5 0 0 1 14 5.1"
        stroke="#0078D4" strokeWidth="2" strokeLinecap="round" opacity="0.4"
      />
      {/* Front cloud — fills lower portion */}
      <path
        d="M6 19.5H19a4 4 0 0 0 .54-7.96A6 6 0 0 0 8 10.1 4 4 0 0 0 6 18z"
        fill="#0078D4"
      />
    </svg>
  );
}

/** WebDAV — large filled cloud in sky-blue. */
function WebDavMark({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg" aria-label="WebDAV">
      <path
        d="M19.5 9.5a7 7 0 0 0-13.4-1.8A4.5 4.5 0 1 0 6.5 17H19a4 4 0 0 0 .5-7.5z"
        fill="#0EA5E9"
      />
    </svg>
  );
}
