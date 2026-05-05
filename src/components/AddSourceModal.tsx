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

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import { ArrowLeft, KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import { SourceIcon } from './SourceIcon';
import { GdriveBrowserPanel } from './GdriveBrowserPanel';
import { isS3Url } from '../utils/url';
import type { SftpAuth, WebDavAuth } from '../utils/sources';

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
  endpoint_url?: string;
  url_style?: 'path' | 'vhost';
}

type ImplementedKind =
  | 'local'
  | 'https'
  | 's3'
  | 'gdrive'
  | 'sftp'
  | 'webdav'
  | 'dropbox'
  | 'azure'
  | 'onedrive';
type S3CompatibleKind = 'b2' | 'wasabi' | 'r2' | 'gcs';
// All "coming soon" tiles have shipped — leaving this as a never
// type would prevent the COMING_SOON array from being typed; use
// `never` to express "no tiles in this tier today" while keeping
// the type machinery intact for future additions.
type ComingSoonKind = never;

interface ProtocolTile {
  kind: ImplementedKind | S3CompatibleKind | ComingSoonKind;
  label: string;
  description: string;
}

const IMPLEMENTED: ProtocolTile[] = [
  { kind: 'local', label: 'Local folder', description: 'Anywhere on this Mac' },
  { kind: 'https', label: 'HTTPS URL', description: 'Public Parquet / CSV / Excel' },
  { kind: 's3', label: 'Amazon S3', description: 'Bucket or prefix with creds' },
  { kind: 'gdrive', label: 'Google Drive', description: 'Folder via OAuth' },
  { kind: 'sftp', label: 'SFTP', description: 'Password or SSH key' },
  { kind: 'webdav', label: 'WebDAV', description: 'Nextcloud / ownCloud / generic' },
  { kind: 'dropbox', label: 'Dropbox', description: 'Sign in or access token' },
  { kind: 'azure', label: 'Azure Blob', description: 'SAS token' },
  { kind: 'onedrive', label: 'OneDrive', description: 'Microsoft device code' },
];

// F45: S3-compatible providers route to the URL stage with the
// endpoint + url_style + region pre-filled. DuckDB httpfs talks to
// these the same way as AWS S3 once `s3_endpoint` is set.
const S3_COMPATIBLE: ProtocolTile[] = [
  { kind: 'b2', label: 'Backblaze B2', description: 'S3-compatible' },
  { kind: 'wasabi', label: 'Wasabi', description: 'S3-compatible' },
  { kind: 'r2', label: 'Cloudflare R2', description: 'S3-compatible' },
  { kind: 'gcs', label: 'Google Cloud Storage', description: 'S3 interop' },
];

// All "coming soon" tiles have shipped to Implemented. Section
// stays so future protocol additions slot in cleanly.
const COMING_SOON: ProtocolTile[] = [];

/** Per-S3-compatible-provider presets for the UrlStage form. The
 *  endpoint is the host DuckDB needs (no scheme); the placeholder
 *  shows the typical bucket URL format the user will paste; the
 *  region default is the provider's "you probably mean this" pick.
 *  Users can override any of these in the form before submit. */
const PRESETS: Record<S3CompatibleKind, UrlStageInitial> = {
  b2: {
    endpointUrl: 's3.us-west-002.backblazeb2.com',
    urlStyle: 'path',
    region: 'us-west-002',
    urlPlaceholder: 's3://your-bucket/prefix/',
    providerLabel: 'Backblaze B2',
  },
  wasabi: {
    endpointUrl: 's3.wasabisys.com',
    urlStyle: 'vhost',
    region: 'us-east-1',
    urlPlaceholder: 's3://your-bucket/prefix/',
    providerLabel: 'Wasabi',
  },
  r2: {
    // R2's endpoint is per-account; users must replace the placeholder.
    // Empty string means "no preset" so the field appears empty rather
    // than showing a misleading example URL.
    endpointUrl: '',
    urlStyle: 'path',
    region: 'auto',
    urlPlaceholder: 's3://your-bucket/prefix/',
    providerLabel: 'Cloudflare R2',
  },
  gcs: {
    endpointUrl: 'storage.googleapis.com',
    urlStyle: 'path',
    region: 'auto',
    urlPlaceholder: 's3://your-bucket/prefix/',
    providerLabel: 'Google Cloud Storage',
  },
};

