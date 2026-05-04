// F42 Day 8 — Add Source modal (Stage A: protocol picker).
//
// One unified entry point for adding any kind of source. Stage A is
// the tile grid showing every protocol Sery Link can register —
// the active four (Local, HTTPS, S3, Google Drive) plus seven
// "Coming soon" tiles for the F43-F49 roadmap (SFTP, WebDAV, B2,
// Azure, GCS, Dropbox, OneDrive). Clicking an active tile routes
// to the right form:
//
//   - Local: opens the OS folder picker, then add_watched_folder.
//   - HTTPS / S3: opens AddRemoteSourceModal (existing url tab).
//   - Drive: opens AddRemoteSourceModal at the gdrive tab.
//
// The "Coming soon" tiles are visually disabled with a tooltip;
// clicking is a no-op. They're load-bearing for the v0.7.0
// marketing-page promise: the user sees that 11 sources are real
// and on the roadmap, even if only 4 are wireable today.
//
// Stage B (kind-specific credential forms inline rather than via
// the existing AddRemoteSourceModal handoff) is a follow-on slice.
// For v0.7.0 the handoff to the existing modal is acceptable —
// the UX win here is the picker itself + the discoverability of
// the full protocol set.
//
// Spec ref: SPEC_F42_SOURCES_SIDEBAR.md §3.2

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import { X } from 'lucide-react';
import { useToast } from './Toast';
import { SourceIcon } from './SourceIcon';
import { AddRemoteSourceModal } from './AddRemoteSourceModal';

interface AddSourceModalProps {
  open: boolean;
  onClose: () => void;
  /** Fires after a source has been registered (either via the inline
   *  Local picker or via the AddRemoteSourceModal handoff). The
   *  parent should reload its config to pick up the new source. */
  onAdded: () => void;
}

type ImplementedKind = 'local' | 'https' | 's3' | 'gdrive';
type ComingSoonKind =
  | 'sftp'
  | 'webdav'
  | 'b2'
  | 'azure'
  | 'gcs'
  | 'dropbox'
  | 'onedrive';

interface ProtocolTile {
  kind: ImplementedKind | ComingSoonKind;
  label: string;
  description: string;
}

const IMPLEMENTED: ProtocolTile[] = [
  {
    kind: 'local',
    label: 'Local folder',
    description: 'Anywhere on this Mac',
  },
  {
    kind: 'https',
    label: 'HTTPS URL',
    description: 'Public Parquet / CSV / Excel',
  },
  {
    kind: 's3',
    label: 'Amazon S3',
    description: 'Bucket or prefix with creds',
  },
  {
    kind: 'gdrive',
    label: 'Google Drive',
    description: 'Folder via OAuth',
  },
];

const COMING_SOON: ProtocolTile[] = [
  {
    kind: 'sftp',
    label: 'SFTP',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'webdav',
    label: 'WebDAV',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'b2',
    label: 'Backblaze B2',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'azure',
    label: 'Azure Blob',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'gcs',
    label: 'Google Cloud Storage',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'dropbox',
    label: 'Dropbox',
    description: 'Coming in v0.7+',
  },
  {
    kind: 'onedrive',
    label: 'OneDrive',
    description: 'Coming in v0.7+',
  },
];

