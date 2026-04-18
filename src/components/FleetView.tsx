// Fleet view — "My Devices" list.
//
// Calls the list_fleet Tauri command (wraps GET /v1/agent/workspace/fleet
// on the backend). Renders one row per agent with live online status,
// dataset count, storage used, and an "Add another machine" entry point
// that opens <AddMachineModal>.
//
// Self-contained: no router changes, no parent-component dependencies.
// Parent decides where to render it (settings tab, dedicated route, etc.).
//
// Paired backend: api/app/api/v1/fleet.py.

import { useCallback, useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Bell, CloudOff, Laptop, Link as LinkIcon } from 'lucide-react';
import { Link } from 'react-router-dom';
import { useAgentStore } from '../stores/agentStore';
import { AddMachineModal } from './AddMachineModal';
import { ConnectModal } from './ConnectModal';

type FleetAgent = {
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

type FleetResponse = {
  workspace_id: string;
  agents: FleetAgent[];
  total: number;
};

interface Props {
  /** Hide the "Add another machine" button (e.g. inside an onboarding flow). */
  hideAddButton?: boolean;
  /** Called with each refreshed fleet snapshot so parents can react (badge counts, etc.). */
  onFleetUpdated?: (fleet: FleetResponse) => void;
}

export function FleetView({ hideAddButton, onFleetUpdated }: Props) {
  const authenticated = useAgentStore((s) => s.authenticated);
  const [fleet, setFleet] = useState<FleetResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showConnectModal, setShowConnectModal] = useState(false);

  const fetchFleet = useCallback(async () => {
    if (!authenticated) {
      setFleet(null);
      setLoading(false);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const resp = await invoke<FleetResponse>('list_fleet');
      setFleet(resp);
      onFleetUpdated?.(resp);
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [authenticated, onFleetUpdated]);

  // Initial load + poll every 15s so online/offline transitions surface
  // without a manual refresh. 15s keeps Redis presence (30s TTL) visible.
  // Polling is skipped while unauthenticated — no point hitting an
  // endpoint that requires a token.
  useEffect(() => {
    fetchFleet();
    if (!authenticated) return;
    const id = window.setInterval(fetchFleet, 15_000);
    return () => window.clearInterval(id);
  }, [authenticated, fetchFleet]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <Laptop className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Your Devices
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Every Sery machine connected to this workspace.
            </p>
          </div>
          {authenticated && !hideAddButton && (
            <button
              onClick={() => setShowAddModal(true)}
              className="rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              + Add another machine
            </button>
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
              Connect to see your devices
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

      {authenticated && loading && !fleet && (
        <div className="rounded-lg border border-slate-200 bg-white p-6 text-center text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          Loading fleet…
        </div>
      )}

      {authenticated && error && !loading && (
        <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          Couldn't load fleet. {error}{' '}
          <button className="underline" onClick={fetchFleet}>
            Retry
          </button>
        </div>
      )}

      {authenticated && fleet && fleet.agents.length === 0 && !loading && (
        <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
          <p className="text-sm text-slate-600 dark:text-slate-400">
            No machines yet. Your workspace is empty.
          </p>
          {!hideAddButton && (
            <button
              onClick={() => setShowAddModal(true)}
              className="mt-4 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Add your first machine
            </button>
          )}
        </div>
      )}

      {authenticated && fleet && fleet.agents.length > 0 && (
        <FleetList agents={fleet.agents} />
      )}
      </div>

      {showAddModal && (
        <AddMachineModal
          onClose={() => setShowAddModal(false)}
          onPaired={() => {
            // A new machine just joined — refresh the list so the user
            // sees it appear immediately, not on the next 15s tick.
            fetchFleet();
          }}
        />
      )}
    </div>
  );
}

// ─── List + Rows ───────────────────────────────────────────────────────────

function FleetList({ agents }: { agents: FleetAgent[] }) {
  // Derive per-agent unread schema-change counts once, outside the row
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
          <AgentRow
            agent={agent}
            unreadSchemaChanges={unreadByAgent.get(agent.agent_id) ?? 0}
          />
        </li>
      ))}
    </ul>
  );
}

function AgentRow({
  agent,
  unreadSchemaChanges,
}: {
  agent: FleetAgent;
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

function StatusDot({ status }: { status: FleetAgent['status'] }) {
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
