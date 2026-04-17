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

import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AddMachineModal } from './AddMachineModal';

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
  const [fleet, setFleet] = useState<FleetResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showAddModal, setShowAddModal] = useState(false);

  const fetchFleet = useCallback(async () => {
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
  }, [onFleetUpdated]);

  // Initial load + poll every 15s so online/offline transitions surface
  // without a manual refresh. 15s keeps Redis presence (30s TTL) visible.
  useEffect(() => {
    fetchFleet();
    const id = window.setInterval(fetchFleet, 15_000);
    return () => window.clearInterval(id);
  }, [fetchFleet]);

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
            Your Fleet
          </h2>
          <p className="text-sm text-slate-500 dark:text-slate-400">
            Every Sery machine connected to this workspace.
          </p>
        </div>
        {!hideAddButton && (
          <button
            onClick={() => setShowAddModal(true)}
            className="rounded-lg bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
          >
            + Add another machine
          </button>
        )}
      </div>

      {loading && !fleet && (
        <div className="rounded-lg border border-slate-200 bg-white p-6 text-center text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          Loading fleet…
        </div>
      )}

      {error && !loading && (
        <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          Couldn't load fleet. {error}{' '}
          <button className="underline" onClick={fetchFleet}>
            Retry
          </button>
        </div>
      )}

      {fleet && fleet.agents.length === 0 && !loading && (
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

      {fleet && fleet.agents.length > 0 && (
        <ul className="divide-y divide-slate-200 overflow-hidden rounded-lg border border-slate-200 bg-white dark:divide-slate-800 dark:border-slate-800 dark:bg-slate-900">
          {fleet.agents.map(agent => (
            <li key={agent.agent_id}>
              <AgentRow agent={agent} />
            </li>
          ))}
        </ul>
      )}

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

// ─── Row ───────────────────────────────────────────────────────────────────

function AgentRow({ agent }: { agent: FleetAgent }) {
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
