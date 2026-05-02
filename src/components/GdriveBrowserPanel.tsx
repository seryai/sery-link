// Google Drive browser panel.
//
// Lives inside AddRemoteSourceModal as the "Google Drive" tab. View
// states:
//
//   1. NOT_CONFIGURED — build was compiled without
//      GOOGLE_OAUTH_CLIENT_ID; back-end command returns an error.
//   2. NOT_CONNECTED — no tokens in keychain. Big "Connect Google
//      Drive" button → fires start_gdrive_oauth → browser opens →
//      user consents → loopback handler stores tokens → Tauri emits
//      gdrive-oauth-complete event.
//   3. CONNECTED — tokens exist. Folder browser: drill in via
//      breadcrumbs, "Watch this folder" downloads its contents into
//      the local cache and registers the folder as a Sery source.
//      A separate "Watching" section lists already-watched folders
//      with an unwatch action.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  ChevronRight,
  Cloud,
  Folder,
  FolderOpen,
  Loader2,
  LogIn,
  LogOut,
  Plus,
  X,
} from 'lucide-react';
import { useToast } from './Toast';

interface DriveFile {
  id: string;
  name: string;
  mime_type: string;
  size?: number;
  modified_time: string;
  parents: string[];
}

interface GdriveWatchedFolder {
  account_id: string;
  folder_id: string;
  name: string;
  last_walk_at?: string | null;
  file_ids: string[];
}

const FOLDER_MIME = 'application/vnd.google-apps.folder';

interface GdriveOAuthEvent {
  ok: boolean;
  account?: string;
  error?: string;
}

type WatchProgress =
  | { folder_id: string; phase: 'walking' }
  | {
      folder_id: string;
      phase: 'downloading';
      current: number;
      total: number;
      file_name: string;
    }
  | { folder_id: string; phase: 'scanning' }
  | {
      folder_id: string;
      phase: 'done';
      total_files: number;
      skipped_native: number;
    };

type ViewState =
  | { kind: 'loading' }
  | { kind: 'not-configured' }
  | { kind: 'not-connected' }
  | { kind: 'connecting' }
  | { kind: 'connected'; loadingFolders: boolean };

interface BreadcrumbEntry {
  id: string;
  name: string;
}

