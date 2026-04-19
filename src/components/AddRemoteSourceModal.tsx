// Add-remote-source dialog.
//
// Sibling of the filesystem "Watch Folder" button — this one takes a
// public HTTPS URL and registers it as a data source. The Rust side
// validates the URL, stores it in watched_folders, and relies on the
// usual rescan flow to pull schema + samples.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Globe, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';

interface AddRemoteSourceModalProps {
  open: boolean;
  onClose: () => void;
  onAdded: (url: string) => void;
}

export function AddRemoteSourceModal({
  open,
  onClose,
  onAdded,
}: AddRemoteSourceModalProps) {
  const [url, setUrl] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const toast = useToast();

  if (!open) return null;

  const insecure = url.trim().toLowerCase().startsWith('http://');

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const normalised = await invoke<string>('add_remote_source', { url });
      toast.success('Remote source added');
      setUrl('');
      onAdded(normalised);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

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
            if (e.key === 'Enter' && url.trim() && !busy) submit();
          }}
          placeholder="https://example.com/data.csv"
          className="w-full rounded-lg border border-slate-200 bg-white px-3 py-2 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
        />

        {insecure && url.trim() !== '' && (
          <p className="mt-2 text-xs text-amber-700 dark:text-amber-300">
            This URL uses <span className="font-mono">http://</span> — the
            connection isn't encrypted. OK for local or internal endpoints,
            but prefer <span className="font-mono">https://</span> for public
            data.
          </p>
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
          public HTTPS links to <code>.csv</code> or <code>.parquet</code>{' '}
          files. No login required.
          <br />
          <strong className="text-slate-800 dark:text-slate-200">
            Not yet:
          </strong>{' '}
          S3 / GCS buckets (need credentials), Google Sheets, databases.
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
            disabled={busy || url.trim() === ''}
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
