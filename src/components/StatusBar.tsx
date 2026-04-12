// Top status strip. Shows current connection state with a colored dot and
// subtle text. Hovering the status reveals the full detail message (error
// text when the connection fails).

import { useAgentStore } from '../stores/agentStore';
import type { ConnectionStatus } from '../stores/agentStore';

const STATUS: Record<
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
    label: 'Offline',
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
  const { connectionStatus, connectionDetail, agentInfo, stats } =
    useAgentStore();

  const meta = STATUS[connectionStatus];
  const detail = connectionDetail ?? meta.label;

  return (
    <div
      className={`flex items-center justify-between border-b border-slate-200 px-4 py-2 text-xs dark:border-slate-800 ${meta.bg}`}
    >
      <div className="flex items-center gap-2" title={detail}>
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
      </div>
    </div>
  );
}
