// Top status strip. Shows whether Sery Link is in local-only mode or
// connected to Sery.ai Cloud, with a Connect button when local-only.
//
// Two mental states:
//   * !authenticated  → LOCAL-ONLY.  Pill is gray with "Local only"
//     label; button on the right says "Connect" and opens
//     ConnectModal.
//   * authenticated   → CONNECTED.  Pill reflects the WebSocket
//     tunnel state (Connected / Connecting / Error).
//
// Cross-cutting: stats (queries today, agent id short) are shown on
// the right side in both modes — they're populated regardless of
// cloud state.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Cloud, CloudOff, Link as LinkIcon, LogOut } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import type { ConnectionStatus } from '../stores/agentStore';
import { ConnectModal } from './ConnectModal';
import { useToast } from './Toast';

const CONNECTED_STATUS: Record<
  ConnectionStatus,
  { dot: string; ring: string; label: string; bg: string }
> = {
  online: {
    dot: 'bg-emerald-500',
    ring: 'ring-emerald-500/30',
    label: 'Connected',
    bg: 'bg-white dark:bg-slate-900',
  },
  connecting: {
    dot: 'bg-amber-500 animate-pulse',
    ring: 'ring-amber-500/30',
    label: 'Connecting…',
    bg: 'bg-white dark:bg-slate-900',
  },
  offline: {
    dot: 'bg-slate-400',
    ring: 'ring-slate-400/30',
    label: 'Reconnecting…',
    bg: 'bg-white dark:bg-slate-900',
  },
  error: {
    dot: 'bg-rose-500',
    ring: 'ring-rose-500/30',
    label: 'Connection error',
    bg: 'bg-rose-50 dark:bg-rose-950/30',
  },
};

export function StatusBar() {
  const {
    authenticated,
    connectionStatus,
    connectionDetail,
    agentInfo,
    stats,
  } = useAgentStore();
  const toast = useToast();
  const [showConnect, setShowConnect] = useState(false);
  // Set when a `seryai://pair?key=...` deep link fires while the user
  // is unauthenticated. Pre-fills ConnectModal so the user sees the
  // key from the invite without copy-pasting. Cleared when the modal
  // closes — never persisted (the modal is the only legitimate
  // consumer of an invite key).
  const [deepLinkKey, setDeepLinkKey] = useState<string | null>(null);

  // Bridge the seryai://pair?key=… URL scheme into the existing
  // workspace-key auth flow. The Rust side (deep_link.rs) already
  // emits `deep-link-pair` with the raw key as the payload; here we
  // open the right modal on receipt, or — when already authenticated
  // — surface a toast explaining how to switch.
  useEffect(() => {
    const unlisten = listen<string>('deep-link-pair', (event) => {
      const key = event.payload;
      if (!key || !key.startsWith('sery_k_')) {
        toast.error("Invite link's key didn't look right. Ask the sender to resend.");
        return;
      }
      if (authenticated) {
        toast.info(
          "You're already connected to a workspace. Disconnect first if you want to switch.",
        );
        return;
      }
      setDeepLinkKey(key);
      setShowConnect(true);
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [authenticated, toast]);

  function closeConnect() {
    setShowConnect(false);
    setDeepLinkKey(null);
  }

  // Two shells — local-only vs connected. Keeping the branch at the
  // top level makes each path obvious when reading the file.
  if (!authenticated) {
    return (
      <>
        <div className="flex items-center justify-between border-b border-slate-200 bg-slate-50 px-4 py-2 text-xs dark:border-slate-800 dark:bg-slate-950/40">
          <div className="flex items-center gap-2" title="Sery is running fully on this machine. Nothing has been uploaded.">
            <CloudOff className="h-3.5 w-3.5 text-slate-500 dark:text-slate-400" />
            <span className="font-medium text-slate-700 dark:text-slate-200">
              Local only
            </span>
            <span className="ml-1 hidden text-slate-500 dark:text-slate-400 sm:inline">
              · Nothing has been uploaded
            </span>
          </div>

          <div className="flex items-center gap-3">
            {stats && (
              <span className="text-slate-500 dark:text-slate-400">
                {stats.queries_today} {stats.queries_today === 1 ? 'query' : 'queries'} today
              </span>
            )}
            <button
              onClick={() => setShowConnect(true)}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-2.5 py-1 text-xs font-semibold text-white transition-colors hover:bg-purple-700"
            >
              <LinkIcon className="h-3 w-3" />
              Connect
            </button>
          </div>
        </div>

        {showConnect && (
          <ConnectModal
            onClose={closeConnect}
            defaultKey={deepLinkKey ?? undefined}
          />
        )}
      </>
    );
  }

  const meta = CONNECTED_STATUS[connectionStatus];
  const detail = connectionDetail ?? meta.label;

  return (
    <div
      className={`flex items-center justify-between border-b border-slate-200 px-4 py-2 text-xs dark:border-slate-800 ${meta.bg}`}
    >
      <div className="flex items-center gap-2" title={detail}>
        <Cloud className="h-3.5 w-3.5 text-emerald-600 dark:text-emerald-400" />
        <span className={`relative flex h-2 w-2`}>
          <span
            className={`absolute inline-flex h-full w-full rounded-full opacity-75 ${meta.dot}`}
          />
          <span
            className={`relative inline-flex h-2 w-2 rounded-full ring-2 ${meta.dot} ${meta.ring}`}
          />
        </span>
        <span className="font-medium text-slate-700 dark:text-slate-200">
          {meta.label}
        </span>
        {connectionStatus === 'error' && connectionDetail && (
          <span className="ml-1 truncate text-rose-600 dark:text-rose-300">
            {connectionDetail}
          </span>
        )}
      </div>

      <div className="flex items-center gap-3 text-slate-500 dark:text-slate-400">
        {stats && (
          <span>
            {stats.queries_today} {stats.queries_today === 1 ? 'query' : 'queries'}{' '}
            today
          </span>
        )}
        {agentInfo && (
          <span
            className="truncate font-mono text-[10px]"
            title={agentInfo.agent_id}
          >
            {agentInfo.agent_id.slice(0, 8)}
          </span>
        )}
        <DisconnectButton />
      </div>
    </div>
  );
}

/**
 * Small disconnect control. Shown when connected — clears keyring,
 * stops the tunnel, drops the user back to local-only mode. Requires
 * confirm because accidentally hitting it would be annoying.
 */
function DisconnectButton() {
  const { setAuthenticated, setAgentInfo } = useAgentStore();
  const toast = useToast();

  const handle = async () => {
    const ok = window.confirm(
      'Disconnect from Sery.ai? Sery will keep running locally, but cross-machine queries, the Machines view, and schema-change sync will stop until you reconnect.',
    );
    if (!ok) return;
    try {
      await invoke('logout');
      setAgentInfo(null);
      setAuthenticated(false);
      toast.success('Disconnected. Sery is running locally.');
    } catch (err) {
      console.error('Disconnect failed:', err);
      toast.error(`Couldn't disconnect: ${String(err)}`);
    }
  };

  return (
    <button
      onClick={handle}
      title="Disconnect from Sery.ai (stay local-only)"
      className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
      aria-label="Disconnect"
    >
      <LogOut className="h-3.5 w-3.5" />
    </button>
  );
}
