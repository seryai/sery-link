// Brand-recognisable icons for each source kind.
//
// lucide-react doesn't ship logos for third-party services (and
// Microsoft / Google / Dropbox brand guidelines disallow heavy
// modification anyway), so the cloud-source icons are inline
// simplified SVGs styled in official-ish brand colors. The local +
// URL paths fall back to lucide icons we already use elsewhere.
//
// We deliberately don't use the trademarked logos directly — these
// are simplified geometric / color-coded marks that make each
// source visually distinct in the sidebar without crossing brand
// guidelines.

import { Folder as FolderIcon, Globe, KeyRound } from 'lucide-react';
import type { SourceKind } from '../utils/url';

interface Props {
  kind: SourceKind;
  /** Tailwind size — defaults to h-5 w-5 to match the FolderList
   *  card. Pass `lg` for the empty-state hero. */
  size?: 'sm' | 'md' | 'lg';
}

const SIZE_CLASS: Record<NonNullable<Props['size']>, string> = {
  sm: 'h-4 w-4',
  md: 'h-5 w-5',
  lg: 'h-8 w-8',
};

export function SourceIcon({ kind, size = 'md' }: Props) {
  const cls = SIZE_CLASS[size];
  switch (kind) {
    case 'gdrive':
      return <GoogleDriveLogo className={cls} />;
    case 's3':
      return <AwsS3Logo className={cls} />;
    case 'http':
      return <Globe className={`${cls} text-slate-500 dark:text-slate-400`} />;
    case 'local':
      return (
        <FolderIcon className={`${cls} text-purple-600 dark:text-purple-300`} />
      );
    case 'sftp':
      return <SftpKeyMark className={cls} />;
    case 'webdav':
      return <WebDavCloudFolder className={cls} />;
    case 'dropbox':
      return <DropboxBlueDiamond className={cls} />;
    case 'azure':
      return <AzureBlobMark className={cls} />;
    case 'onedrive':
      return <OneDriveCloudMark className={cls} />;
  }
}

/** Background tint for the icon's containing chip — matches the
 *  brand-ish color of the icon so the visual weight is consistent. */
export function sourceIconBgClass(kind: SourceKind): string {
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
      return 'bg-cyan-100 dark:bg-cyan-950/40';
    case 'onedrive':
      return 'bg-indigo-100 dark:bg-indigo-950/40';
  }
}

// ── Brand SVGs ─────────────────────────────────────────────────────
//
// The Google Drive logo is the standard tri-color triangle (blue,
// green, yellow). We use the geometric shape with the official
// hex colors. Simplified path — not pixel-identical to Google's
// reference SVG, but readable as Drive at 16-32px.
function GoogleDriveLogo({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Google Drive"
    >
      {/* Yellow left */}
      <path d="M3.5 14.5 L8 6.5 L13 6.5 L8.5 14.5 Z" fill="#FBBC04" />
      {/* Blue bottom */}
      <path d="M3.5 14.5 L8.5 14.5 L11 19 L6 19 Z" fill="#1FA463" />
      <path d="M11 19 L6 19 L8.5 14.5 L20 14.5 L17.5 19 Z" fill="#4285F4" />
      {/* Green right */}
      <path d="M13 6.5 L8.5 14.5 L20 14.5 L15.5 6.5 Z" fill="#34A853" />
    </svg>
  );
}

/** Simplified Amazon S3 "bucket" mark in the AWS S3 orange. We
 *  draw a stylised bucket — official AWS guidelines forbid using
 *  the trademarked logo without permission, so we use a generic
 *  bucket shape in the brand-recognisable color instead. */
function AwsS3Logo({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Amazon S3"
    >
      <g className="text-orange-600 dark:text-orange-400">
        {/* Bucket outline */}
        <path d="M5 7 L7 19 L17 19 L19 7 Z" stroke="currentColor" />
        {/* Liquid line */}
        <path d="M5.5 10 L18.5 10" stroke="currentColor" />
        {/* Top oval */}
        <ellipse cx="12" cy="7" rx="7" ry="1.5" stroke="currentColor" />
      </g>
    </svg>
  );
}

