// Add-remote-source dialog.
//
// Sibling of the filesystem "Watch Folder" button — this one takes a
// public HTTPS URL and registers it as a data source. The Rust side
// validates the URL, stores it in watched_folders, and relies on the
// usual rescan flow to pull schema + samples.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Cloud, Globe, KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import { isS3Url } from '../utils/url';
import { GdriveBrowserPanel } from './GdriveBrowserPanel';

type Tab = 'url' | 'gdrive';

interface AddRemoteSourceModalProps {
  open: boolean;
  onClose: () => void;
  onAdded: (url: string) => void;
}

interface S3Credentials {
  access_key_id: string;
  secret_access_key: string;
  region: string;
  session_token?: string;
}

export function AddRemoteSourceModal({
  open,
  onClose,
  onAdded,
}: AddRemoteSourceModalProps) {
  const [tab, setTab] = useState<Tab>('url');
  const [url, setUrl] = useState('');
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [region, setRegion] = useState('us-east-1');
  const [sessionToken, setSessionToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();

  if (!open) return null;

  const trimmedUrl = url.trim();
  const isS3 = isS3Url(trimmedUrl);
  const insecure = trimmedUrl.toLowerCase().startsWith('http://');

  const resetForm = () => {
    setUrl('');
    setAccessKey('');
    setSecretKey('');
    setRegion('us-east-1');
    setSessionToken('');
    setError(null);
  };

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
      const normalised = await invoke<string>('add_remote_source', args);
      toast.success(isS3 ? 'S3 source added' : 'Remote source added');
      resetForm();
      onAdded(normalised);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const canSubmit =
    !busy &&
    trimmedUrl !== '' &&
    (!isS3 ||
      (accessKey.trim() !== '' &&
        secretKey.trim() !== '' &&
        region.trim() !== ''));

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-full max-w-lg rounded-xl border border-slate-200 bg-white p-6 shadow-xl dark:border-slate-800 dark:bg-slate-900"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2 className="flex items-center gap-2 text-lg font-bold text-slate-900 dark:text-slate-50">
              <Globe className="h-5 w-5 text-purple-600 dark:text-purple-400" />
              Add a remote source
            </h2>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Sery Link reads remote schemas locally and stores
              credentials in your OS keychain — nothing is uploaded
              to Sery&apos;s servers.
            </p>
          </div>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Protocol roadmap callout — closes I1 from
            UI_AUDIT_2026_05.md. The marketing claims 10 protocols;
            today's tabs only show 2 of the remote ones. The grid
            below makes the broader claim visible without lying
            about what works today. Tiles are informational only —
            clicking is a no-op until F42 ships the unified protocol
            picker. */}
        <ProtocolRoadmapGrid />

        {/* Tabs */}
        <div className="mb-4 flex gap-1 border-b border-slate-200 dark:border-slate-700">
          <TabButton active={tab === 'url'} onClick={() => setTab('url')}>
            <Globe className="h-3.5 w-3.5" />
            S3 / URL
          </TabButton>
          <TabButton active={tab === 'gdrive'} onClick={() => setTab('gdrive')}>
            <Cloud className="h-3.5 w-3.5" />
            Google Drive
          </TabButton>
        </div>

        {tab === 'gdrive' && <GdriveBrowserPanel onClose={onClose} />}

        {tab === 'url' && (
          <UrlPanel
            url={url}
            setUrl={setUrl}
            accessKey={accessKey}
            setAccessKey={setAccessKey}
            secretKey={secretKey}
            setSecretKey={setSecretKey}
            region={region}
            setRegion={setRegion}
            sessionToken={sessionToken}
            setSessionToken={setSessionToken}
            busy={busy}
            error={error}
            canSubmit={canSubmit}
            insecure={insecure}
            isS3={isS3}
            trimmedUrl={trimmedUrl}
            submit={submit}
            onClose={onClose}
          />
        )}
      </div>
    </div>
  );
}