type Stage =
  | { kind: 'picker' }
  // 'url' covers HTTPS, S3, and S3-compatible providers (the form
  // auto-detects S3 by URL scheme; presets fill endpoint/region).
  | { kind: 'url'; initial?: UrlStageInitial }
  | { kind: 'gdrive' }
  | { kind: 'sftp' }
  | { kind: 'webdav' }
  | { kind: 'dropbox' }
  | { kind: 'azure' }
  | { kind: 'onedrive' };

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
      // F42 Day 4 slice 2: use the kind-specific add command which
      // writes directly to `sources` Vec (with mirror watched_folders
      // entry for the legacy scanner path). Eliminates the migration-
      // on-next-load round trip — the new source is in `sources`
      // immediately, so the Sources sidebar reflects it as soon as
      // reloadConfig fires.
      await invoke<string>('add_local_source', {
        path: selected,
        recursive: true,
      });
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
              onPickSftp={() => setStage({ kind: 'sftp' })}
              onPickWebDav={() => setStage({ kind: 'webdav' })}
              onPickDropbox={() => setStage({ kind: 'dropbox' })}
              onPickAzure={() => setStage({ kind: 'azure' })}
              onPickOneDrive={() => setStage({ kind: 'onedrive' })}
              onPickS3Compatible={(preset) =>
                setStage({ kind: 'url', initial: PRESETS[preset] })
              }
            />
          )}
          {stage.kind === 'url' && (
            <UrlStage
              initial={stage.initial}
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
          {stage.kind === 'sftp' && (
            <SftpStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
          )}
          {stage.kind === 'webdav' && (
            <WebDavStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
          )}
          {stage.kind === 'dropbox' && (
            <DropboxStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
          )}
          {stage.kind === 'azure' && (
            <AzureBlobStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
          )}
          {stage.kind === 'onedrive' && (
            <OneDriveStage
              onAdded={() => {
                onAdded();
                closeAll();
              }}
              onCancel={() => setStage({ kind: 'picker' })}
            />
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
  const providerLabel =
    stage.kind === 'url' ? stage.initial?.providerLabel : undefined;
  const title =
    stage.kind === 'picker'
      ? 'Add a source'
      : stage.kind === 'url'
        ? providerLabel
          ? `Add a ${providerLabel} source`
          : 'Add an HTTPS or S3 source'
        : stage.kind === 'sftp'
          ? 'Add an SFTP source'
          : stage.kind === 'webdav'
            ? 'Add a WebDAV source'
            : stage.kind === 'dropbox'
              ? 'Add a Dropbox source'
              : stage.kind === 'azure'
                ? 'Add an Azure Blob source'
                : stage.kind === 'onedrive'
                  ? 'Add a OneDrive source'
                  : 'Connect Google Drive';
  const subtitle =
    stage.kind === 'picker'
      ? "Bookmark any place where your data lives. We never copy or upload anything you haven't asked us to."
      : stage.kind === 'url'
        ? providerLabel
          ? `${providerLabel} speaks the S3 protocol — Sery talks to it via DuckDB's S3 client. Credentials live in your OS keychain.`
          : 'Public URLs and S3 buckets read schema locally. Credentials live in your OS keychain — never on Sery servers.'
        : stage.kind === 'sftp'
          ? "Files are pulled to a local cache via SSH; the connection's auth credentials live in your OS keychain."
          : stage.kind === 'webdav'
            ? 'Works with Nextcloud, ownCloud, and any generic WebDAV server. Files cache locally; auth lives in your OS keychain.'
            : stage.kind === 'dropbox'
              ? 'Connect-with-Dropbox via OAuth, or paste a Personal Access Token from dropbox.com/developers/apps.'
              : stage.kind === 'azure'
                ? 'Generate a SAS token in the Azure portal scoped to your container with read-only permissions. Stored in your OS keychain.'
                : stage.kind === 'onedrive'
                  ? 'Sign in via the device code flow — Sery shows a code, you enter it on microsoft.com/devicelogin. Tokens stored in your OS keychain.'
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
  onPickSftp,
  onPickWebDav,
  onPickDropbox,
  onPickAzure,
  onPickOneDrive,
  onPickS3Compatible,
}: {
  busy: boolean;
  onPickLocal: () => void;
  onPickUrl: () => void;
  onPickGdrive: () => void;
  onPickSftp: () => void;
  onPickWebDav: () => void;
  onPickDropbox: () => void;
  onPickAzure: () => void;
  onPickOneDrive: () => void;
  onPickS3Compatible: (kind: S3CompatibleKind) => void;
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
                case 'sftp':
                  onPickSftp();
                  break;
                case 'webdav':
                  onPickWebDav();
                  break;
                case 'dropbox':
                  onPickDropbox();
                  break;
                case 'azure':
                  onPickAzure();
                  break;
                case 'onedrive':
                  onPickOneDrive();
                  break;
              }
            }}
          />
        ))}
      </div>
      {/* F45: S3-compatible providers — DuckDB httpfs talks to all of
          these the same way once s3_endpoint is set. The presets
          fill in the right host + url_style + region so the user
          only types their bucket URL + creds. */}
      <div className="mt-6">
        <h3 className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
          S3-compatible
        </h3>
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {S3_COMPATIBLE.map((tile) => (
            <ProtocolCard
              key={tile.kind}
              tile={tile}
              disabled={busy}
              onClick={() =>
                onPickS3Compatible(tile.kind as S3CompatibleKind)
              }
            />
          ))}
        </div>
      </div>
      {COMING_SOON.length > 0 && (
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
      )}
    </>
  );
}

// ─── Stage B — URL / S3 inline form ────────────────────────────────

/** Initial form state for the URL stage. Per-preset variants pre-fill
 *  the endpoint so users picking "Backblaze B2" land on a form that's
 *  ready to take their bucket URL + creds without first looking up
 *  the endpoint host on the provider's docs page. */
interface UrlStageInitial {
  endpointUrl?: string;
  urlStyle?: 'path' | 'vhost';
  region?: string;
  /** Hint text rendered below the URL input — provider-specific
   *  reminder of the bucket-URL format. */
  urlPlaceholder?: string;
  /** Branded label for the form heading + button — "Add a Backblaze
   *  B2 source" reads more confidently than "Add an HTTPS or S3
   *  source" when the user explicitly picked B2 from the picker. */
  providerLabel?: string;
}

