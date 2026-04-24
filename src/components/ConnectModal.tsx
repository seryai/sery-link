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
import { ExternalLink, Key, X } from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';

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
};

export function ConnectModal({
  onClose,
  onConnected,
  defaultDisplayName,
}: Props) {
  const [key, setKey] = useState('');
  const [displayName, setDisplayName] = useState(
    defaultDisplayName ?? defaultMachineName(),
  );
  const [submitting, setSubmitting] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
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
      onClose();
    } catch (err) {
      setErrorMsg(friendlyConnectError(err));
    } finally {
      setSubmitting(false);
    }
  };

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