/* ProtocolRoadmapGrid — informational tile grid above the modal
 * tabs that surfaces all 10 protocols the website promises. The
 * 4 currently-shipped ones are styled "Now"; the 7 v0.7+ ones are
 * styled "Coming". Tiles aren't clickable yet — they ship for real
 * via the F42 protocol-picker. The point is honest disclosure:
 * the modal stops looking like "we have 2 cloud sources" and starts
 * looking like "we have 4, with 7 more on the way."
 *
 * Note: Local Folder is included in the "Now" set and points users
 * at the "Watch Folder" button on the FolderList — it isn't reached
 * from this modal but is part of the 10-protocol claim. F42 will
 * unify both paths under one Add Source button. */
const PROTOCOL_TILES: Array<{
  name: string;
  short: string;
  available: boolean;
  hint?: string;
}> = [
  { name: 'Local folder', short: 'Local', available: true, hint: 'Use the Watch Folder button on the Folders page' },
  { name: 'HTTPS URL', short: 'HTTPS', available: true },
  { name: 'AWS S3', short: 'S3', available: true },
  { name: 'Google Drive', short: 'Drive', available: true },
  { name: 'SFTP', short: 'SFTP', available: false },
  { name: 'WebDAV', short: 'WebDAV', available: false },
  { name: 'Backblaze B2', short: 'B2', available: false },
  { name: 'Azure Blob', short: 'Azure', available: false },
  { name: 'Google Cloud Storage', short: 'GCS', available: false },
  { name: 'Dropbox', short: 'Dropbox', available: false },
  { name: 'OneDrive', short: 'OneDrive', available: false },
];

