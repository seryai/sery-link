// CatchUpDialog — opt-in metadata catch-up for folders that were
// indexed locally before the workspace was connected.
//
// Two entry points:
//   1. ConnectModal phase-2: fires automatically right after a
//      successful auth_with_key, when list_catch_up_folders returns
//      a non-empty list. The connect flow's "happy path" surface.
//   2. StatusBar follow-up: a small "N folders to share" pill that
//      opens this dialog standalone when the user clicked "Not now"
//      on the auto-prompt. Without this surface, "Not now" stranded
//      the user — there was no way back to the catch-up flow short
//      of disconnecting and reconnecting.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { CheckCircle2, FolderOpen, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';

export interface CatchUpFolder {
  path: string;
  datasets: number;
  total_bytes: number;
  last_scan_at: string | null;
}

type Props = {
  /** Folders to offer for catch-up. Caller is responsible for fetching
   * via `list_catch_up_folders` — keeps this component pure / testable. */
  folders: CatchUpFolder[];
  onClose: () => void;
  /** Headline override. Defaults to a "Connected. One more step?" prompt
   * suitable for the post-connect path. The standalone-from-StatusBar
   * caller passes a follow-up framing. */
  title?: string;
  subtitle?: string;
};

export function CatchUpDialog({ folders, onClose, title, subtitle }: Props) {
  const toast = useToast();
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(
    () => new Set(folders.map((f) => f.path)),
  );
  const [submitting, setSubmitting] = useState(false);

  const togglePath = (path: string) => {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const handleSyncNow = async () => {
    const paths = Array.from(selectedPaths);
    if (paths.length === 0) {
      onClose();
      return;
    }
    setSubmitting(true);
    // Fire-and-forget: catch_up_sync runs scans serially in the
    // background. The cloud-side scan_status pill is the primary
    // progress surface; closing the modal lets the user keep
    // working while it runs.
    invoke('catch_up_sync', { paths }).catch((err) => {
      console.error('catch_up_sync failed:', err);
      toast.error('Some folders failed to sync. Check Sery Link logs.');
    });
    toast.success(
      paths.length === 1
        ? 'Syncing 1 folder — progress shows in your dashboard.'
        : `Syncing ${paths.length} folders — progress shows in your dashboard.`,
    );
    onClose();
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="presentation"
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-md rounded-xl bg-white p-6 shadow-xl dark:bg-slate-900"
        aria-labelledby="catch-up-title"
      >
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2
              id="catch-up-title"
              className="flex items-center gap-2 text-lg font-semibold text-slate-900 dark:text-slate-50"
            >
              <CheckCircle2 className="h-5 w-5 text-emerald-500" />
              {title ?? 'Connected. One more step?'}
            </h2>
            <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
              {subtitle ??
                'You have folders indexed locally. Share their metadata with your workspace so the assistant and other machines can see them.'}
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
            aria-label="Close"
            disabled={submitting}
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <ul className="mt-2 max-h-72 space-y-1 overflow-y-auto rounded-lg border border-slate-200 dark:border-slate-800">
          {folders.map((f) => {
            const checked = selectedPaths.has(f.path);
            return (
              <li
                key={f.path}
                className="flex items-center gap-3 px-3 py-2 hover:bg-slate-50 dark:hover:bg-slate-800/50"
              >
                <input
                  type="checkbox"
                  checked={checked}
                  onChange={() => togglePath(f.path)}
                  disabled={submitting}
                  className="h-4 w-4 rounded border-slate-300 text-purple-600 focus:ring-purple-500 dark:border-slate-700"
                />
                <FolderOpen className="h-4 w-4 flex-shrink-0 text-slate-400" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
                    {basename(f.path)}
                  </div>
                  <div className="truncate text-xs text-slate-500 dark:text-slate-400">
                    {f.datasets} {f.datasets === 1 ? 'file' : 'files'}
                    {' · '}
                    {formatBytes(f.total_bytes)}
                  </div>
                </div>
              </li>
            );
          })}
        </ul>

        <p className="mt-3 text-xs text-slate-500 dark:text-slate-400">
          Only metadata is uploaded — file paths, schemas, and
          AI-generated descriptions. File contents stay on this
          machine.{' '}
          <a
            href="https://sery.ai/privacy#machines-and-datasets"
            target="_blank"
            rel="noopener noreferrer"
            className="underline hover:text-slate-700 dark:hover:text-slate-200"
          >
            What gets uploaded?
          </a>
        </p>

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="rounded-lg px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Not now
          </button>
          <button
            type="button"
            onClick={handleSyncNow}
            disabled={selectedPaths.size === 0 || submitting}
            className="inline-flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:bg-slate-300 dark:disabled:bg-slate-700"
          >
            {submitting && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            {selectedPaths.size === 0
              ? 'Sync'
              : selectedPaths.size === 1
                ? 'Sync 1 folder'
                : `Sync ${selectedPaths.size} folders`}
          </button>
        </div>
      </div>
    </div>
  );
}

function basename(path: string): string {
  const m = path.match(/[^/\\]+$/);
  return m ? m[0] : path;
}

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(
    sizes.length - 1,
    Math.floor(Math.log(bytes) / Math.log(k)),
  );
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${sizes[i]}`;
}
