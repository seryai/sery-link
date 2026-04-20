// Machines view — "My Machines" list.
//
// Calls the list_machines Tauri command (wraps GET
// /v1/agent/workspace/fleet on the backend — the URL name is kept for
// continuity with the HTTP contract). Renders one row per machine with
// live online status, dataset count, and storage used.
//
// Adding a new machine: users generate a workspace key on the web
// dashboard (Settings → Workspace keys) and paste it into Sery Link's
// ConnectModal on the new machine.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Bell, CloudOff, Laptop, Link as LinkIcon } from 'lucide-react';
import { Link } from 'react-router-dom';
import { useAgentStore } from '../stores/agentStore';
import { ConnectModal } from './ConnectModal';

type Machine = {
  agent_id: string;
  display_name: string | null;
  name: string;
  hostname: string | null;
  os_type: string | null;
  status: 'online' | 'offline' | 'error';
  last_seen_at: string | null;
  dataset_count: number;
  total_bytes: number;
  is_current_user: boolean;
};

type MachinesResponse = {
  workspace_id: string;
  agents: Machine[];
  total: number;
};

interface Props {
  /** Hide the "Add another machine" button (e.g. inside an onboarding flow). */
  hideAddButton?: boolean;
  /** Called with each refreshed machines snapshot so parents can react (badge counts, etc.). */
  onMachinesUpdated?: (machines: MachinesResponse) => void;
}

