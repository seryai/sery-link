// Google Drive browser panel — Phase 3c-2 of the cloud-connectors
// migration (datalake/SPEC_CLOUD_CONNECTORS_MIGRATION.md).
//
// Lives inside AddRemoteSourceModal as the "Google Drive" tab. Three
// states the user moves through:
//
//   1. NOT_CONFIGURED — build was compiled without
//      GOOGLE_OAUTH_CLIENT_ID; back-end command returns an error.
//      We render a docs pointer instead of misleading the user.
//   2. NOT_CONNECTED — no tokens in keychain. Big "Connect Google
//      Drive" button → fires start_gdrive_oauth → browser opens →
//      user consents → deep_link.rs receives the callback → tokens
//      land in keychain → Tauri emits gdrive-oauth-complete event.
//   3. CONNECTED — tokens exist. Folder browser: list of root
//      folders, click to drill in, breadcrumbs to navigate back.
//
// Phase 3c-2 explicitly does NOT include a "Watch this folder"
// action. That ships in 3c-3 alongside the scan walker so the
// affordance never points at a code path that doesn't yet do what
// it says.

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

const FOLDER_MIME = 'application/vnd.google-apps.folder';

interface GdriveOAuthEvent {
  ok: boolean;
  account?: string;
  error?: string;
}

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
  // Breadcrumb starts at "My Drive" pseudo-root. Drive's own root
  // ID for queries is the literal string 'root'.
  const [crumbs, setCrumbs] = useState<BreadcrumbEntry[]>([
    { id: 'root', name: 'My Drive' },
  ]);
  const [error, setError] = useState<string | null>(null);

  // ── Initial state probe ─────────────────────────────────────────
  useEffect(() => {
    (async () => {
      try {
        const connected = await invoke<boolean>('gdrive_status');
        if (connected) {
          setState({ kind: 'connected', loadingFolders: true });
          await loadFolder('root');
        } else {
          setState({ kind: 'not-connected' });
        }
      } catch (err) {
        // gdrive_status never errors today, but treat any failure as
        // "not connected" — graceful degradation.
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
        await loadFolder('root');
      } else {
        const msg = payload.error || 'OAuth flow failed';
        // Distinguish user-cancel from real error in the UX.
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
      // If the build wasn't configured with GOOGLE_OAUTH_CLIENT_ID,
      // gdrive_list_folder bubbles the same "not configured" error
      // that start_flow does. Render the not-configured panel.
      if (/not configured for this build/i.test(msg)) {
        setState({ kind: 'not-configured' });
        return;
      }
      // Token problems (refresh failure, revoked grant) — drop back
      // to "not connected" with a hint so the user reconnects.
      if (/no.+google drive account/i.test(msg) || /401|invalid_grant/i.test(msg)) {
        setError('Authentication expired. Reconnect to Google Drive.');
        setState({ kind: 'not-connected' });
        return;
      }
      setError(msg);
      setState({ kind: 'connected', loadingFolders: false });
    }
  }

  async function handleConnect() {
    setError(null);
    setState({ kind: 'connecting' });
    try {
      await invoke('start_gdrive_oauth');
      // After this returns, the browser is opening. Stay in
      // 'connecting' state; the gdrive-oauth-complete event listener
      // above moves us to 'connected' (or back to 'not-connected'
      // on failure/cancel).
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
    if (!confirm('Disconnect Google Drive? You can reconnect any time.')) {
      return;
    }
    try {
      await invoke('disconnect_gdrive');
      toast.success('Google Drive disconnected');
      setState({ kind: 'not-connected' });
      setFolders([]);
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
          for the maintainer-side setup, then rebuild with{' '}
          <code className="rounded bg-amber-100 px-1 py-0.5 text-[11px] dark:bg-amber-900/40">
            GOOGLE_OAUTH_CLIENT_ID
          </code>{' '}
          set in the build environment.
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
      <div className="mb-3 flex items-center gap-1 text-xs text-slate-600 dark:text-slate-400">
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
              return (
                <li
                  key={f.id}
                  className={`flex items-center gap-2 px-3 py-2 text-sm ${
                    isFolder
                      ? 'cursor-pointer hover:bg-slate-50 dark:hover:bg-slate-800/60'
                      : 'text-slate-500 dark:text-slate-400'
                  }`}
                  onClick={() => isFolder && handleEnterFolder(f)}
                >
                  {isFolder ? (
                    <FolderOpen className="h-4 w-4 flex-shrink-0 text-amber-500" />
                  ) : (
                    <Folder className="h-4 w-4 flex-shrink-0 text-slate-400" />
                  )}
                  <span className="truncate">{f.name}</span>
                  {isFolder && (
                    <ChevronRight className="ml-auto h-3.5 w-3.5 flex-shrink-0 text-slate-400" />
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

      {/* Footer note + disconnect. The "Watch this folder" button
          ships in Phase 3c-3 alongside the scan walker. Until then
          this panel is read-only browse — connection works end-to-end
          but the folder isn't yet a registered source. */}
      <div className="mt-4 flex items-center justify-between rounded-md border border-slate-200 bg-slate-50 p-3 text-xs text-slate-600 dark:border-slate-800 dark:bg-slate-900/40 dark:text-slate-400">
        <span>
          <strong className="text-slate-800 dark:text-slate-200">
            Phase 3c-2:
          </strong>{' '}
          browse only. Watch + scan ship in 3c-3.
        </span>
        <button
          onClick={handleDisconnect}
          className="inline-flex items-center gap-1 text-rose-600 hover:text-rose-700 dark:text-rose-400 dark:hover:text-rose-300"
        >
          <LogOut className="h-3 w-3" />
          Disconnect
        </button>
      </div>
    </div>
  );
}
