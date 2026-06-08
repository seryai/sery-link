// Connect modal — the single entry point for going from local-only
// to connected-to-cloud state.
//
// Shown when the user clicks "Connect" in the StatusBar (when
// `authenticated === false`). Accepts EITHER credential shape:
//
//   * Workspace key — `sery_k_…`. Long-lived. Settings → Workspace
//     Keys on the dashboard. Multiple machines can be added with the
//     same key over time.
//   * Mesh invitation code — 10-char Crockford base32 (no I/L/O/U).
//     Single-use, expirable. Settings → Machine Invitations on the
//     dashboard. Auto-detected by format so the user doesn't need to
//     pick a tab.
//
// Paste + Connect → either auth_with_key or auth_with_invitation
// runs on the Rust side, writes the token to the keyring, starts
// the tunnel, and the StatusBar flips to Online.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';
import { ExternalLink, Key, X } from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';
import { CatchUpDialog, type CatchUpFolder } from './CatchUpDialog';

type Phase =
  | { kind: 'key' }
  | { kind: 'catch_up'; folders: CatchUpFolder[] };

type CredentialKind = 'workspace_key' | 'invitation' | 'unknown';

// Crockford base32 — excludes I, L, O, U to avoid look-alikes. Must
// stay in sync with `_CODE_ALPHABET` in api/app/services/mesh/__init__.py.
const INVITATION_CODE_LENGTH = 10;
const INVITATION_ALPHABET = /^[ABCDEFGHJKMNPQRSTVWXYZ23456789]+$/;

function classify(input: string): CredentialKind {
  const trimmed = input.trim();
  if (trimmed.startsWith('sery_k_') && trimmed.length >= 16) {
    return 'workspace_key';
  }
  const upper = trimmed.toUpperCase();
  if (upper.length === INVITATION_CODE_LENGTH && INVITATION_ALPHABET.test(upper)) {
    return 'invitation';
  }
  return 'unknown';
}

type Props = {
  onClose: () => void;
  /**
   * Called after the credential is accepted. The caller typically
   * uses this to start the WebSocket tunnel + refresh any cached
   * state that depended on being authenticated.
   */
  onConnected?: (token: AgentToken) => void;
  /**
   * Pre-populated workspace key. Set by the deep-link pairing flow
   * (`seryai://pair?key=...`) so users who clicked an invite link in
   * email/chat don't have to copy-paste. Still requires explicit
   * Connect-button click — we never auto-submit a deep-linked key.
   */
  defaultKey?: string;
  /**
   * Pre-populated mesh invitation code. Set by the
   * `seryai://join?code=...` deep link from the dashboard's
   * Machine Invitations panel. Same explicit-confirmation rule as
   * defaultKey — paste arrived; user still chooses to click Connect.
   */
  defaultCode?: string;
};

export function ConnectModal({
  onClose,
  onConnected,
  defaultKey,
  defaultCode,
}: Props) {
  const [value, setValue] = useState(defaultKey ?? defaultCode ?? '');
  const [submitting, setSubmitting] = useState(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: 'key' });
  const { setAgentInfo, setAuthenticated, config } = useAgentStore();
  const toast = useToast();

  const kind = classify(value);
  const canSubmit = kind !== 'unknown' && !submitting;
  const arrivedFromDeepLink = Boolean(defaultKey || defaultCode);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!canSubmit) return;
    setSubmitting(true);
    setErrorMsg(null);
    try {
      const machineName = config?.agent?.name?.trim() || defaultMachineName();
      let token: AgentToken;
      if (kind === 'workspace_key') {
        token = await invoke<AgentToken>('auth_with_key', {
          key: value.trim(),
          displayName: machineName,
        });
      } else {
        // Invitation codes get upper-cased to tolerate user-typed
        // lowercase — the server alphabet is upper-only Crockford.
        token = await invoke<AgentToken>('auth_with_invitation', {
          code: value.trim().toUpperCase(),
          displayName: machineName,
        });
      }
      setAgentInfo(token);
      setAuthenticated(true);

      // Persist workspace_id / agent_id to config so offline-capable
      // paths (scanner, cache) don't need a round-trip later. The
      // Rust-side auth commands already do this, but this is a
      // belt-and-suspenders follow-up in case the config write
      // raced with the setAgentInfo above.

      // Start the WebSocket tunnel so cloud queries work.
      invoke('start_websocket_tunnel').catch(err =>
        console.error('Tunnel failed to start after connect:', err),
      );

      toast.success('This device is now linked to your Sery account.');
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
        setPhase({ kind: 'catch_up', folders: catchUp });
      } else {
        onClose();
      }
    } catch (err) {
      setErrorMsg(friendlyConnectError(err, kind));
    } finally {
      setSubmitting(false);
    }
  };

  if (phase.kind === 'catch_up') {
    return <CatchUpDialog folders={phase.folders} onClose={onClose} />;
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
              Link this device to your Sery account so it can see your
              other machines.
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

        {arrivedFromDeepLink && (
          <div className="mb-3 rounded-md border border-purple-200 bg-purple-50/60 p-3 text-xs text-purple-900 dark:border-purple-900/60 dark:bg-purple-950/30 dark:text-purple-200">
            This code arrived via a link. Click <strong>Connect</strong> to link this device.
          </div>
        )}

        <label className="block">
          <span className="mb-1 block text-sm font-medium text-slate-700 dark:text-slate-300">
            Paste your connect code
          </span>
          <input
            type="text"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            value={value}
            onChange={e => setValue(e.target.value)}
            placeholder="Paste from app.sery.ai"
            autoFocus
            autoComplete="off"
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 font-mono text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-50"
          />
        </label>


        <div className="mt-4 flex justify-center">
          <button
            type="button"
            onClick={() => openUrl('https://app.sery.ai/connect')}
            className="inline-flex items-center gap-1 text-sm font-medium text-purple-600 hover:underline dark:text-purple-400"
          >
            Get a code from Sery.ai
            <ExternalLink className="h-3.5 w-3.5" />
          </button>
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

function friendlyConnectError(err: unknown, _kind: CredentialKind): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (raw.includes('401') || lower.includes('invalid') || lower.includes('unauthorized') || lower.includes('expired') || lower.includes('not found')) {
    return "That code didn't work — it may have expired, been used already, or been mistyped. Grab a fresh one from app.sery.ai.";
  }
  if (raw.includes('403') || lower.includes('revoked')) {
    return 'That code has been revoked. Generate a fresh one at app.sery.ai.';
  }
  if (raw.includes('409') || lower.includes('already')) {
    return 'This code has already been used. Get a new one at app.sery.ai.';
  }
  if (lower.includes('cap') || lower.includes('limit')) {
    return 'Your account is at its device limit. Free up a slot or upgrade.';
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