function UrlStage({
  onAdded,
  onCancel,
  initial,
}: {
  onAdded: () => void;
  onCancel: () => void;
  initial?: UrlStageInitial;
}) {
  const toast = useToast();
  const [url, setUrl] = useState('');
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [region, setRegion] = useState(initial?.region ?? 'us-east-1');
  const [sessionToken, setSessionToken] = useState('');
  const [endpointUrl, setEndpointUrl] = useState(initial?.endpointUrl ?? '');
  const [urlStyle, setUrlStyle] = useState<'path' | 'vhost'>(
    initial?.urlStyle ?? 'vhost',
  );
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
          endpoint_url: endpointUrl.trim() || undefined,
          url_style: endpointUrl.trim() ? urlStyle : undefined,
        };
      }
      const normalised = await invoke<string>('add_remote_source', args);
      toast.success(isS3 ? 'S3 source added' : 'Remote source added');
      // Match the Local add path: auto-kick the initial scan in the
      // background so the user sees row counts populate without
      // needing to right-click → Rescan. Failures here are non-fatal
      // — the user can manually rescan from the sidebar context menu.
      invoke('rescan_folder', { folderPath: normalised }).catch((err) => {
        console.error('Initial scan failed:', err);
      });
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
        placeholder={
          initial?.urlPlaceholder ??
          'https://… (CSV / Parquet / Excel) or s3://bucket/prefix/'
        }
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
          <div className="mt-3 border-t border-purple-200/70 pt-3 dark:border-purple-900/50">
            <details
              open={Boolean(initial?.endpointUrl) || endpointUrl !== ''}
              className="group"
            >
              <summary className="cursor-pointer text-[11px] font-medium uppercase tracking-wide text-purple-700 hover:text-purple-900 dark:text-purple-200 dark:hover:text-purple-50">
                S3-compatible endpoint (B2 / Wasabi / R2 / GCS / MinIO)
              </summary>
              <div className="mt-2 grid gap-2 sm:grid-cols-2">
                <CredField
                  label="Endpoint URL"
                  value={endpointUrl}
                  onChange={setEndpointUrl}
                  placeholder="leave blank for AWS S3"
                />
                <label className="block">
                  <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                    URL style
                  </span>
                  <select
                    value={urlStyle}
                    onChange={(e) =>
                      setUrlStyle(e.target.value as 'path' | 'vhost')
                    }
                    className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100"
                  >
                    <option value="vhost">vhost (AWS, Wasabi)</option>
                    <option value="path">path (B2, R2, MinIO)</option>
                  </select>
                </label>
              </div>
              <p className="mt-2 text-xs text-purple-800/80 dark:text-purple-200/80">
                Paste the host from your provider's docs (with or without
                <span className="font-mono"> https:// </span>—either works).
                Most providers also tell you which URL style to use.
              </p>
            </details>
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
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: 'text' | 'password';
  autoFocus?: boolean;
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
        autoFocus={autoFocus}
        className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
      />
    </label>
  );
}

// ─── Stage B — SFTP form ───────────────────────────────────────────

function SftpStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [host, setHost] = useState('');
  const [port, setPort] = useState('22');
  const [username, setUsername] = useState('');
  const [basePath, setBasePath] = useState('/');
  const [authMode, setAuthMode] = useState<'password' | 'private_key'>(
    'password',
  );
  const [password, setPassword] = useState('');
  const [keyPath, setKeyPath] = useState('~/.ssh/id_ed25519');
  const [passphrase, setPassphrase] = useState('');
  const [busy, setBusy] = useState(false);
  const [testStatus, setTestStatus] = useState<'idle' | 'ok' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const buildAuth = (): SftpAuth | null => {
    if (authMode === 'password') {
      if (!password) return null;
      return { type: 'password', password };
    }
    if (!keyPath.trim()) return null;
    return {
      type: 'private_key',
      private_key_path: keyPath.trim(),
      passphrase: passphrase || undefined,
    };
  };

  const portNumber = parseInt(port, 10);
  const portValid = !isNaN(portNumber) && portNumber > 0 && portNumber <= 65535;
  const auth = buildAuth();
  const canSubmit =
    !busy &&
    host.trim() !== '' &&
    username.trim() !== '' &&
    basePath.trim() !== '' &&
    portValid &&
    auth !== null;

  const test = async () => {
    if (!auth) return;
    setError(null);
    setTestStatus(null);
    setBusy(true);
    try {
      await invoke<void>('test_sftp_credentials', {
        host: host.trim(),
        port: portNumber,
        username: username.trim(),
        auth,
      });
      setTestStatus('ok');
      toast.success('Connection OK');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (!auth) return;
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_sftp_source', {
        host: host.trim(),
        port: portNumber,
        username: username.trim(),
        basePath: basePath.trim(),
        auth,
      });
      toast.success('SFTP source added');
      // Note: scanner integration for SFTP (download-on-rescan) is
      // deferred. The new source appears in the sidebar but its
      // first scan is a no-op until that slice ships.
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <div className="grid gap-3 sm:grid-cols-3">
        <div className="sm:col-span-2">
          <CredField
            label="Host"
            value={host}
            onChange={(v) => {
              setHost(v);
              setTestStatus(null);
            }}
            placeholder="fileserver.example.com"
          />
        </div>
        <CredField
          label="Port"
          value={port}
          onChange={(v) => {
            setPort(v);
            setTestStatus(null);
          }}
          placeholder="22"
        />
      </div>
      <div className="mt-3 grid gap-3 sm:grid-cols-2">
        <CredField
          label="Username"
          value={username}
          onChange={(v) => {
            setUsername(v);
            setTestStatus(null);
          }}
          placeholder="alice"
        />
        <CredField
          label="Base path"
          value={basePath}
          onChange={setBasePath}
          placeholder="/home/alice/data"
        />
      </div>

      <div className="mt-4 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
          <KeyRound className="h-3.5 w-3.5" />
          Authentication
        </div>
        <div className="mb-3 flex gap-2 text-xs">
          <button
            type="button"
            onClick={() => {
              setAuthMode('password');
              setTestStatus(null);
            }}
            className={`rounded-md border px-3 py-1 transition-colors ${
              authMode === 'password'
                ? 'border-purple-500 bg-purple-600 text-white'
                : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
            }`}
          >
            Password
          </button>
          <button
            type="button"
            onClick={() => {
              setAuthMode('private_key');
              setTestStatus(null);
            }}
            className={`rounded-md border px-3 py-1 transition-colors ${
              authMode === 'private_key'
                ? 'border-purple-500 bg-purple-600 text-white'
                : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
            }`}
          >
            SSH key
          </button>
        </div>
        {authMode === 'password' ? (
          <CredField
            label="Password"
            value={password}
            onChange={(v) => {
              setPassword(v);
              setTestStatus(null);
            }}
            type="password"
            placeholder="••••••••"
          />
        ) : (
          <div className="grid gap-2 sm:grid-cols-2">
            <CredField
              label="Private key path"
              value={keyPath}
              onChange={(v) => {
                setKeyPath(v);
                setTestStatus(null);
              }}
              placeholder="~/.ssh/id_ed25519"
            />
            <CredField
              label="Passphrase (optional)"
              value={passphrase}
              onChange={setPassphrase}
              type="password"
              placeholder="Only if the key is encrypted"
            />
          </div>
        )}
        <p className="mt-2 text-xs text-purple-800/80 dark:text-purple-200/80">
          Stored in your OS keychain. The private key file itself
          stays on disk; Sery only stores its path. Test the
          connection before saving — bad creds surface here, not as
          a silent empty rescan.
        </p>
      </div>

      {error && (
        <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      {testStatus === 'ok' && !error && (
        <div className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-300">
          Connection OK — ready to save.
        </div>
      )}

      <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-300">
        Rescan downloads matching files (CSV / Parquet / Excel /
        docs) to{' '}
        <span className="font-mono">~/.seryai/sftp-cache/&lt;id&gt;/</span>{' '}
        and indexes them locally. Each rescan is a full re-download —
        bandwidth-heavy for large trees. Incremental sync ships in a
        future release.
      </div>

      <div className="mt-6 flex items-center justify-between gap-2">
        <button
          onClick={test}
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
        >
          {busy && testStatus !== 'ok' && (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          )}
          Test connection
        </button>
        <div className="flex items-center gap-2">
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
            Add SFTP source
          </button>
        </div>
      </div>
    </>
  );
}

// ─── Stage B — OneDrive device code flow ───────────────────────────

interface OneDriveCreds {
  access_token: string;
  refresh_token: string;
  expires_at: string;
}

interface DeviceCodeStart {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message: string;
}

type OneDrivePollResult =
  | { status: 'pending' }
  | { status: 'slow_down' }
  | { status: 'completed'; creds: OneDriveCreds };

function OneDriveStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [phase, setPhase] = useState<
    | { kind: 'idle' }
    | { kind: 'starting' }
    | {
        kind: 'polling';
        deviceCode: string;
        userCode: string;
        verificationUri: string;
        intervalMs: number;
      }
    | { kind: 'completed'; creds: OneDriveCreds }
  >({ kind: 'idle' });
  const [basePath, setBasePath] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Poll loop. Mounts/unmounts on phase changes.
  useEffect(() => {
    if (phase.kind !== 'polling') return;
    let cancelled = false;
    let interval = phase.intervalMs;

    const tick = async () => {
      if (cancelled) return;
      try {
        const result = await invoke<OneDrivePollResult>('poll_onedrive_auth', {
          deviceCode: phase.deviceCode,
        });
        if (cancelled) return;
        if (result.status === 'completed') {
          setPhase({ kind: 'completed', creds: result.creds });
          return;
        }
        if (result.status === 'slow_down') {
          interval = Math.min(interval * 2, 30_000);
        }
        setTimeout(tick, interval);
      } catch (err) {
        if (cancelled) return;
        setError(String(err));
        setPhase({ kind: 'idle' });
      }
    };
    setTimeout(tick, interval);
    return () => {
      cancelled = true;
    };
  }, [phase]);

  const startAuth = async () => {
    setError(null);
    setBusy(true);
    setPhase({ kind: 'starting' });
    try {
      const start = await invoke<DeviceCodeStart>('start_onedrive_auth');
      setPhase({
        kind: 'polling',
        deviceCode: start.device_code,
        userCode: start.user_code,
        verificationUri: start.verification_uri,
        intervalMs: Math.max(start.interval, 1) * 1000,
      });
    } catch (err) {
      setError(String(err));
      setPhase({ kind: 'idle' });
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (phase.kind !== 'completed') return;
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_onedrive_source', {
        basePath: basePath.trim(),
        creds: phase.creds,
      });
      toast.success('OneDrive source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {phase.kind === 'idle' && (
        <>
          <p className="text-sm text-slate-700 dark:text-slate-300">
            Sign in once via Microsoft's device code flow:
          </p>
          <ol className="ml-4 list-decimal space-y-1 text-sm text-slate-700 dark:text-slate-300">
            <li>Click "Start sign-in" below.</li>
            <li>
              Sery shows a code like <span className="font-mono">XXXX-XXXX</span>.
            </li>
            <li>
              Open{' '}
              <span className="font-mono">microsoft.com/devicelogin</span>{' '}
              in any browser, paste the code, sign in.
            </li>
            <li>Come back here — auth completes automatically.</li>
          </ol>
          <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
            Tokens (access + refresh) live in your OS keychain.
            Refreshes happen automatically; you only sign in once.
          </p>
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
              onClick={startAuth}
              disabled={busy}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              Start sign-in
            </button>
          </div>
        </>
      )}

      {phase.kind === 'starting' && (
        <div className="flex items-center gap-2 text-sm text-slate-500">
          <Loader2 className="h-4 w-4 animate-spin" />
          Requesting device code from Microsoft…
        </div>
      )}

      {phase.kind === 'polling' && (
        <>
          <div className="rounded-lg border border-purple-200 bg-purple-50 p-4 text-center dark:border-purple-900/60 dark:bg-purple-950/30">
            <p className="text-xs uppercase tracking-wide text-purple-700 dark:text-purple-200">
              Enter this code at{' '}
              <span className="font-mono">{phase.verificationUri}</span>
            </p>
            <p className="mt-2 font-mono text-3xl font-bold tracking-widest text-purple-900 dark:text-purple-100">
              {phase.userCode}
            </p>
          </div>
          <p className="mt-3 flex items-center gap-2 text-sm text-slate-600 dark:text-slate-400">
            <Loader2 className="h-4 w-4 animate-spin" />
            Waiting for sign-in to complete…
          </p>
          <div className="mt-6 flex justify-end">
            <button
              onClick={() => setPhase({ kind: 'idle' })}
              className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
            >
              Cancel sign-in
            </button>
          </div>
        </>
      )}

      {phase.kind === 'completed' && (
        <>
          <div className="rounded-md border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-300">
            Signed in. Pick the OneDrive folder to bookmark and click
            Add.
          </div>
          <div className="mt-3">
            <CredField
              label="Base path"
              value={basePath}
              onChange={setBasePath}
              placeholder="empty for whole OneDrive, or /Documents"
            />
          </div>
          <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-300">
            Rescan downloads matching files (CSV / Parquet / Excel /
            docs) to{' '}
            <span className="font-mono">
              ~/.seryai/onedrive-cache/&lt;id&gt;/
            </span>{' '}
            and indexes them locally. Subsequent rescans skip files
            whose remote size + mtime are unchanged.
          </div>
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
              disabled={busy}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              Add OneDrive source
            </button>
          </div>
        </>
      )}
    </>
  );
}

// ─── Stage B — Azure Blob form ─────────────────────────────────────

function AzureBlobStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [accountUrl, setAccountUrl] = useState('');
  const [prefix, setPrefix] = useState('');
  const [sasToken, setSasToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [testStatus, setTestStatus] = useState<'idle' | 'ok' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    !busy && accountUrl.trim() !== '' && sasToken.trim().length > 16;

  const test = async () => {
    setError(null);
    setTestStatus(null);
    setBusy(true);
    try {
      await invoke<void>('test_azure_blob_credentials', {
        accountUrl: accountUrl.trim(),
        sasToken: sasToken.trim(),
      });
      setTestStatus('ok');
      toast.success('SAS token OK');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_azure_blob_source', {
        accountUrl: accountUrl.trim(),
        prefix: prefix.trim(),
        sasToken: sasToken.trim(),
      });
      toast.success('Azure Blob source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <CredField
        label="Container URL"
        value={accountUrl}
        onChange={(v) => {
          setAccountUrl(v);
          setTestStatus(null);
        }}
        placeholder="https://myaccount.blob.core.windows.net/mycontainer"
      />
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        Format:{' '}
        <span className="font-mono">
          https://&lt;account&gt;.blob.core.windows.net/&lt;container&gt;
        </span>
        . Find this in the Azure portal: Storage account → Containers
        → click your container → URL.
      </p>

      <div className="mt-3">
        <CredField
          label="Path prefix (optional)"
          value={prefix}
          onChange={setPrefix}
          placeholder="data/ (empty = whole container)"
        />
      </div>

      <div className="mt-4 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
          <KeyRound className="h-3.5 w-3.5" />
          SAS token
        </div>
        <CredField
          label="SAS token"
          value={sasToken}
          onChange={(v) => {
            setSasToken(v);
            setTestStatus(null);
          }}
          type="password"
          placeholder="?sv=… or sv=…"
        />
        <p className="mt-2 text-xs text-purple-800/80 dark:text-purple-200/80">
          Generate at: Storage account → Shared access tokens.
          Permissions: <strong>Read + List</strong>. Set the expiry
          long enough to outlive your typical scan rotation. Stored
          in your OS keychain; never sent to Sery servers.
        </p>
      </div>

      {error && (
        <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      {testStatus === 'ok' && !error && (
        <div className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-300">
          SAS token OK — ready to save.
        </div>
      )}

      <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-300">
        Rescan downloads matching files (CSV / Parquet / Excel /
        docs) to{' '}
        <span className="font-mono">
          ~/.seryai/azure-cache/&lt;id&gt;/
        </span>{' '}
        and indexes them locally. Subsequent rescans skip files
        whose remote size + mtime are unchanged (incremental sync).
      </div>

      <div className="mt-6 flex items-center justify-between gap-2">
        <button
          onClick={test}
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
        >
          {busy && testStatus !== 'ok' && (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          )}
          Test SAS token
        </button>
        <div className="flex items-center gap-2">
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
            Add Azure source
          </button>
        </div>
      </div>
    </>
  );
}

// ─── Stage B — Dropbox form ────────────────────────────────────────

interface DropboxAuthStart {
  authorize_url: string;
  code_verifier: string;
}

function DropboxStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  // Two auth paths share this stage. OAuth is the default
  // (Connect-with-Dropbox) — no token-pasting, refreshes itself.
  // PAT is the fallback for users whose Dropbox app deployment
  // hasn't been registered yet, or for self-hosters.
  const [mode, setMode] = useState<'oauth' | 'pat'>('oauth');
  const [basePath, setBasePath] = useState('/');

  return (
    <>
      <div className="mb-3 inline-flex items-center rounded-md border border-slate-200 bg-slate-50 p-0.5 text-xs dark:border-slate-700 dark:bg-slate-800/60">
        <button
          onClick={() => setMode('oauth')}
          className={`rounded px-2.5 py-1 font-medium ${
            mode === 'oauth'
              ? 'bg-white text-slate-900 shadow-sm dark:bg-slate-900 dark:text-slate-100'
              : 'text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-slate-100'
          }`}
        >
          Sign in with Dropbox
        </button>
        <button
          onClick={() => setMode('pat')}
          className={`rounded px-2.5 py-1 font-medium ${
            mode === 'pat'
              ? 'bg-white text-slate-900 shadow-sm dark:bg-slate-900 dark:text-slate-100'
              : 'text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-slate-100'
          }`}
        >
          Access token
        </button>
      </div>

      {mode === 'oauth' ? (
        <DropboxOAuthForm
          basePath={basePath}
          setBasePath={setBasePath}
          onAdded={onAdded}
          onCancel={onCancel}
        />
      ) : (
        <DropboxPatForm
          basePath={basePath}
          setBasePath={setBasePath}
          onAdded={onAdded}
          onCancel={onCancel}
        />
      )}
    </>
  );
}

function DropboxOAuthForm({
  basePath,
  setBasePath,
  onAdded,
  onCancel,
}: {
  basePath: string;
  setBasePath: (v: string) => void;
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  // Phase machine:
  //   idle      — show the "Open Dropbox" button.
  //   awaiting  — Dropbox tab is open; user pastes the code Dropbox
  //               showed them. Verifier is held in state.
  const [phase, setPhase] = useState<
    | { kind: 'idle' }
    | { kind: 'awaiting'; codeVerifier: string; authorizeUrl: string }
  >({ kind: 'idle' });
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startAuth = async () => {
    setError(null);
    setBusy(true);
    try {
      const start = await invoke<DropboxAuthStart>('start_dropbox_oauth');
      try {
        // Best-effort open; if the opener plugin isn't available
        // (or Dropbox blocks the in-app webview), the user can
        // still copy the URL from the field below.
        const opener = await import('@tauri-apps/plugin-opener');
        await opener.openUrl(start.authorize_url);
      } catch {
        // Falls through; the URL is shown in the awaiting phase.
      }
      setPhase({
        kind: 'awaiting',
        codeVerifier: start.code_verifier,
        authorizeUrl: start.authorize_url,
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (phase.kind !== 'awaiting') return;
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_dropbox_source_oauth', {
        basePath: basePath.trim(),
        code: code.trim(),
        codeVerifier: phase.codeVerifier,
      });
      toast.success('Dropbox source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      {phase.kind === 'idle' && (
        <>
          <p className="text-sm text-slate-700 dark:text-slate-300">
            Connect-with-Dropbox via OAuth — no token to copy:
          </p>
          <ol className="ml-4 list-decimal space-y-1 text-sm text-slate-700 dark:text-slate-300">
            <li>Click "Open Dropbox" below.</li>
            <li>Sign in and click "Allow" on Dropbox.</li>
            <li>Dropbox shows you a code — copy it.</li>
            <li>Paste it back here and click Add.</li>
          </ol>
          <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
            Tokens (access + refresh) live in your OS keychain.
            Refreshes happen automatically; you only sign in once.
          </p>

          <div className="mt-3">
            <CredField
              label="Base path"
              value={basePath}
              onChange={setBasePath}
              placeholder="/ (whole Dropbox) or /Subfolder"
            />
          </div>

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
              onClick={startAuth}
              disabled={busy || basePath.trim() === ''}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              Open Dropbox
            </button>
          </div>
        </>
      )}

      {phase.kind === 'awaiting' && (
        <>
          <div className="rounded-lg border border-purple-200 bg-purple-50 p-3 text-xs text-purple-900 dark:border-purple-900/60 dark:bg-purple-950/30 dark:text-purple-100">
            Dropbox should have opened. If not,{' '}
            <a
              href={phase.authorizeUrl}
              target="_blank"
              rel="noreferrer"
              className="underline"
            >
              open this link
            </a>
            . After clicking Allow, Dropbox shows a one-time code —
            paste it here.
          </div>

          <div className="mt-3">
            <CredField
              label="Authorization code"
              value={code}
              onChange={setCode}
              placeholder="Paste the code Dropbox showed you"
              autoFocus
            />
          </div>

          {error && (
            <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
              {error}
            </div>
          )}

          <div className="mt-6 flex items-center justify-between gap-2">
            <button
              onClick={() => {
                setPhase({ kind: 'idle' });
                setCode('');
              }}
              disabled={busy}
              className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
            >
              Back
            </button>
            <div className="flex items-center gap-2">
              <button
                onClick={onCancel}
                disabled={busy}
                className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
              >
                Cancel
              </button>
              <button
                onClick={submit}
                disabled={busy || code.trim() === ''}
                className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                Add Dropbox source
              </button>
            </div>
          </div>
        </>
      )}
    </>
  );
}

function DropboxPatForm({
  basePath,
  setBasePath,
  onAdded,
  onCancel,
}: {
  basePath: string;
  setBasePath: (v: string) => void;
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [accessToken, setAccessToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [testStatus, setTestStatus] = useState<'idle' | 'ok' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const canSubmit =
    !busy && accessToken.trim() !== '' && basePath.trim() !== '';

  const test = async () => {
    setError(null);
    setTestStatus(null);
    setBusy(true);
    try {
      await invoke<void>('test_dropbox_credentials', {
        accessToken: accessToken.trim(),
      });
      setTestStatus('ok');
      toast.success('Token OK');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_dropbox_source', {
        basePath: basePath.trim(),
        accessToken: accessToken.trim(),
      });
      toast.success('Dropbox source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <CredField
        label="Access token"
        value={accessToken}
        onChange={(v) => {
          setAccessToken(v);
          setTestStatus(null);
        }}
        type="password"
        placeholder="sl.B…"
      />
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        Generate at{' '}
        <span className="font-mono">
          dropbox.com/developers/apps
        </span>{' '}
        → New app → Scoped access → Full Dropbox → Settings →
        Generated access token. Stored in your OS keychain; never
        sent to Sery servers.
      </p>

      <div className="mt-3">
        <CredField
          label="Base path"
          value={basePath}
          onChange={setBasePath}
          placeholder="/ (whole Dropbox) or /Subfolder"
        />
      </div>

      {error && (
        <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      {testStatus === 'ok' && !error && (
        <div className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-300">
          Token OK — ready to save.
        </div>
      )}

      <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-300">
        Rescan downloads matching files (CSV / Parquet / Excel /
        docs) to{' '}
        <span className="font-mono">
          ~/.seryai/dropbox-cache/&lt;id&gt;/
        </span>{' '}
        and indexes them locally. Subsequent rescans skip files
        whose remote size + mtime are unchanged (incremental sync).
      </div>

      <div className="mt-6 flex items-center justify-between gap-2">
        <button
          onClick={test}
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
        >
          {busy && testStatus !== 'ok' && (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          )}
          Test token
        </button>
        <div className="flex items-center gap-2">
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
            Add Dropbox source
          </button>
        </div>
      </div>
    </>
  );
}

// ─── Stage B — WebDAV form ─────────────────────────────────────────

function WebDavStage({
  onAdded,
  onCancel,
}: {
  onAdded: () => void;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [serverUrl, setServerUrl] = useState('');
  const [basePath, setBasePath] = useState('/');
  const [authMode, setAuthMode] = useState<
    'anonymous' | 'basic' | 'digest'
  >('basic');
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [busy, setBusy] = useState(false);
  const [testStatus, setTestStatus] = useState<'idle' | 'ok' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const buildAuth = (): WebDavAuth | null => {
    if (authMode === 'anonymous') return { type: 'anonymous' };
    if (!username.trim() || !password) return null;
    return authMode === 'basic'
      ? { type: 'basic', username: username.trim(), password }
      : { type: 'digest', username: username.trim(), password };
  };

  const auth = buildAuth();
  const canSubmit =
    !busy &&
    serverUrl.trim() !== '' &&
    basePath.trim() !== '' &&
    auth !== null;

  const test = async () => {
    if (!auth) return;
    setError(null);
    setTestStatus(null);
    setBusy(true);
    try {
      await invoke<void>('test_webdav_credentials', {
        serverUrl: serverUrl.trim(),
        auth,
      });
      setTestStatus('ok');
      toast.success('Connection OK');
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (!auth) return;
    setError(null);
    setBusy(true);
    try {
      await invoke<string>('add_webdav_source', {
        serverUrl: serverUrl.trim(),
        basePath: basePath.trim(),
        auth,
      });
      toast.success('WebDAV source added');
      onAdded();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <CredField
        label="Server URL"
        value={serverUrl}
        onChange={(v) => {
          setServerUrl(v);
          setTestStatus(null);
        }}
        placeholder="https://nc.example.com/remote.php/dav/files/<user>/"
      />
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        Nextcloud:{' '}
        <span className="font-mono">
          https://nc.example.com/remote.php/dav/files/&lt;user&gt;/
        </span>
        {' · '}ownCloud:{' '}
        <span className="font-mono">
          https://owncloud.example.com/remote.php/dav/files/&lt;user&gt;/
        </span>
      </p>

      <div className="mt-3">
        <CredField
          label="Base path"
          value={basePath}
          onChange={setBasePath}
          placeholder="/Documents"
        />
      </div>

      <div className="mt-4 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
          <KeyRound className="h-3.5 w-3.5" />
          Authentication
        </div>
        <div className="mb-3 flex flex-wrap gap-2 text-xs">
          <button
            type="button"
            onClick={() => {
              setAuthMode('basic');
              setTestStatus(null);
            }}
            className={`rounded-md border px-3 py-1 transition-colors ${
              authMode === 'basic'
                ? 'border-purple-500 bg-purple-600 text-white'
                : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
            }`}
          >
            Basic
          </button>
          <button
            type="button"
            onClick={() => {
              setAuthMode('digest');
              setTestStatus(null);
            }}
            className={`rounded-md border px-3 py-1 transition-colors ${
              authMode === 'digest'
                ? 'border-purple-500 bg-purple-600 text-white'
                : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
            }`}
          >
            Digest
          </button>
          <button
            type="button"
            onClick={() => {
              setAuthMode('anonymous');
              setTestStatus(null);
            }}
            className={`rounded-md border px-3 py-1 transition-colors ${
              authMode === 'anonymous'
                ? 'border-purple-500 bg-purple-600 text-white'
                : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
            }`}
          >
            Anonymous
          </button>
        </div>
        {authMode !== 'anonymous' ? (
          <div className="grid gap-2 sm:grid-cols-2">
            <CredField
              label="Username"
              value={username}
              onChange={(v) => {
                setUsername(v);
                setTestStatus(null);
              }}
              placeholder="alice"
            />
            <CredField
              label="Password / app token"
              value={password}
              onChange={(v) => {
                setPassword(v);
                setTestStatus(null);
              }}
              type="password"
              placeholder="••••••••"
            />
          </div>
        ) : (
          <p className="text-xs text-purple-800/80 dark:text-purple-200/80">
            No credentials sent. Only works for public WebDAV
            servers — most providers refuse anonymous access.
          </p>
        )}
        {authMode === 'basic' && (
          <p className="mt-2 text-xs text-purple-800/80 dark:text-purple-200/80">
            For Nextcloud / ownCloud, generate an app password in
            Settings → Security and paste it here. Stored in your
            OS keychain; never sent to Sery servers.
          </p>
        )}
      </div>

      {error && (
        <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      {testStatus === 'ok' && !error && (
        <div className="mt-3 rounded-md border border-emerald-300 bg-emerald-50 p-2 text-xs text-emerald-700 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-300">
          Connection OK — ready to save.
        </div>
      )}

      <div className="mt-3 rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-700 dark:bg-slate-900/40 dark:text-slate-300">
        Rescan downloads matching files (CSV / Parquet / Excel /
        docs) to{' '}
        <span className="font-mono">
          ~/.seryai/webdav-cache/&lt;id&gt;/
        </span>{' '}
        and indexes them locally. Subsequent rescans skip files
        whose remote size + mtime are unchanged (incremental sync).
      </div>

      <div className="mt-6 flex items-center justify-between gap-2">
        <button
          onClick={test}
          disabled={!canSubmit}
          className="inline-flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
        >
          {busy && testStatus !== 'ok' && (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          )}
          Test connection
        </button>
        <div className="flex items-center gap-2">
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
            Add WebDAV source
          </button>
        </div>
      </div>
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
  kind: ImplementedKind | S3CompatibleKind | ComingSoonKind,
):
  | 'local'
  | 'http'
  | 's3'
  | 'gdrive'
  | 'sftp'
  | 'webdav'
  | 'dropbox'
  | 'azure'
  | 'onedrive'
  | null {
  switch (kind) {
    case 'local':
      return 'local';
    case 'https':
      return 'http';
    case 's3':
    // S3-compatible providers all use the S3 icon — they speak the
    // same wire protocol DuckDB already supports.
    // eslint-disable-next-line no-fallthrough
    case 'b2':
    case 'wasabi':
    case 'r2':
    case 'gcs':
      return 's3';
    case 'gdrive':
      return 'gdrive';
    case 'sftp':
      return 'sftp';
    case 'webdav':
      return 'webdav';
    case 'dropbox':
      return 'dropbox';
    case 'azure':
      return 'azure';
    case 'onedrive':
      return 'onedrive';
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
