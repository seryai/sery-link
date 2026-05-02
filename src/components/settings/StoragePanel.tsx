// Settings → Storage. Shows how much disk Sery is using and lets
// the user reclaim space without losing their OAuth tokens.
//
// Numbers come from the get_storage_info Tauri command which walks
// ~/.seryai recursively. The walk runs in spawn_blocking on the
// Rust side so big caches don't block the runtime; from the UI we
// just see a small loading state.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { HardDrive, Loader2, RefreshCw, Trash2 } from 'lucide-react';
import { useToast } from '../Toast';

interface StorageInfo {
  data_dir_bytes: number;
  gdrive_cache_bytes: number;
  free_bytes: number;
}

export function StoragePanel() {
  const toast = useToast();
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [clearing, setClearing] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const result = await invoke<StorageInfo>('get_storage_info');
      setInfo(result);
    } catch (err) {
      toast.error(`Couldn't load storage info: ${err}`);
    } finally {
      setLoading(false);
    }
  }, [toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleClear = async () => {
    if (!info || info.gdrive_cache_bytes === 0) return;
    if (
      !confirm(
        `Clear the Google Drive cache? ${formatBytes(info.gdrive_cache_bytes)} will be freed. ` +
          'Your OAuth grant stays in place — re-watching reuses it without another consent.',
      )
    ) {
      return;
    }
    setClearing(true);
    try {
      await invoke('clear_gdrive_cache');
      toast.success('Drive cache cleared');
      await refresh();
    } catch (err) {
      toast.error(`Couldn't clear cache: ${err}`);
    } finally {
      setClearing(false);
    }
  };

  // Watching free space drop near the watch threshold deserves a
  // visual nudge — yellow at <10 GB, red at <5 GB (the threshold
  // gdrive_watch_folder enforces).
  const lowDiskTone = (() => {
    if (!info) return null;
    if (info.free_bytes < 5 * GB) return 'rose';
    if (info.free_bytes < 10 * GB) return 'amber';
    return null;
  })();

  return (
    <div className="space-y-4">
      <div className="rounded-xl border border-slate-200 bg-white p-5 dark:border-slate-800 dark:bg-slate-900">
        <div className="mb-3 flex items-center justify-between">
          <h3 className="flex items-center gap-2 text-base font-semibold text-slate-900 dark:text-slate-50">
            <HardDrive className="h-4 w-4 text-slate-500 dark:text-slate-400" />
            Disk usage
          </h3>
          <button
            onClick={refresh}
            disabled={loading}
            className="rounded-md p-1.5 text-slate-500 hover:bg-slate-100 hover:text-slate-700 disabled:opacity-50 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-200"
            title="Refresh"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
          </button>
        </div>

        {loading && !info ? (
          <div className="flex items-center gap-2 py-4 text-sm text-slate-500 dark:text-slate-400">
            <Loader2 className="h-4 w-4 animate-spin" />
            Measuring…
          </div>
        ) : info ? (
          <dl className="space-y-2 text-sm">
            <Row
              label="Free on this volume"
              value={formatBytes(info.free_bytes)}
              tone={lowDiskTone}
              note={
                lowDiskTone === 'rose'
                  ? 'Below the 5 GB threshold — Drive watches will refuse until you free more space.'
                  : lowDiskTone === 'amber'
                    ? 'Getting tight. New Drive watches need 5 GB free.'
                    : null
              }
            />
            <Row
              label="Sery data dir (~/.seryai)"
              value={formatBytes(info.data_dir_bytes)}
            />
            <Row
              label="Google Drive cache"
              value={formatBytes(info.gdrive_cache_bytes)}
              note={
                info.gdrive_cache_bytes > 0
                  ? 'Downloaded copies of files in your watched Drive folders. Safe to clear; re-watching re-downloads.'
                  : 'No Drive content cached yet.'
              }
            />
          </dl>
        ) : null}
      </div>

      {info && info.gdrive_cache_bytes > 0 && (
        <div className="rounded-xl border border-slate-200 bg-white p-5 dark:border-slate-800 dark:bg-slate-900">
          <h3 className="mb-2 text-base font-semibold text-slate-900 dark:text-slate-50">
            Reclaim space
          </h3>
          <p className="mb-3 text-sm text-slate-600 dark:text-slate-400">
            Clearing the Drive cache removes downloaded files but keeps your
            OAuth grant. Re-connecting Drive in Folders will start fresh
            without a new consent screen.
          </p>
          <button
            onClick={handleClear}
            disabled={clearing}
            className="inline-flex items-center gap-2 rounded-md bg-rose-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-rose-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {clearing ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Trash2 className="h-3.5 w-3.5" />
            )}
            Clear Drive cache ({formatBytes(info.gdrive_cache_bytes)})
          </button>
        </div>
      )}
    </div>
  );
}

function Row({
  label,
  value,
  note,
  tone,
}: {
  label: string;
  value: string;
  note?: string | null;
  tone?: 'amber' | 'rose' | null;
}) {
  const valueClass =
    tone === 'rose'
      ? 'text-rose-700 dark:text-rose-300'
      : tone === 'amber'
        ? 'text-amber-700 dark:text-amber-300'
        : 'text-slate-900 dark:text-slate-100';
  return (
    <div>
      <div className="flex items-baseline justify-between">
        <dt className="text-slate-600 dark:text-slate-400">{label}</dt>
        <dd className={`font-mono text-sm font-semibold ${valueClass}`}>{value}</dd>
      </div>
      {note && (
        <p className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">{note}</p>
      )}
    </div>
  );
}

const KB = 1024;
const MB = 1024 * KB;
const GB = 1024 * MB;
const TB = 1024 * GB;

function formatBytes(n: number): string {
  if (n >= TB) return `${(n / TB).toFixed(2)} TB`;
  if (n >= GB) return `${(n / GB).toFixed(2)} GB`;
  if (n >= MB) return `${(n / MB).toFixed(1)} MB`;
  if (n >= KB) return `${(n / KB).toFixed(0)} KB`;
  return `${n} B`;
}
