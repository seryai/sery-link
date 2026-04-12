// Modal that pops up when the backend emits `auth_expired` — gives the
// user a one-click path back through the OAuth flow without losing their
// session state. Dismiss is explicit; we don't auto-close to make sure
// the user acknowledges that the agent is offline until they re-auth.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AlertTriangle, Loader2, LogIn, X } from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';

export function ReAuthModal() {
  const { reAuthRequired, setReAuthRequired, setAgentInfo, config } =
    useAgentStore();
  const toast = useToast();
  const [reconnecting, setReconnecting] = useState(false);

  if (!reAuthRequired) return null;

  const dismiss = () => setReAuthRequired(false);

  const reconnect = async () => {
    setReconnecting(true);
    try {
      const token = await invoke<AgentToken>('start_auth_flow', {
        agentName: config?.agent.name ?? 'My Computer',
        platform: config?.agent.platform ?? 'Unknown',
      });
      setAgentInfo(token);
      toast.success('Re-authenticated');
      setReAuthRequired(false);

      // Restart the tunnel now that we have a fresh token
      try {
        await invoke('start_websocket_tunnel');
      } catch (err) {
        console.error('Failed to restart tunnel:', err);
      }
    } catch (err) {
      toast.error(`Sign-in failed: ${err}`);
    } finally {
      setReconnecting(false);
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

          <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-amber-100 dark:bg-amber-900/40">
            <AlertTriangle className="h-6 w-6 text-amber-600 dark:text-amber-300" />
          </div>

          <h2 className="mb-2 text-xl font-bold text-slate-900 dark:text-slate-50">
            Your session has expired
          </h2>
          <p className="mb-6 text-sm text-slate-600 dark:text-slate-300">
            Sery couldn't validate this device. Queries from the cloud will
            be paused until you sign in again. Your data and settings are
            safe.
          </p>

          <div className="flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
            <button
              onClick={dismiss}
              disabled={reconnecting}
              className="rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              Later
            </button>
            <button
              onClick={reconnect}
              disabled={reconnecting}
              className="flex items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
            >
              {reconnecting ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Opening browser…
                </>
              ) : (
                <>
                  <LogIn className="h-4 w-4" />
                  Sign in again
                </>
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
