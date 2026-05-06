// Connect modal — the single entry point for going from local-only
// to connected-to-cloud state.
//
// Shown when the user clicks "Connect" in the StatusBar (when
// `authenticated === false`). Accepts a workspace key of the form
// `sery_k_…` generated from the web dashboard's Settings →
// Workspace keys page. Paste + click Connect → auth_with_key writes
// the token to the keyring, starts the tunnel, and the StatusBar
// flips to Online.
//
// Also exposes a visible path to the web dashboard for users who
// don't have a key yet — no silent signup, no hidden magic.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  CheckCircle2,
  ExternalLink,
  FolderOpen,
  Key,
  Loader2,
  X,
} from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';

interface CatchUpFolder {
  path: string;
  datasets: number;
  total_bytes: number;
  last_scan_at: string | null;
}

type Phase =
  | { kind: 'key' }
  | { kind: 'catch_up'; folders: CatchUpFolder[] }
  | { kind: 'syncing' };

type Props = {
  onClose: () => void;
  /**
   * Called after the workspace key is accepted. The caller typically
   * uses this to start the WebSocket tunnel + refresh any cached
   * state that depended on being authenticated.
   */
  onConnected?: (token: AgentToken) => void;
  /**
   * Pre-populated machine name. The user is rarely interested in
   * typing this — we default to the hostname-derived "My MacBook"
   * style string but let them override.
   */
  defaultDisplayName?: string;
  /**
   * Pre-populated workspace key. Set by the deep-link pairing flow
   * (`seryai://pair?key=...`) so users who clicked an invite link in
   * email/chat don't have to copy-paste. Still requires explicit
   * Connect-button click — we never auto-submit a deep-linked key.
   */
  defaultKey?: string;
};