/** SFTP icon — a key over a server-like rectangle. Distinguishes
 *  from local folder (which is a generic folder mark) by the key,
 *  which signals "credentialed remote". Slate gray to match the
 *  technical / infrastructure aesthetic. */
function SftpKeyMark({ className }: { className?: string }) {
  return (
    <KeyRound
      className={`${className} text-slate-700 dark:text-slate-300`}
    />
  );
}

/** WebDAV — a cloud with a folder cutout. Sky-blue: WebDAV's
 *  origin is "files over HTTP" so the cloud-and-folder pairing
 *  reads as "cloud-hosted files". */
function WebDavCloudFolder({ className }: { className?: string }) {
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
        {/* Cloud outline */}
        <path d="M7 17 a3.5 3.5 0 0 1 0.5 -6.95 a4.5 4.5 0 0 1 8.5 -1.55 a3.5 3.5 0 0 1 0.5 6.95 Z" stroke="currentColor" />
        {/* Folder tab inside the cloud */}
        <rect x="9" y="11" width="6" height="4" rx="0.5" stroke="currentColor" fill="none" />
        <path d="M9 11 L11 11 L11.5 12 L15 12" stroke="currentColor" />
      </g>
    </svg>
  );
}

/** Dropbox — the recognizable blue-diamond folded shape. Two
 *  triangular halves meeting at a horizontal seam. Approximates
 *  Dropbox's logo geometry without using their trademarked mark. */
function DropboxBlueDiamond({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="Dropbox"
    >
      {/* Top half — two triangles forming a mountain */}
      <path d="M2 8 L6 4 L12 8 L8 12 Z" fill="#0061FF" />
      <path d="M22 8 L18 4 L12 8 L16 12 Z" fill="#0061FF" />
      {/* Bottom half — mirrored */}
      <path d="M2 16 L6 20 L12 16 L8 12 Z" fill="#0061FF" />
      <path d="M22 16 L18 20 L12 16 L16 12 Z" fill="#0061FF" />
    </svg>
  );
}

/** Azure Blob — a stack of cylinders evoking blob storage, in
 *  Azure cyan. Reuses the "containers stacked" metaphor that's
 *  common in Azure marketing without copying the official mark. */
function AzureBlobMark({ className }: { className?: string }) {
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
      aria-label="Azure Blob"
    >
      <g className="text-cyan-600 dark:text-cyan-400">
        {/* Top cylinder */}
        <ellipse cx="12" cy="6" rx="6" ry="2" stroke="currentColor" />
        <path d="M6 6 L6 10 a6 2 0 0 0 12 0 L18 6" stroke="currentColor" />
        {/* Bottom cylinder */}
        <path d="M6 12 L6 16 a6 2 0 0 0 12 0 L18 12" stroke="currentColor" />
        <ellipse cx="12" cy="12" rx="6" ry="2" stroke="currentColor" />
      </g>
    </svg>
  );
}

/** OneDrive — overlapping cloud shapes evoking Microsoft's cloud
 *  family iconography, in Microsoft indigo. */
function OneDriveCloudMark({ className }: { className?: string }) {
  return (
    <svg
      className={className}
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      aria-label="OneDrive"
    >
      <g className="text-indigo-600 dark:text-indigo-400">
        {/* Smaller back cloud */}
        <path
          d="M8 11 a2.5 2.5 0 0 1 0.5 -4.95 a3 3 0 0 1 5.5 -0.5 a2 2 0 0 1 0.5 4 Z"
          fill="currentColor"
          opacity="0.4"
        />
        {/* Front big cloud */}
        <path
          d="M5 18 a3.5 3.5 0 0 1 0.5 -6.95 a4.5 4.5 0 0 1 8.5 -1.55 a3.5 3.5 0 0 1 0.5 6.95 Z"
          fill="currentColor"
        />
      </g>
    </svg>
  );
}
