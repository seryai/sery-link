// Modal raised when the backend emits `workspace_key_revoked` — the stored
// key was revoked server-side. Shows a key input so the user can paste a new
// key without opening a browser (workspace-key machines have no Sery account).

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { KeyRound, Loader2, X } from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';

export function WorkspaceKeyRevokedModal() {
  const { workspaceKeyRevoked, setWorkspaceKeyRevoked, setAgentInfo, config } =
    useAgentStore();
  const toast = useToast();
  const [key, setKey] = useState('');
  const [connecting, setConnecting] = useState(false);

  if (!workspaceKeyRevoked) return null;

  const dismiss = () => {
    setWorkspaceKeyRevoked(false);
    setKey('');
  };

  const connect = async () => {
    const trimmed = key.trim();
    if (!trimmed) return;
    setConnecting(true);
    try {
      const token = await invoke<AgentToken>('auth_with_key', {
        key: trimmed,
        displayName: config?.agent.name ?? 'My Computer',
      });
      setAgentInfo(token);
      setWorkspaceKeyRevoked(false);
      setKey('');
      toast.success('Connected with new workspace key');
      try {
        await invoke('start_websocket_tunnel');
      } catch (err) {
        console.error('Failed to restart tunnel:', err);
      }
    } catch (err) {
      toast.error(`Connection failed: ${err}`);
    } finally {
      setConnecting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-slate-900/60 p-4 backdrop-blur-sm"
      role="dialog"
      aria-modal="true"
    >
      <div className="w-full max-w-md animate-slide_up overflow-hidden rounded-2xl bg-white shadow-2xl dark:bg-slate-900">
        <div className="relative p-6">
          <button
            onClick={dismiss}
            className="absolute right-4 top-4 rounded-md p-1 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800 dark:hover:text-slate-200"
            aria-label="Dismiss"
          >
            <X className="h-5 w-5" />
          </button>

          <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-purple-100 dark:bg-purple-900/40">
            <KeyRound className="h-6 w-6 text-purple-600 dark:text-purple-300" />
          </div>

          <h2 className="mb-2 text-xl font-bold text-slate-900 dark:text-slate-50">
            Workspace key revoked
          </h2>
          <p className="mb-5 text-sm text-slate-600 dark:text-slate-300">
            Your workspace key was revoked. Ask your workspace admin for a new
            key and paste it below to reconnect.
          </p>

          <input
            type="password"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && !connecting && connect()}
            placeholder="sery_k_…"
            className="mb-4 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 font-mono text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-50 dark:placeholder-slate-500"
            autoFocus
          />

          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <button
              onClick={dismiss}
              disabled={connecting}
              className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              Later
            </button>
            <button
              onClick={connect}
              disabled={connecting || !key.trim()}
              className="flex items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
            >
              {connecting ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Connecting…
                </>
              ) : (
                <>
                  <KeyRound className="h-4 w-4" />
                  Connect
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
