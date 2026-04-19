// Add-remote-source dialog.
//
// Sibling of the filesystem "Watch Folder" button — this one takes a
// public HTTPS URL and registers it as a data source. The Rust side
// validates the URL, stores it in watched_folders, and relies on the
// usual rescan flow to pull schema + samples.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Globe, KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import { isS3Url } from '../utils/url';

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
              Add a remote file
            </h2>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Paste a public URL to a CSV or Parquet file. Sery Link fetches
              the schema locally — the file is never uploaded to our servers.
            </p>
          </div>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

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
          placeholder="https://example.com/data.csv  or  s3://bucket/path/file.parquet"
          className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
        />

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
              Keys are saved to your macOS Keychain and used only to read this
              bucket. They never leave your machine — queries run in DuckDB on
              your laptop.
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
            What works:
          </strong>{' '}
          public HTTPS links to <code>.csv</code>/<code>.parquet</code>,{' '}
          and <code>s3://bucket/path/file.parquet</code>-style S3 objects
          (with AWS keys).
          <br />
          <strong className="text-slate-800 dark:text-slate-200">
            Not yet:
          </strong>{' '}
          full S3 bucket listing, GCS / Azure, Google Sheets, databases.
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
      </div>
    </div>
  );
}
