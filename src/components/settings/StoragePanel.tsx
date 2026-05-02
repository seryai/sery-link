// Settings → Storage. Shows how much disk Sery is using and lets
// the user reclaim space without losing their OAuth tokens.
//
// Numbers come from the get_storage_info Tauri command which walks
// ~/.seryai recursively. The walk runs in spawn_blocking on the
// Rust side so big caches don't block the runtime; from the UI we
// just see a small loading state.

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertCircle,
  ChevronDown,
  ChevronRight,
  HardDrive,
  Loader2,
  RefreshCw,
  Trash2,
} from 'lucide-react';
import { useToast } from '../Toast';

interface StorageInfo {
  data_dir_bytes: number;
  gdrive_cache_bytes: number;
  free_bytes: number;
}

type SkipReason =
  | 'native_unexportable'
  | 'unsupported_extension'
  | 'too_large'
  | 'download_failed';

interface SkippedEntry {
  account_id: string;
  watch_folder_id: string;
  file_id: string;
  name: string;
  mime_type: string;
  size_bytes: number | null;
  reason: SkipReason;
  skipped_at: string;
  detail: string | null;
}

interface SkippedSummary {
  recent: SkippedEntry[];
  by_reason: Partial<Record<SkipReason, number>>;
  total: number;
}

const REASON_LABEL: Record<SkipReason, string> = {
  native_unexportable: 'Google Docs / Forms / Drawings',
  unsupported_extension: 'Non-indexable file types',
  too_large: 'Over the 1 GiB cap',
  download_failed: 'Download failed',
};

export function StoragePanel() {
  const toast = useToast();
  const [info, setInfo] = useState<StorageInfo | null>(null);
  const [skipped, setSkipped] = useState<SkippedSummary | null>(null);
  const [skippedExpanded, setSkippedExpanded] = useState(false);
  const [loading, setLoading] = useState(true);
  const [clearing, setClearing] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const [storageResult, skippedResult] = await Promise.all([
        invoke<StorageInfo>('get_storage_info'),
        invoke<SkippedSummary>('get_gdrive_skipped', { limit: 100 }).catch(() => ({
          recent: [],
          by_reason: {},
          total: 0,
        })),
      ]);
      setInfo(storageResult);
      setSkipped(skippedResult);
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

      {skipped && skipped.total > 0 && (
        <div className="rounded-xl border border-slate-200 bg-white p-5 dark:border-slate-800 dark:bg-slate-900">
          <h3 className="mb-2 flex items-center gap-2 text-base font-semibold text-slate-900 dark:text-slate-50">
            <AlertCircle className="h-4 w-4 text-amber-500" />
            Drive files Sery skipped
          </h3>
          <p className="mb-3 text-sm text-slate-600 dark:text-slate-400">
            {skipped.total.toLocaleString()} file
            {skipped.total === 1 ? '' : 's'} weren&apos;t indexed. Hover a row
            to see why; clearing the Drive cache resets this list.
          </p>
          <ul className="mb-3 space-y-1 text-sm">
            {(Object.keys(REASON_LABEL) as SkipReason[]).map((reason) => {
              const count = skipped.by_reason[reason] ?? 0;
              if (count === 0) return null;
              return (
                <li
                  key={reason}
                  className="flex items-baseline justify-between text-slate-600 dark:text-slate-400"
                >
                  <span>{REASON_LABEL[reason]}</span>
                  <span className="font-mono font-semibold text-slate-900 dark:text-slate-100">
                    {count.toLocaleString()}
                  </span>
                </li>
              );
            })}
          </ul>
          <button
            onClick={() => setSkippedExpanded((v) => !v)}
            className="inline-flex items-center gap-1 text-xs font-medium text-purple-600 hover:text-purple-700 dark:text-purple-400 dark:hover:text-purple-300"
          >
            {skippedExpanded ? (
              <ChevronDown className="h-3 w-3" />
            ) : (
              <ChevronRight className="h-3 w-3" />
            )}
            {skippedExpanded
              ? 'Hide files'
              : `Show ${Math.min(skipped.recent.length, 100)} most recent`}
          </button>

          {skippedExpanded && skipped.recent.length > 0 && (
            <ul className="mt-3 max-h-64 divide-y divide-slate-200 overflow-y-auto rounded-md border border-slate-200 dark:divide-slate-700 dark:border-slate-700">
              {skipped.recent.map((entry) => (
                <li
                  key={`${entry.skipped_at}-${entry.file_id}`}
                  className="flex items-center gap-2 px-3 py-1.5 text-xs"
                  title={entry.detail || REASON_LABEL[entry.reason]}
                >
                  <span className="min-w-0 flex-1 truncate text-slate-700 dark:text-slate-300">
                    {entry.name}
                  </span>
                  <span className="flex-shrink-0 text-slate-500 dark:text-slate-400">
                    {REASON_LABEL[entry.reason]}
                  </span>
                  {entry.size_bytes !== null && (
                    <span className="flex-shrink-0 font-mono text-slate-400 dark:text-slate-500">
                      {formatBytes(entry.size_bytes)}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

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