export function ConnectModal({
  onClose,
  onConnected,
  defaultDisplayName,
  defaultKey,
}: Props) {
  const [key, setKey] = useState(defaultKey ?? '');
  const [displayName, setDisplayName] = useState(
    defaultDisplayName ?? defaultMachineName(),
  );
  const [submitting, setSubmitting] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: 'key' });
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const { setAgentInfo, setAuthenticated } = useAgentStore();
  const toast = useToast();

  const canSubmit =
    key.trim().startsWith('sery_k_') &&
    key.trim().length >= 16 &&
    displayName.trim().length > 0 &&
    !submitting;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    setErrorMsg(null);
    try {
      const token = await invoke<AgentToken>('auth_with_key', {
        key: key.trim(),
        displayName: displayName.trim(),
      });
      setAgentInfo(token);
      setAuthenticated(true);

      // Persist workspace_id / agent_id to config so offline-capable
      // paths (scanner, cache) don't need a round-trip later. The
      // Rust-side auth_with_key command already does this, but this
      // is a belt-and-suspenders follow-up in case the config write
      // raced with the setAgentInfo above.

      // Start the WebSocket tunnel so cloud queries work.
      invoke('start_websocket_tunnel').catch(err =>
        console.error('Tunnel failed to start after connect:', err),
      );

      toast.success('Connected. Your workspace is live.');
      onConnected?.(token);

      // Catch-up step: if the user added folders + scanned them in
      // local-only mode, the cloud has no record of them yet. Offer
      // to push that metadata up. Best-effort: a backend that's
      // somehow without this command falls back to closing as today.
      let catchUp: CatchUpFolder[] = [];
      try {
        catchUp = await invoke<CatchUpFolder[]>('list_catch_up_folders');
      } catch (e) {
        console.warn('list_catch_up_folders failed:', e);
      }
      if (catchUp.length > 0) {
        setSelectedPaths(new Set(catchUp.map(f => f.path)));
        setPhase({ kind: 'catch_up', folders: catchUp });
      } else {
        onClose();
      }
    } catch (err) {
      setErrorMsg(friendlyConnectError(err));
    } finally {
      setSubmitting(false);
    }
  };

  const togglePath = (path: string) => {
    setSelectedPaths(prev => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const handleSyncNow = async () => {
    if (phase.kind !== 'catch_up') return;
    const paths = Array.from(selectedPaths);
    if (paths.length === 0) {
      onClose();
      return;
    }
    setPhase({ kind: 'syncing' });
    // Fire-and-forget: catch_up_sync runs scans serially in the
    // background. The cloud-side scan_status pill is the primary
    // progress surface; closing the modal lets the user keep
    // working while it runs.
    invoke('catch_up_sync', { paths }).catch(err => {
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

  if (phase.kind === 'catch_up') {
    return (
      <div
        className="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
        onClick={onClose}
        role="presentation"
      >
        <div
          onClick={e => e.stopPropagation()}
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
                Connected. One more step?
              </h2>
              <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
                You have folders indexed locally. Share their metadata
                with your workspace so the assistant and other
                machines can see them.
              </p>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
              aria-label="Close"
            >
              <X className="h-5 w-5" />
            </button>
          </div>

          <ul className="mt-2 max-h-72 space-y-1 overflow-y-auto rounded-lg border border-slate-200 dark:border-slate-800">
            {phase.folders.map(f => {
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
              href="https://sery.ai/privacy"
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
              className="rounded-lg px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
            >
              Not now
            </button>
            <button
              type="button"
              onClick={handleSyncNow}
              disabled={selectedPaths.size === 0}
              className="rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:bg-slate-300 dark:disabled:bg-slate-700"
            >
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

  if (phase.kind === 'syncing') {
    return (
      <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/50">
        <div className="flex items-center gap-3 rounded-xl bg-white p-6 shadow-xl dark:bg-slate-900">
          <Loader2 className="h-5 w-5 animate-spin text-purple-600 dark:text-purple-400" />
          <span className="text-sm text-slate-700 dark:text-slate-200">
            Starting sync…
          </span>
        </div>
      </div>
    );
  }

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="presentation"
    >
      <form
        onSubmit={handleSubmit}
        onClick={e => e.stopPropagation()}
        className="w-full max-w-md rounded-xl bg-white p-6 shadow-xl dark:bg-slate-900"
        aria-labelledby="connect-modal-title"
      >
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2
              id="connect-modal-title"
              className="flex items-center gap-2 text-lg font-semibold text-slate-900 dark:text-slate-50"
            >
              <Key className="h-5 w-5 text-purple-600 dark:text-purple-400" />
              Connect to Sery.ai
            </h2>
            <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
              Paste a workspace key to enable cross-machine queries
              and the Machines view.
            </p>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {defaultKey && (
          <div className="mb-3 rounded-md border border-purple-200 bg-purple-50/60 p-3 text-xs text-purple-900 dark:border-purple-900/60 dark:bg-purple-950/30 dark:text-purple-200">
            This key arrived via an invite link. Confirm the machine
            name below and click <strong>Connect</strong>.
          </div>
        )}

        <label className="block">
          <span className="mb-1 block text-sm font-medium text-slate-700 dark:text-slate-300">
            Workspace key
          </span>
          <input
            type="text"
            value={key}
            onChange={e => setKey(e.target.value)}
            placeholder="sery_k_XXXXXXXXXXXXXXXXXXXX"
            autoFocus
            autoComplete="off"
            spellCheck={false}
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 font-mono text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-50"
          />
          <span className="mt-1 block text-xs text-slate-500 dark:text-slate-400">
            Starts with <code>sery_k_</code>. Generated on the web
            dashboard.
          </span>
        </label>

        <label className="mt-4 block">
          <span className="mb-1 block text-sm font-medium text-slate-700 dark:text-slate-300">
            Name this machine
          </span>
          <input
            type="text"
            value={displayName}
            onChange={e => setDisplayName(e.target.value)}
            placeholder="e.g. Home Desktop"
            maxLength={64}
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-50"
          />
        </label>

        <div className="mt-4 rounded-lg border border-slate-200 bg-slate-50 p-3 text-sm dark:border-slate-800 dark:bg-slate-800/50">
          <p className="text-slate-600 dark:text-slate-300">
            Don't have a workspace key yet?
          </p>
          <a
            href="https://app.sery.ai/settings/workspace-keys"
            target="_blank"
            rel="noopener noreferrer"
            className="mt-1 inline-flex items-center gap-1 text-sm font-medium text-purple-600 hover:underline dark:text-purple-400"
          >
            Open the dashboard to create one
            <ExternalLink className="h-3.5 w-3.5" />
          </a>
        </div>

        {errorMsg && (
          <div className="mt-4 rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            {errorMsg}
          </div>
        )}

        <div className="mt-6 flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="rounded-lg px-3 py-1.5 text-sm font-medium text-slate-600 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!canSubmit}
            className="rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:bg-slate-300 dark:disabled:bg-slate-700"
          >
            {submitting ? 'Connecting…' : 'Connect'}
          </button>
        </div>
      </form>
    </div>
  );
}

// ─── Helpers ──────────────────────────────────────────────────────────────

function detectPlatform(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'macOS';
  if (ua.includes('win')) return 'Windows';
  if (ua.includes('linux')) return 'Linux';
  return 'Unknown';
}

function defaultMachineName(): string {
  const platform = detectPlatform();
  return platform === 'Unknown' ? 'My Computer' : `My ${platform}`;
}

function basename(path: string): string {
  // Cross-platform — handles forward and backslash separators so we
  // don't import a path lib for one display string.
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

function friendlyConnectError(err: unknown): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (raw.includes('401') || lower.includes('invalid key') || lower.includes('unauthorized')) {
    return "That key isn't recognized. Double-check you copied the whole thing, including the sery_k_ prefix.";
  }
  if (raw.includes('403') || lower.includes('revoked')) {
    return 'That key has been revoked. Generate a fresh one in the dashboard.';
  }
  if (lower.includes('timed out') || lower.includes('timeout')) {
    return "Can't reach Sery.ai. Check your internet and try again.";
  }
  if (lower.includes('network') || lower.includes('connection')) {
    return "Can't reach Sery.ai. Check your internet and try again.";
  }
  if (raw.includes('429')) {
    return 'Too many attempts. Wait a minute and try again.';
  }
  return raw;
}
