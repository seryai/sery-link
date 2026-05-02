// Brand-recognisable icons for each watched-source type.
//
// lucide-react doesn't ship logos for third-party services
// (Google's brand guidelines disallow heavy modification anyway),
// so the cloud-source icons are inline simplified SVGs styled in
// official-ish brand colors. The local + URL paths fall back to
// the lucide icons we already use elsewhere.

import { Folder as FolderIcon, Globe } from 'lucide-react';
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