export function MachinesView({ hideAddButton, onMachinesUpdated }: Props) {
  const authenticated = useAgentStore((s) => s.authenticated);
  const [machines, setMachines] = useState<MachinesResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showConnectModal, setShowConnectModal] = useState(false);

  const fetchMachines = useCallback(async () => {
    if (!authenticated) {
      setMachines(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const resp = await invoke<MachinesResponse>('list_machines');
      setMachines(resp);
      onMachinesUpdated?.(resp);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [authenticated, onMachinesUpdated]);

  // Initial load + poll every 15s so online/offline transitions surface
  // without a manual refresh. 15s keeps Redis presence (30s TTL) visible.
  // Polling is skipped while unauthenticated — no point hitting an
  // endpoint that requires a token.
  useEffect(() => {
    fetchMachines();
    if (!authenticated) return;
    const id = window.setInterval(fetchMachines, 15_000);
    return () => window.clearInterval(id);
  }, [authenticated, fetchMachines]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <Laptop className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Your Machines
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Every Sery machine connected to this workspace.
            </p>
          </div>
          {authenticated && !hideAddButton && (
            <a
              href="https://app.sery.ai/settings/workspace-keys"
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1.5 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
              title="Open the dashboard to create a workspace key. Paste it into Sery Link on your other machines to pair them."
            >
              + Add another machine
            </a>
          )}
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6 space-y-4">

      {!authenticated && (
        <div className="flex h-full flex-col items-center justify-center gap-4 rounded-lg border-2 border-dashed border-slate-300 p-12 text-center dark:border-slate-700">
          <CloudOff className="h-10 w-10 text-slate-400 dark:text-slate-600" />
          <div>
            <h2 className="text-base font-semibold text-slate-900 dark:text-slate-50">
              Connect to see your machines
            </h2>
            <p className="mt-1 max-w-md text-sm text-slate-500 dark:text-slate-400">
              Sery is running locally on this machine. To pair it with
              your other machines and query across them, connect to
              Sery.ai with a workspace key.
            </p>
          </div>
          <button
            onClick={() => setShowConnectModal(true)}
            className="inline-flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
          >
            <LinkIcon className="h-4 w-4" />
            Connect to Sery.ai
          </button>
          {showConnectModal && (
            <ConnectModal onClose={() => setShowConnectModal(false)} />
          )}
        </div>
      )}

      {authenticated && loading && !machines && (
        <div className="rounded-lg border border-slate-200 bg-white p-6 text-center text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          Loading machines…
        </div>
      )}

      {authenticated && error && !loading && (
        <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          Couldn't load machines. {error}{' '}
          <button className="underline" onClick={fetchMachines}>
            Retry
          </button>
        </div>
      )}

      {authenticated && machines && machines.agents.length === 0 && !loading && (
        <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Only this machine is connected so far.
          </p>
          <p className="mt-2 max-w-md mx-auto text-xs text-slate-500 dark:text-slate-400">
            To pair another machine, generate a workspace key on the
            dashboard and paste it into Sery Link on the new machine.
          </p>
          {!hideAddButton && (
            <a
              href="https://app.sery.ai/settings/workspace-keys"
              target="_blank"
              rel="noopener noreferrer"
              className="mt-4 inline-block rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Open dashboard → create key
            </a>
          )}
        </div>
      )}

      {authenticated && machines && machines.agents.length > 0 && (
        <MachinesList agents={machines.agents} />
      )}
      </div>
    </div>
  );
}

// ─── List + Rows ───────────────────────────────────────────────────────────

function MachinesList({ agents }: { agents: Machine[] }) {
  // Derive per-machine unread schema-change counts once, outside the row
  // components — avoids each row subscribing independently to the store.
  const notifications = useAgentStore((s) => s.schemaNotifications);
  const unreadByAgent = useMemo(() => {
    const map = new Map<string, number>();
    for (const n of notifications) {
      if (n.read || !n.origin_agent_id) continue;
      map.set(n.origin_agent_id, (map.get(n.origin_agent_id) ?? 0) + 1);
    }
    return map;
  }, [notifications]);

  return (
    <ul className="divide-y divide-slate-200 overflow-hidden rounded-lg border border-slate-200 bg-white dark:divide-slate-800 dark:border-slate-800 dark:bg-slate-900">
      {agents.map((agent) => (
        <li key={agent.agent_id}>
          <MachineRow
            agent={agent}
            unreadSchemaChanges={unreadByAgent.get(agent.agent_id) ?? 0}
          />
        </li>
      ))}
    </ul>
  );
}

function MachineRow({
  agent,
  unreadSchemaChanges,
}: {
  agent: Machine;
  unreadSchemaChanges: number;
}) {
  const niceName = agent.display_name ?? agent.name;
  const sub = [
    agent.os_type,
    agent.hostname && agent.hostname !== niceName ? agent.hostname : null,
  ]
    .filter(Boolean)
    .join(' · ');

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <StatusDot status={agent.status} />
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
            {niceName}
          </span>
          {agent.is_current_user && (
            <span className="rounded bg-purple-100 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-purple-700 dark:bg-purple-900/40 dark:text-purple-300">
              This machine
            </span>
          )}
          {unreadSchemaChanges > 0 && (
            <Link
              to="/notifications"
              title={`${unreadSchemaChanges} unread schema change${
                unreadSchemaChanges === 1 ? '' : 's'
              } on this machine`}
              className="inline-flex items-center gap-1 rounded-full bg-purple-600/90 px-1.5 py-0.5 text-[10px] font-semibold text-white transition-colors hover:bg-purple-700"
            >
              <Bell className="h-2.5 w-2.5" />
              {unreadSchemaChanges > 99 ? '99+' : unreadSchemaChanges}
            </Link>
          )}
        </div>
        <div className="truncate text-xs text-slate-500 dark:text-slate-400">
          {sub || '—'}
        </div>
      </div>
      <div className="text-right text-xs text-slate-500 dark:text-slate-400">
        <div>
          {agent.dataset_count} {agent.dataset_count === 1 ? 'file' : 'files'}
        </div>
        <div>{formatBytes(agent.total_bytes)}</div>
      </div>
    </div>
  );
}

function StatusDot({ status }: { status: Machine['status'] }) {
  const color =
    status === 'online'
      ? 'bg-emerald-500'
      : status === 'error'
        ? 'bg-rose-500'
        : 'bg-slate-300 dark:bg-slate-600';
  return (
    <span
      className={`inline-block h-2.5 w-2.5 shrink-0 rounded-full ${color}`}
      title={status}
      aria-label={status}
    />
  );
}

// ─── Helpers ───────────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (!bytes || bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}