export function AddSourceModal({ open, onClose, onAdded }: AddSourceModalProps) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  // When the user picks HTTPS/S3/Drive we hand off to the existing
  // AddRemoteSourceModal. We track which tab to open it at so the
  // user lands on the right form.
  const [remoteTab, setRemoteTab] = useState<'url' | 'gdrive' | null>(null);

  if (!open) return null;

  const onPickLocal = async () => {
    setBusy(true);
    try {
      let defaultPath: string | undefined;
      try {
        defaultPath = await documentDir();
      } catch {
        defaultPath = undefined;
      }
      const selected = await openDialog({
        directory: true,
        multiple: false,
        defaultPath,
      });
      if (typeof selected !== 'string') {
        // user cancelled; close the busy state but keep the modal open
        setBusy(false);
        return;
      }
      await invoke('add_watched_folder', { path: selected, recursive: true });
      toast.success('Folder added');
      // Background scan — same pattern as FolderList.
      invoke('rescan_folder', { folderPath: selected }).catch((err) => {
        console.error('Initial scan failed:', err);
      });
      onAdded();
      onClose();
    } catch (err) {
      toast.error(`Couldn't add folder: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const onPickRemote = (initialTab: 'url' | 'gdrive') => {
    setRemoteTab(initialTab);
  };

  // The protocol picker is rendered only when we're not currently
  // delegating to AddRemoteSourceModal — otherwise the picker stays
  // mounted underneath which leaks click handlers to the wrong layer.
  const showPicker = remoteTab === null;

  return (
    <>
      {showPicker && (
        <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4">
          <div className="w-full max-w-2xl rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900">
            <div className="flex items-center justify-between border-b border-slate-200 px-5 py-4 dark:border-slate-800">
              <div>
                <h2 className="text-lg font-semibold text-slate-800 dark:text-slate-100">
                  Add a source
                </h2>
                <p className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
                  Bookmark any place where your data lives. We never copy or
                  upload anything you haven't asked us to.
                </p>
              </div>
              <button
                onClick={onClose}
                className="rounded p-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
                aria-label="Close"
              >
                <X className="h-5 w-5" />
              </button>
            </div>
            <div className="p-5">
              <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                {IMPLEMENTED.map((tile) => (
                  <ProtocolCard
                    key={tile.kind}
                    tile={tile}
                    disabled={busy}
                    onClick={() => {
                      switch (tile.kind) {
                        case 'local':
                          onPickLocal();
                          break;
                        case 'https':
                        case 's3':
                          onPickRemote('url');
                          break;
                        case 'gdrive':
                          onPickRemote('gdrive');
                          break;
                      }
                    }}
                  />
                ))}
              </div>
              <div className="mt-6">
                <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                  Coming in v0.7+
                </h3>
                <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                  {COMING_SOON.map((tile) => (
                    <ProtocolCard key={tile.kind} tile={tile} disabled />
                  ))}
                </div>
              </div>
            </div>
          </div>
        </div>
      )}
      {/* Delegate handoff for HTTPS / S3 / Drive. Pre-positioned at
          the right tab via initialTab. */}
      <AddRemoteSourceModal
        open={remoteTab !== null}
        initialTab={remoteTab ?? 'url'}
        onClose={() => setRemoteTab(null)}
        onAdded={() => {
          setRemoteTab(null);
          onAdded();
          onClose();
        }}
      />
    </>
  );
}

// ─── Helper: protocol tile ─────────────────────────────────────────

function ProtocolCard({
  tile,
  disabled,
  onClick,
}: {
  tile: ProtocolTile;
  disabled?: boolean;
  onClick?: () => void;
}) {
  // The tile maps the new structured kinds + the "coming soon" set
  // onto the legacy SourceIcon enum where available, falling back to
  // a generic globe for protocols SourceIcon doesn't know about.
  const iconKind = legacyIconKindForTile(tile.kind);
  const isComingSoon = onClick === undefined;
  return (
    <button
      type="button"
      disabled={disabled || isComingSoon}
      onClick={onClick}
      className={`flex flex-col items-center gap-2 rounded-lg border p-4 text-center transition-all ${
        isComingSoon
          ? 'cursor-not-allowed border-dashed border-slate-200 bg-slate-50 text-slate-400 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-500'
          : disabled
            ? 'cursor-wait border-slate-200 bg-slate-50 opacity-60 dark:border-slate-700 dark:bg-slate-800/40'
            : 'cursor-pointer border-slate-200 bg-white text-slate-700 hover:border-purple-400 hover:bg-purple-50 hover:text-slate-900 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300 dark:hover:border-purple-500 dark:hover:bg-purple-900/20 dark:hover:text-slate-100'
      }`}
      title={isComingSoon ? `${tile.label} — coming in v0.7+` : tile.label}
    >
      <div
        className={`flex h-10 w-10 items-center justify-center rounded-md ${
          isComingSoon
            ? 'bg-slate-100 dark:bg-slate-800'
            : 'bg-slate-100 dark:bg-slate-700'
        }`}
      >
        {iconKind ? (
          <SourceIcon kind={iconKind} size="md" />
        ) : (
          <PlaceholderIcon />
        )}
      </div>
      <div className="text-sm font-medium">{tile.label}</div>
      <div className="text-xs leading-tight text-slate-500 dark:text-slate-400">
        {tile.description}
      </div>
    </button>
  );
}

function legacyIconKindForTile(
  kind: ImplementedKind | ComingSoonKind,
): 'local' | 'http' | 's3' | 'gdrive' | null {
  switch (kind) {
    case 'local':
      return 'local';
    case 'https':
      return 'http';
    case 's3':
      return 's3';
    case 'gdrive':
      return 'gdrive';
    default:
      return null; // coming-soon protocols use the placeholder
  }
}

function PlaceholderIcon() {
  // Generic dotted-square for the "coming soon" tiles — visually
  // distinct from a real protocol icon so it reads as "future" not
  // "broken."
  return (
    <svg
      className="h-5 w-5 text-slate-400 dark:text-slate-500"
      viewBox="0 0 20 20"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <rect
        x="3"
        y="3"
        width="14"
        height="14"
        rx="2"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeDasharray="2 2"
      />
      <circle cx="10" cy="10" r="1.5" fill="currentColor" />
    </svg>
  );
}