export function GdriveBrowserPanel() {
  const toast = useToast();
  const [state, setState] = useState<ViewState>({ kind: 'loading' });
  const [folders, setFolders] = useState<DriveFile[]>([]);
  const [crumbs, setCrumbs] = useState<BreadcrumbEntry[]>([
    { id: 'root', name: 'My Drive' },
  ]);
  const [error, setError] = useState<string | null>(null);
  const [watching, setWatching] = useState<WatchProgress | null>(null);
  const [watchedFolders, setWatchedFolders] = useState<GdriveWatchedFolder[]>([]);

  // ── Initial state probe ─────────────────────────────────────────
  useEffect(() => {
    (async () => {
      try {
        const connected = await invoke<boolean>('gdrive_status');
        if (connected) {
          setState({ kind: 'connected', loadingFolders: true });
          await Promise.all([loadFolder('root'), loadWatchedFolders()]);
        } else {
          setState({ kind: 'not-connected' });
        }
      } catch (err) {
        console.warn('gdrive_status failed:', err);
        setState({ kind: 'not-connected' });
      }
    })();
  }, []);

  // ── OAuth completion listener ───────────────────────────────────
  useEffect(() => {
    const unlisten = listen<GdriveOAuthEvent>('gdrive-oauth-complete', async (event) => {
      const payload = event.payload;
      if (payload.ok) {
        toast.success('Google Drive connected');
        setState({ kind: 'connected', loadingFolders: true });
        setCrumbs([{ id: 'root', name: 'My Drive' }]);
        await Promise.all([loadFolder('root'), loadWatchedFolders()]);
      } else {
        const msg = payload.error || 'OAuth flow failed';
        if (/access_denied|user.?cancel/i.test(msg)) {
          toast.info('Google Drive connection cancelled');
        } else {
          toast.error(`Google Drive: ${msg}`);
        }
        setState({ kind: 'not-connected' });
      }
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [toast]);

  // ── Watch progress listener ─────────────────────────────────────
  // Backend emits gdrive-watch-progress with `phase` + per-phase
  // payload fields. We render this as a progress bar; on `done` we
  // refresh the watched-folders list and clear the progress UI.
  useEffect(() => {
    const unlisten = listen<WatchProgress>('gdrive-watch-progress', (event) => {
      setWatching(event.payload);
      if (event.payload.phase === 'done') {
        toast.success(
          `Watching now scans ${event.payload.total_files} files` +
            (event.payload.skipped_native > 0
              ? ` (${event.payload.skipped_native} Google Docs skipped — export support coming)`
              : ''),
        );
        void loadWatchedFolders();
        // Clear after a short beat so users see "done" before it
        // disappears. setTimeout is fine here — no race-y state.
        setTimeout(() => setWatching(null), 1500);
      }
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [toast]);

  // ── Background-refresh listener ─────────────────────────────────
  // The hourly gdrive_refresh tick emits gdrive-refresh per folder.
  // We just re-fetch the watched list so the "last refreshed N min
  // ago" label and file count stay accurate without forcing the user
  // to reopen the modal.
  useEffect(() => {
    const unlisten = listen<{
      folder_id: string;
      downloaded: number;
      deleted: number;
    }>('gdrive-refresh', () => {
      void loadWatchedFolders();
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, []);

  async function loadFolder(folderId: string) {
    setError(null);
    setState({ kind: 'connected', loadingFolders: true });
    try {
      const result = await invoke<DriveFile[]>('gdrive_list_folder', {
        folderId,
        includeFolders: true,
      });
      setFolders(result);
      setState({ kind: 'connected', loadingFolders: false });
    } catch (err) {
      const msg = String(err);
      if (/not configured for this build/i.test(msg)) {
        setState({ kind: 'not-configured' });
        return;
      }
      if (/no.+google drive account/i.test(msg) || /401|invalid_grant/i.test(msg)) {
        setError('Authentication expired. Reconnect to Google Drive.');
        setState({ kind: 'not-connected' });
        return;
      }
      setError(msg);
      setState({ kind: 'connected', loadingFolders: false });
    }
  }

  async function loadWatchedFolders() {
    try {
      const list = await invoke<GdriveWatchedFolder[]>(
        'gdrive_list_watched_folders',
      );
      setWatchedFolders(list);
    } catch (err) {
      console.warn('gdrive_list_watched_folders failed:', err);
    }
  }

  async function handleConnect() {
    setError(null);
    setState({ kind: 'connecting' });
    try {
      await invoke('start_gdrive_oauth');
    } catch (err) {
      const msg = String(err);
      if (/not configured for this build/i.test(msg)) {
        setState({ kind: 'not-configured' });
      } else {
        toast.error(msg);
        setState({ kind: 'not-connected' });
      }
    }
  }

  async function handleDisconnect() {
    if (
      !confirm(
        'Disconnect Google Drive? Watched folders will be removed and cached files deleted.',
      )
    ) {
      return;
    }
    try {
      await invoke('disconnect_gdrive');
      toast.success('Google Drive disconnected');
      setState({ kind: 'not-connected' });
      setFolders([]);
      setWatchedFolders([]);
      setCrumbs([{ id: 'root', name: 'My Drive' }]);
    } catch (err) {
      toast.error(`Disconnect failed: ${err}`);
    }
  }

  function handleEnterFolder(folder: DriveFile) {
    setCrumbs((prev) => [...prev, { id: folder.id, name: folder.name }]);
    void loadFolder(folder.id);
  }

  function handleCrumb(index: number) {
    const newCrumbs = crumbs.slice(0, index + 1);
    setCrumbs(newCrumbs);
    void loadFolder(newCrumbs[newCrumbs.length - 1].id);
  }

  async function handleWatch(folderId: string, folderName: string) {
    if (folderId === 'root') {
      // Watching all of My Drive could be hundreds of GB — almost
      // certainly not what the user meant.
      toast.error("Pick a specific folder — Sery won't watch your entire Drive at once.");
      return;
    }
    if (watchedFolders.some((w) => w.folder_id === folderId)) {
      toast.info(`Already watching "${folderName}"`);
      return;
    }
    try {
      setWatching({ folder_id: folderId, phase: 'walking' });
      await invoke('gdrive_watch_folder', {
        folderId,
        folderName,
      });
      // Final toast / cleanup happens in the gdrive-watch-progress
      // listener when the `done` event arrives — keeping the success
      // path single-sourced avoids "two toasts on success" flickers.
    } catch (err) {
      setWatching(null);
      toast.error(`Couldn't watch folder: ${err}`);
    }
  }

  async function handleUnwatch(folder: GdriveWatchedFolder) {
    if (
      !confirm(
        `Stop watching "${folder.name}"? Cached files for this folder will be removed.`,
      )
    ) {
      return;
    }
    try {
      await invoke('gdrive_unwatch_folder', { folderId: folder.folder_id });
      toast.success(`Stopped watching "${folder.name}"`);
      await loadWatchedFolders();
    } catch (err) {
      toast.error(`Unwatch failed: ${err}`);
    }
  }

  // ── Render branches ─────────────────────────────────────────────

  if (state.kind === 'loading') {
    return (
      <div className="flex items-center justify-center py-10 text-sm text-slate-500 dark:text-slate-400">
        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        Checking Google Drive status…
      </div>
    );
  }

  if (state.kind === 'not-configured') {
    return (
      <div className="rounded-lg border border-amber-200 bg-amber-50/60 p-4 text-sm text-amber-900 dark:border-amber-900/60 dark:bg-amber-950/20 dark:text-amber-200">
        <div className="mb-1 font-semibold">
          Google Drive not configured for this build
        </div>
        <p className="leading-relaxed">
          This Sery Link binary was compiled without a Google OAuth
          client ID. See{' '}
          <code className="rounded bg-amber-100 px-1 py-0.5 text-[11px] dark:bg-amber-900/40">
            datalake/SETUP_GOOGLE_OAUTH.md
          </code>{' '}
          for the maintainer-side setup.
        </p>
      </div>
    );
  }

  if (state.kind === 'not-connected' || state.kind === 'connecting') {
    const busy = state.kind === 'connecting';
    return (
      <div className="flex flex-col items-center rounded-lg border border-purple-200 bg-purple-50/40 px-6 py-8 text-center dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-full bg-white shadow-sm dark:bg-slate-800">
          <Cloud className="h-6 w-6 text-purple-600 dark:text-purple-400" />
        </div>
        <h3 className="mb-1 text-base font-semibold text-slate-900 dark:text-slate-50">
          Connect Google Drive
        </h3>
        <p className="mx-auto mb-5 max-w-sm text-xs leading-relaxed text-slate-600 dark:text-slate-400">
          Sery Link will open your browser to Google&apos;s consent
          screen. We request <strong>read-only</strong> access only
          (drive.readonly + drive.metadata.readonly). Tokens are
          stored in your OS keychain and never sent to Sery&apos;s
          servers.
        </p>
        <button
          onClick={handleConnect}
          disabled={busy}
          className="inline-flex items-center gap-2 rounded-md bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {busy ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <LogIn className="h-4 w-4" />
          )}
          {busy ? 'Waiting for consent…' : 'Connect Google Drive'}
        </button>
        {error && (
          <p className="mt-3 text-xs text-rose-700 dark:text-rose-300">{error}</p>
        )}
      </div>
    );
  }

  // state.kind === 'connected'
  return (
    <div>
      {/* Breadcrumb */}
      <div className="mb-2 flex flex-wrap items-center gap-1 text-xs text-slate-600 dark:text-slate-400">
        {crumbs.map((c, i) => (
          <span key={c.id} className="flex items-center gap-1">
            <button
              onClick={() => handleCrumb(i)}
              disabled={i === crumbs.length - 1}
              className="rounded px-1.5 py-0.5 hover:bg-slate-100 disabled:cursor-default disabled:font-semibold disabled:text-slate-900 disabled:hover:bg-transparent dark:hover:bg-slate-800 dark:disabled:text-slate-100"
            >
              {c.name}
            </button>
            {i < crumbs.length - 1 && (
              <ChevronRight className="h-3 w-3 text-slate-300 dark:text-slate-600" />
            )}
          </span>
        ))}
      </div>

      {/* Hint copy — clarifies the click affordance. The previous
          UX hid all CTAs in a top-right button that was disabled at
          root, leaving users confused about how to actually pick a
          folder. Now: click row body to drill in, click Watch on
          the right to add it as a Sery source. */}
      <p className="mb-2 text-[11px] text-slate-500 dark:text-slate-400">
        Click a folder name to open it. Click <strong>Watch</strong> to add it as a Sery source.
      </p>

      {/* Folder list */}
      <div className="max-h-72 overflow-y-auto rounded-lg border border-slate-200 dark:border-slate-700">
        {state.loadingFolders ? (
          <div className="flex items-center justify-center py-8 text-sm text-slate-500 dark:text-slate-400">
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
            Loading folder…
          </div>
        ) : folders.length === 0 ? (
          <div className="py-8 text-center text-sm text-slate-500 dark:text-slate-400">
            This folder is empty.
          </div>
        ) : (
          <ul className="divide-y divide-slate-200 dark:divide-slate-700">
            {folders.map((f) => {
              const isFolder = f.mime_type === FOLDER_MIME;
              const isWatched = watchedFolders.some((w) => w.folder_id === f.id);
              return (
                <li
                  key={f.id}
                  className={`flex items-center gap-2 px-3 py-2 text-sm ${
                    isFolder ? '' : 'text-slate-500 dark:text-slate-400'
                  }`}
                >
                  {isFolder ? (
                    <FolderOpen className="h-4 w-4 flex-shrink-0 text-amber-500" />
                  ) : (
                    <Folder className="h-4 w-4 flex-shrink-0 text-slate-400" />
                  )}
                  <button
                    onClick={() => isFolder && handleEnterFolder(f)}
                    disabled={!isFolder}
                    className={`min-w-0 flex-1 truncate text-left ${
                      isFolder
                        ? 'cursor-pointer hover:underline'
                        : 'cursor-default'
                    }`}
                  >
                    {f.name}
                  </button>
                  {isFolder &&
                    (isWatched ? (
                      <span className="flex-shrink-0 rounded bg-emerald-50 px-1.5 py-0.5 text-[11px] font-medium text-emerald-700 dark:bg-emerald-950/40 dark:text-emerald-300">
                        Watching
                      </span>
                    ) : (
                      <button
                        onClick={(e) => {
                          // Prevent the row's drill-in click from
                          // also firing — Watch should be a leaf
                          // action, not navigation.
                          e.stopPropagation();
                          void handleWatch(f.id, f.name);
                        }}
                        disabled={watching !== null}
                        className="inline-flex flex-shrink-0 items-center gap-1 rounded-md bg-purple-600 px-2 py-1 text-[11px] font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-50"
                      >
                        <Plus className="h-3 w-3" />
                        Watch
                      </button>
                    ))}
                  {isFolder && (
                    <ChevronRight className="h-3.5 w-3.5 flex-shrink-0 text-slate-400" />
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>

      {error && (
        <p className="mt-2 text-xs text-rose-700 dark:text-rose-300">{error}</p>
      )}

      {/* Watch progress — only shown while a watch is in flight */}
      {watching && <WatchProgressBar progress={watching} />}

      {/* Currently watched */}
      {watchedFolders.length > 0 && (
        <div className="mt-4">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
            Watching
          </div>
          <ul className="divide-y divide-slate-200 rounded-lg border border-slate-200 dark:divide-slate-700 dark:border-slate-700">
            {watchedFolders.map((w) => (
              <li
                key={`${w.account_id}:${w.folder_id}`}
                className="flex items-center gap-2 px-3 py-2 text-sm"
              >
                <FolderOpen className="h-4 w-4 flex-shrink-0 text-purple-500" />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-slate-900 dark:text-slate-100">
                    {w.name}
                  </div>
                  <div className="text-[11px] text-slate-500 dark:text-slate-400">
                    {w.file_ids.length} file{w.file_ids.length === 1 ? '' : 's'}
                    {w.last_walk_at &&
                      ` · last refreshed ${formatRelativeTime(w.last_walk_at)}`}
                  </div>
                </div>
                <button
                  onClick={() => handleUnwatch(w)}
                  className="flex-shrink-0 rounded p-1 text-slate-400 hover:bg-slate-100 hover:text-rose-600 dark:hover:bg-slate-800 dark:hover:text-rose-400"
                  title="Stop watching"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {/* Footer */}
      <div className="mt-4 flex items-center justify-end">
        <button
          onClick={handleDisconnect}
          className="inline-flex items-center gap-1 text-xs text-rose-600 hover:text-rose-700 dark:text-rose-400 dark:hover:text-rose-300"
        >
          <LogOut className="h-3 w-3" />
          Disconnect Google Drive
        </button>
      </div>
    </div>
  );
}

function WatchProgressBar({ progress }: { progress: WatchProgress }) {
  const message = (() => {
    switch (progress.phase) {
      case 'walking':
        return 'Scanning Drive folder…';
      case 'downloading':
        return `Downloading ${progress.current} of ${progress.total} — ${progress.file_name}`;
      case 'scanning':
        return 'Indexing files…';
      case 'done':
        return `Done — ${progress.total_files} files cached`;
    }
  })();

  const percent =
    progress.phase === 'downloading'
      ? Math.min(100, (progress.current / Math.max(1, progress.total)) * 100)
      : progress.phase === 'done'
        ? 100
        : null;

  return (
    <div className="mt-3 rounded-md border border-purple-200 bg-purple-50/60 p-3 text-xs text-purple-900 dark:border-purple-900/60 dark:bg-purple-950/30 dark:text-purple-200">
      <div className="mb-2 flex items-center gap-2">
        {progress.phase !== 'done' && (
          <Loader2 className="h-3.5 w-3.5 animate-spin" />
        )}
        <span className="truncate">{message}</span>
      </div>
      <div className="h-1 overflow-hidden rounded-full bg-purple-200/60 dark:bg-purple-900/40">
        <div
          className="h-full bg-purple-600 transition-all dark:bg-purple-400"
          style={{ width: percent !== null ? `${percent}%` : '40%' }}
        />
      </div>
    </div>
  );
}

/** Lightweight relative-time formatter. Big projects use Intl
 *  RelativeTimeFormat or date-fns; this UI just needs minutes/hours/
 *  days granularity for the "last refreshed" label. */
function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const seconds = Math.max(0, (Date.now() - then) / 1000);
  if (seconds < 60) return 'just now';
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} hr ago`;
  const days = Math.floor(hours / 24);
  return `${days} day${days === 1 ? '' : 's'} ago`;
}