function ProtocolRoadmapGrid() {
  return (
    <div className="mb-4 rounded-md border border-slate-200 bg-slate-50/60 p-3 dark:border-slate-700 dark:bg-slate-900/40">
      <div className="mb-2 flex items-center justify-between text-[11px]">
        <span className="font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
          Storage protocols
        </span>
        <span className="text-slate-400 dark:text-slate-500">
          4 now · 7 coming in v0.7+
        </span>
      </div>
      <div className="grid grid-cols-3 gap-1.5 sm:grid-cols-4">
        {PROTOCOL_TILES.map((p) => (
          <div
            key={p.short}
            title={p.hint ?? (p.available ? 'Available — pick a tab below' : 'Coming in v0.7+')}
            className={
              p.available
                ? 'flex items-center justify-between gap-1 rounded border border-slate-200 bg-white px-2 py-1.5 text-[11px] dark:border-slate-700 dark:bg-slate-800'
                : 'flex items-center justify-between gap-1 rounded border border-dashed border-slate-200 bg-white/40 px-2 py-1.5 text-[11px] text-slate-400 dark:border-slate-700/50 dark:bg-slate-800/30 dark:text-slate-500'
            }
          >
            <span className={p.available ? 'font-medium text-slate-700 dark:text-slate-200' : ''}>
              {p.short}
            </span>
            <span
              className={
                p.available
                  ? 'rounded-sm bg-emerald-100 px-1 text-[9px] font-semibold uppercase tracking-wide text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300'
                  : 'rounded-sm bg-slate-100 px-1 text-[9px] font-medium uppercase tracking-wide text-slate-500 dark:bg-slate-800 dark:text-slate-400'
              }
            >
              {p.available ? 'Now' : 'v0.7+'}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`-mb-px flex items-center gap-1.5 border-b-2 px-3 py-2 text-sm font-semibold transition-colors ${
        active
          ? 'border-purple-600 text-purple-700 dark:border-purple-400 dark:text-purple-300'
          : 'border-transparent text-slate-500 hover:text-slate-700 dark:text-slate-400 dark:hover:text-slate-200'
      }`}
    >
      {children}
    </button>
  );
}

function UrlPanel(props: {
  url: string;
  setUrl: (s: string) => void;
  accessKey: string;
  setAccessKey: (s: string) => void;
  secretKey: string;
  setSecretKey: (s: string) => void;
  region: string;
  setRegion: (s: string) => void;
  sessionToken: string;
  setSessionToken: (s: string) => void;
  busy: boolean;
  error: string | null;
  canSubmit: boolean;
  insecure: boolean;
  isS3: boolean;
  trimmedUrl: string;
  submit: () => void;
  onClose: () => void;
}) {
  const {
    url,
    setUrl,
    accessKey,
    setAccessKey,
    secretKey,
    setSecretKey,
    region,
    setRegion,
    sessionToken,
    setSessionToken,
    busy,
    error,
    canSubmit,
    insecure,
    isS3,
    trimmedUrl,
    submit,
    onClose,
  } = props;
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
          placeholder="https:// CSV / Parquet URL, public Google Sheets share link, or s3:// URL"
          className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
        />
        <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
          Public Google Sheets URLs (
          <span className="font-mono">docs.google.com/spreadsheets/d/…</span>)
          are auto-converted to CSV — first tab only unless your link
          includes <span className="font-mono">#gid=N</span>. For private
          sheets, connect Google Drive instead.
        </p>

        {insecure && trimmedUrl !== '' && (
          <p className="mt-2 text-xs text-amber-700 dark:text-amber-300">
            This URL uses <span className="font-mono">http://</span> — the
            connection isn't encrypted. OK for local or internal endpoints,
            but prefer <span className="font-mono">https://</span> for public
            data.
          </p>
        )}

        {isS3 && (
          <div className="mt-4 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
            <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
              <KeyRound className="h-3.5 w-3.5" />
              AWS credentials
            </div>
            <p className="mb-3 text-xs text-purple-800/80 dark:text-purple-200/80">
              Keys are stored in your OS's credential store (Keychain
              on macOS, Credential Manager on Windows, Secret Service
              on Linux) and used only to read this bucket. Nothing is
              uploaded.
            </p>
            <div className="grid gap-2 sm:grid-cols-2">
              <label className="block">
                <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                  Access key ID
                </span>
                <input
                  type="text"
                  value={accessKey}
                  onChange={(e) => setAccessKey(e.target.value)}
                  placeholder="AKIA…"
                  className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
                />
              </label>
              <label className="block">
                <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                  Secret access key
                </span>
                <input
                  type="password"
                  value={secretKey}
                  onChange={(e) => setSecretKey(e.target.value)}
                  placeholder="••••••••"
                  className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
                />
              </label>
              <label className="block">
                <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                  Region
                </span>
                <input
                  type="text"
                  value={region}
                  onChange={(e) => setRegion(e.target.value)}
                  placeholder="us-east-1"
                  className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
                />
              </label>
              <label className="block">
                <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
                  Session token (optional)
                </span>
                <input
                  type="password"
                  value={sessionToken}
                  onChange={(e) => setSessionToken(e.target.value)}
                  placeholder="Only for temporary STS creds"
                  className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
                />
              </label>
            </div>
          </div>
        )}

        {error && (
          <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            {error}
          </div>
        )}

        <div className="mt-4 rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-600 dark:border-slate-800 dark:bg-slate-900/40 dark:text-slate-400">
          <strong className="text-slate-800 dark:text-slate-200">
            URL formats:
          </strong>{' '}
          public HTTPS links to <code>.csv</code>/<code>.parquet</code>,
          individual S3 objects, and S3 bucket/prefix listings like{' '}
          <code>s3://bucket/prefix/</code> (recursive — matches{' '}
          <code>.csv</code>/<code>.tsv</code>/<code>.parquet</code> at
          any depth, capped at 10,000 objects) or explicit globs like{' '}
          <code>s3://bucket/**/*.parquet</code>.
        </div>

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            onClick={onClose}
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
