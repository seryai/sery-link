// Google Drive panel. Lives inside AddRemoteSourceModal as the
// "Google Drive" tab.
//
// v0.6 redesign: Drive is treated as a single watched source —
// connecting it auto-watches the entire My Drive root. The earlier
// folder picker UX (browse/select-with-checkbox) was confusing and
// added a step nobody really wanted; if a user has a huge Drive and
// wants to be selective, that comes back in a settings page later.
//
// Flow:
//   1. NOT_CONFIGURED — build was compiled without
//      GOOGLE_OAUTH_CLIENT_ID / SECRET; surface a docs pointer.
//   2. NOT_CONNECTED — no tokens. "Connect Google Drive" button
//      kicks off OAuth.
//   3. CONNECTING — browser is open; we're waiting for the
//      gdrive-oauth-complete event.
//   4. INDEXING — OAuth succeeded, the auto-watch is in flight.
//      Modal closes on success and the rest of the watch progress
//      flows through the existing scan-event UI in the main folder
//      list (gdrive-watch-progress emits the standard scan events
//      via the rescan_folder hand-off in the watch command).
//   5. CONNECTED — already-set-up state shown when the user reopens
//      the modal post-connect. Just shows status + Disconnect.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Cloud, Loader2, LogIn, LogOut } from 'lucide-react';
import { useToast } from './Toast';

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
  | { kind: 'indexing' }
  | { kind: 'connected' };

interface Props {
  /** Lets us close the parent modal once the auto-watch fires —
   *  matches the S3 / URL flow where submit closes the dialog and
   *  progress shows in the main UI. */
  onClose: () => void;
}

const ROOT_NAME = 'My Drive';

export function GdriveBrowserPanel({ onClose }: Props) {
  const toast = useToast();
  const [state, setState] = useState<ViewState>({ kind: 'loading' });
  const [error, setError] = useState<string | null>(null);

  // ── Initial state probe ─────────────────────────────────────────
  useEffect(() => {
    (async () => {
      try {
        const connected = await invoke<boolean>('gdrive_status');
        setState(connected ? { kind: 'connected' } : { kind: 'not-connected' });
      } catch (err) {
        console.warn('gdrive_status failed:', err);
        setState({ kind: 'not-connected' });
      }
    })();
  }, []);

  // ── OAuth completion → auto-watch root → close modal ───────────
  useEffect(() => {
    const unlisten = listen<GdriveOAuthEvent>('gdrive-oauth-complete', async (event) => {
      const payload = event.payload;
      if (!payload.ok) {
        const msg = payload.error || 'OAuth flow failed';
        if (/access_denied|user.?cancel/i.test(msg)) {
          toast.info('Google Drive connection cancelled');
        } else {
          toast.error(`Google Drive: ${msg}`);
        }
        setState({ kind: 'not-connected' });
        return;
      }

      // OAuth succeeded — kick off the auto-watch on root, then
      // close the modal. We deliberately fire-and-forget the
      // invoke: the watch command keeps emitting gdrive-watch-progress
      // events that the main UI listens for, so the user sees
      // progress in the folder list / status bar without staring
      // at a modal.
      toast.success('Google Drive connected — indexing your files…');
      setState({ kind: 'indexing' });
      try {
        // Don't await — the watch can take minutes for large Drives.
        // Errors surface via gdrive-watch-progress + toast in
        // whichever component subscribes (main UI today, this
        // component if reopened).
        invoke('gdrive_watch_folder', {
          folderId: 'root',
          folderName: ROOT_NAME,
        }).catch((err) => {
          console.error('auto-watch failed:', err);
          toast.error(`Couldn't start indexing: ${err}`);
        });
        // Close on next tick so the toast registers before the
        // modal teardown unmounts the toast root.
        setTimeout(onClose, 50);
      } catch (err) {
        toast.error(`Couldn't start indexing: ${err}`);
        setState({ kind: 'connected' });
      }
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [toast, onClose]);

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
    } catch (err) {
      toast.error(`Disconnect failed: ${err}`);
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

  if (state.kind === 'connected') {
    return (
      <div className="rounded-lg border border-emerald-200 bg-emerald-50/40 p-4 text-sm dark:border-emerald-900/60 dark:bg-emerald-950/20">
        <div className="mb-2 flex items-center gap-2 font-semibold text-emerald-900 dark:text-emerald-200">
          <Cloud className="h-4 w-4" />
          Google Drive connected
        </div>
        <p className="text-xs leading-relaxed text-slate-600 dark:text-slate-400">
          Sery is watching your Drive. New and changed files are
          re-indexed automatically. Tokens live in your OS keychain
          and never leave this machine.
        </p>
        <div className="mt-3 flex justify-end">
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

  if (state.kind === 'indexing') {
    return (
      <div className="flex flex-col items-center rounded-lg border border-purple-200 bg-purple-50/40 px-6 py-8 text-center dark:border-purple-900/60 dark:bg-purple-950/20">
        <Loader2 className="mb-3 h-6 w-6 animate-spin text-purple-600 dark:text-purple-400" />
        <h3 className="mb-1 text-base font-semibold text-slate-900 dark:text-slate-50">
          Indexing your Drive…
        </h3>
        <p className="mx-auto max-w-sm text-xs leading-relaxed text-slate-600 dark:text-slate-400">
          You can close this window. Sery will keep working in the
          background.
        </p>
      </div>
    );
  }

  // not-connected | connecting
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
        Sery will index every file in your Drive (Sheets, CSVs, PDFs,
        Office docs, …) so you can search and query it alongside
        your local folders. <strong>Read-only</strong> OAuth scope —
        Sery never modifies your Drive content. Tokens stay in your
        OS keychain.
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
