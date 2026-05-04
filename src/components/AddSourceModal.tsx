// F42 Day 8 — Add Source modal (Stages A + B inline).
//
// One unified entry point for adding any kind of source. Stage A is
// the tile grid showing every protocol Sery Link can register —
// the active four (Local, HTTPS, S3, Google Drive) plus seven
// "Coming soon" tiles for the F43-F49 roadmap (SFTP, WebDAV, B2,
// Azure, GCS, Dropbox, OneDrive). Picking an active tile transitions
// to Stage B, the kind-specific form, INLINE in the same modal —
// no jolt-handoff to a second modal.
//
//   - Local: opens the OS folder picker, then add_watched_folder
//     (no Stage B form needed; the OS dialog IS the form).
//   - HTTPS: URL input + Add.
//   - S3: URL + AWS creds (4 fields) + Add. Pre-flight runs server-
//     side via add_remote_source which calls test_s3_credentials_blocking
//     before persisting (existing behavior).
//   - Drive: embeds GdriveBrowserPanel directly.
//
// The "Coming soon" tiles are visually disabled with a tooltip;
// clicking is a no-op. They're load-bearing for the v0.7.0
// marketing-page promise: the user sees that 11 sources are real
// and on the roadmap, even if only 4 are wireable today.
//
// Spec ref: SPEC_F42_SOURCES_SIDEBAR.md §3.2

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import { ArrowLeft, KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import { SourceIcon } from './SourceIcon';
import { GdriveBrowserPanel } from './GdriveBrowserPanel';
import { isS3Url } from '../utils/url';

interface AddSourceModalProps {
  open: boolean;
  onClose: () => void;
  /** Fires after a source has been registered. The parent should
   *  reload its config to pick up the new source. */
  onAdded: () => void;
}

interface S3Credentials {
  access_key_id: string;
  secret_access_key: string;
  region: string;
  session_token?: string;
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
  { kind: 'local', label: 'Local folder', description: 'Anywhere on this Mac' },
  { kind: 'https', label: 'HTTPS URL', description: 'Public Parquet / CSV / Excel' },
  { kind: 's3', label: 'Amazon S3', description: 'Bucket or prefix with creds' },
  { kind: 'gdrive', label: 'Google Drive', description: 'Folder via OAuth' },
];

const COMING_SOON: ProtocolTile[] = [
  { kind: 'sftp', label: 'SFTP', description: 'Coming in v0.7+' },
  { kind: 'webdav', label: 'WebDAV', description: 'Coming in v0.7+' },
  { kind: 'b2', label: 'Backblaze B2', description: 'Coming in v0.7+' },
  { kind: 'azure', label: 'Azure Blob', description: 'Coming in v0.7+' },
  { kind: 'gcs', label: 'Google Cloud Storage', description: 'Coming in v0.7+' },
  { kind: 'dropbox', label: 'Dropbox', description: 'Coming in v0.7+' },
  { kind: 'onedrive', label: 'OneDrive', description: 'Coming in v0.7+' },
];

type Stage =
  | { kind: 'picker' }
  | { kind: 'url' }       // HTTPS or S3 (form auto-detects)
  | { kind: 'gdrive' };

export function AddSourceModal({ open, onClose, onAdded }: AddSourceModalProps) {
  const toast = useToast();
  const [stage, setStage] = useState<Stage>({ kind: 'picker' });
  const [busy, setBusy] = useState(false);

  if (!open) return null;

  // Reset to picker stage on close so the next open starts fresh.
  const closeAll = () => {
    setStage({ kind: 'picker' });
    onClose();
  };

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
        // user cancelled the OS dialog; keep the modal open at picker
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
      closeAll();
    } catch (err) {
      toast.error(`Couldn't add folder: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onClick={closeAll}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-2xl rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900"
      >
        <ModalHeader
          stage={stage}
          onBack={() => setStage({ kind: 'picker' })}
          onClose={closeAll}
        />
        <div className="p-5">
          {stage.kind === 'picker' && (
            <PickerStage
              busy={busy}
              onPickLocal={onPickLocal}
              onPickUrl={() => setStage({ kind: 'url' })}
              onPickGdrive={() => setStage({ kind: 'gdrive' })}
            />
          )}
          {stage.kind === 'url' && (
            <UrlStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
          )}
          {stage.kind === 'gdrive' && (
            <GdriveBrowserPanel onClose={closeAll} />
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Header — title flips per stage; back button when in Stage B ──

function ModalHeader({
  stage,
  onBack,
  onClose,
}: {
  stage: Stage;
  onBack: () => void;
  onClose: () => void;
}) {
  const title =
    stage.kind === 'picker'
      ? 'Add a source'
      : stage.kind === 'url'
        ? 'Add an HTTPS or S3 source'
        : 'Connect Google Drive';
  const subtitle =
    stage.kind === 'picker'
      ? "Bookmark any place where your data lives. We never copy or upload anything you haven't asked us to."
      : stage.kind === 'url'
        ? 'Public URLs and S3 buckets read schema locally. Credentials live in your OS keychain — never on Sery servers.'
        : 'Sign in once via Google OAuth. Drive files are cached locally; nothing is uploaded.';
  return (
    <div className="flex items-center justify-between border-b border-slate-200 px-5 py-4 dark:border-slate-800">
      <div className="flex items-start gap-3">
        {stage.kind !== 'picker' && (
          <button
            onClick={onBack}
            className="mt-0.5 flex h-7 w-7 items-center justify-center rounded text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
            aria-label="Back to protocol picker"
          >
            <ArrowLeft className="h-4 w-4" />
          </button>
        )}
        <div>
          <h2 className="text-lg font-semibold text-slate-800 dark:text-slate-100">
            {title}
          </h2>
          <p className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
            {subtitle}
          </p>
        </div>
      </div>
      <button
        onClick={onClose}
        className="rounded p-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
        aria-label="Close"
      >
        <X className="h-5 w-5" />
      </button>
    </div>
  );
}

// ─── Stage A — protocol picker ─────────────────────────────────────

function PickerStage({
  busy,
  onPickLocal,
  onPickUrl,
  onPickGdrive,
}: {
  busy: boolean;
  onPickLocal: () => void;
  onPickUrl: () => void;
  onPickGdrive: () => void;
}) {
  return (
    <>
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
                  onPickUrl();
                  break;
                case 'gdrive':
                  onPickGdrive();
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
    </>
  );
}

// ─── Stage B — URL / S3 inline form ────────────────────────────────

function UrlStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [url, setUrl] = useState('');
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [region, setRegion] = useState('us-east-1');
  const [sessionToken, setSessionToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const trimmedUrl = url.trim();
  const isS3 = isS3Url(trimmedUrl);
  const insecure = trimmedUrl.toLowerCase().startsWith('http://');

  const canSubmit =
    !busy &&
    trimmedUrl !== '' &&
    (!isS3 ||
      (accessKey.trim() !== '' &&
        secretKey.trim() !== '' &&
        region.trim() !== ''));

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const args: { url: string; credentials?: S3Credentials } = { url };
      if (isS3) {
        args.credentials = {
          access_key_id: accessKey.trim(),
          secret_access_key: secretKey.trim(),
          region: region.trim(),
          session_token: sessionToken.trim() || undefined,
        };
      }
      await invoke<string>('add_remote_source', args);
      toast.success(isS3 ? 'S3 source added' : 'Remote source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
        URL
      </label>
      <input
        autoFocus
        type="text"
        value={url}
        onChange={(e) => setUrl(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && canSubmit) submit();
        }}
        placeholder="https://… (CSV / Parquet / Excel) or s3://bucket/prefix/"
        className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
      />
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        Public Google Sheets URLs auto-convert to CSV (first tab unless
        the link includes <span className="font-mono">#gid=N</span>). For
        private sheets, use the Google Drive tile instead.
      </p>

      {insecure && trimmedUrl !== '' && (
        <p className="mt-2 text-xs text-amber-700 dark:text-amber-300">
          This URL uses <span className="font-mono">http://</span> — the
          connection isn't encrypted. OK for local / internal endpoints,
          but prefer <span className="font-mono">https://</span> for
          public data.
        </p>
      )}

      {isS3 && (
        <div className="mt-4 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
          <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
            <KeyRound className="h-3.5 w-3.5" />
            AWS credentials
          </div>
          <p className="mb-3 text-xs text-purple-800/80 dark:text-purple-200/80">
            Keys are stored in your OS's credential store and used only
            to read this bucket. Pre-flight tested before save — bad
            keys surface here, not later as a silent empty scan.
          </p>
          <div className="grid gap-2 sm:grid-cols-2">
            <CredField
              label="Access key ID"
              value={accessKey}
              onChange={setAccessKey}
              placeholder="AKIA…"
            />
            <CredField
              label="Secret access key"
              value={secretKey}
              onChange={setSecretKey}
              placeholder="••••••••"
              type="password"
            />
            <CredField
              label="Region"
              value={region}
              onChange={setRegion}
              placeholder="us-east-1"
            />
            <CredField
              label="Session token (optional)"
              value={sessionToken}
              onChange={setSessionToken}
              placeholder="Only for temporary STS creds"
              type="password"
            />
          </div>
        </div>
      )}

      {error && (
        <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      <div className="mt-6 flex items-center justify-end gap-2">
        <button
          onClick={onCancel}
          disabled={busy}
          className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
        >
          Cancel
        </button>
        <button
          onClick={submit}
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          Add source
        </button>
      </div>
    </>
  );
}

function CredField({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: 'text' | 'password';
}) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {label}
      </span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
      />
    </label>
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
      return null;
  }
}

function PlaceholderIcon() {
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
