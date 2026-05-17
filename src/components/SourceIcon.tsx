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
    default:
      return <FolderIcon className={`${cls} text-slate-400 dark:text-slate-500`} />;
  }
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
    default:
      return 'bg-slate-100 dark:bg-slate-800';
  }
}

// ── Custom SVGs for brands not in simple-icons ─────────────────────────────

/** Amazon S3 — orange (#FF9900) cylinder representing object storage.
 *  simple-icons removed the Amazon trademark; this uses brand color only. */
function AwsS3Mark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Amazon S3"
    >
      {/* Top ellipse */}
      <ellipse cx="12" cy="5" rx="8" ry="2.5" fill="#FF9900" />
      {/* Cylinder body */}
      <path d="M4 5 L4 17 Q4 19.5 12 19.5 Q20 19.5 20 17 L20 5 Q20 7.5 12 7.5 Q4 7.5 4 5Z" fill="#FF9900" opacity="0.85" />
      {/* Bottom ellipse cap */}
      <ellipse cx="12" cy="17" rx="8" ry="2.5" fill="#FF9900" opacity="0.7" />
      {/* Mid line to add depth */}
      <ellipse cx="12" cy="11" rx="8" ry="2.5" fill="none" stroke="#FF6600" strokeWidth="0.5" opacity="0.6" />
    </svg>
  );
}

/** Azure Blob Storage — blue (#0078D4) geometric mark.
 *  Evokes Azure's layered/stacked container metaphor. */
function AzureBlobMark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Azure Blob Storage"
    >
      {/* Large back square rotated as a diamond */}
      <rect x="6" y="6" width="12" height="12" rx="1.5"
        fill="#0078D4" opacity="0.3" transform="rotate(45 12 12)" />
      {/* Front square */}
      <rect x="7" y="7" width="10" height="10" rx="1.5" fill="#0078D4" />
      {/* Inner highlight */}
      <rect x="9.5" y="9.5" width="5" height="5" rx="1" fill="#50B0F0" opacity="0.5" />
    </svg>
  );
}

/** Microsoft OneDrive — two overlapping clouds in OneDrive blue (#0078D4).
 *  Microsoft trademark removed from simple-icons; uses brand color only. */
function OneDriveMark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Microsoft OneDrive"
    >
      {/* Back cloud (lighter) */}
      <path
        d="M9.5 16H6a3 3 0 0 1-.39-5.974A4 4 0 0 1 13.5 9.1"
        stroke="#0078D4"
        strokeWidth="1.5"
        strokeLinecap="round"
        opacity="0.45"
      />
      {/* Front cloud */}
      <path
        d="M8 19H18a3.5 3.5 0 0 0 .477-6.967A5 5 0 0 0 9.1 11.5a3.5 3.5 0 0 0-1.1 7.5Z"
        fill="#0078D4"
      />
    </svg>
  );
}

/** WebDAV — cloud with folder inside, sky-blue. */
function WebDavMark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="WebDAV"
    >
      <g className="text-sky-600 dark:text-sky-400">
        <path d="M6.5 19a4 4 0 0 1-.5-7.95A5 5 0 0 1 15.5 10a3.5 3.5 0 0 1 .5 6.95" stroke="currentColor" />
        <path d="M10 16 L10 21 M10 21 L8 19 M10 21 L12 19" stroke="currentColor" />
      </g>
    </svg>
  );
}
