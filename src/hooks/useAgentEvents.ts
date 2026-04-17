// Central Tauri event listener. Mounted once at the app root so the store
// always reflects the latest state the Rust backend has emitted.
//
// Every Rust emit_* call in src-tauri/src/events.rs has a matching handler
// here. The event name constants are imported from types/events.ts so
// they stay in lock-step with the backend.

import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from '../components/Toast';
import {
  EVENT_NAMES,
  type AgentConfig,
  type AgentStats,
  type QueryCompleted,
  type QueryHistoryEntry,
  type QueryStarted,
  type ScanComplete,
  type ScanProgress,
  type SchemaChangedPayload,
  type SyncCompletedPayload,
  type SyncFailedPayload,
  type WsStatus,
} from '../types/events';

export function useAgentEvents() {
  const {
    setConnectionStatus,
    setStats,
    setConfig,
    applyScanProgress,
    clearScanProgress,
    prependHistory,
    setHistory,
    setReAuthRequired,
    addSchemaNotification,
  } = useAgentStore();
  const toast = useToast();

  useEffect(() => {
    const unsubs: Array<Promise<UnlistenFn>> = [];

    // Connection status
    unsubs.push(
      listen<WsStatus>(EVENT_NAMES.WS_STATUS, (event) => {
        setConnectionStatus(event.payload.status, event.payload.detail);
      }),
    );

    // Scan progress (streamed while a folder sync runs)
    unsubs.push(
      listen<ScanProgress>(EVENT_NAMES.SCAN_PROGRESS, (event) => {
        applyScanProgress(event.payload);
      }),
    );

    // Scan completion (clear the progress row)
    unsubs.push(
      listen<ScanComplete>(EVENT_NAMES.SCAN_COMPLETE, (event) => {
        clearScanProgress(event.payload.folder);
      }),
    );

    // Schema-change notification — fires once per dataset whose shape
    // drifted between scans. Push into the store (so the Notifications
    // view + Fleet badge update) regardless; the toast is gated by
    // the user's setting so scan-heavy users can silence it without
    // losing the persisted record.
    unsubs.push(
      listen<SchemaChangedPayload>(EVENT_NAMES.SCHEMA_CHANGED, (event) => {
        addSchemaNotification(event.payload);
        const toastsEnabled =
          useAgentStore.getState().config?.app?.schema_change_toasts_enabled ?? true;
        if (!toastsEnabled) return;
        const { dataset_name, added, removed, type_changed } = event.payload;
        const parts: string[] = [];
        if (added > 0) parts.push(`${added} added`);
        if (removed > 0) parts.push(`${removed} removed`);
        if (type_changed > 0) parts.push(`${type_changed} type changed`);
        toast.info(
          `Schema changed: ${dataset_name} (${parts.join(', ') || 'no detail'})`,
        );
      }),
    );

    // Query lifecycle — we don't store "started" in the list yet, only log it
    unsubs.push(
      listen<QueryStarted>(EVENT_NAMES.QUERY_STARTED, (event) => {
        // Placeholder row so the history UI shows "running" immediately
        const entry: QueryHistoryEntry = {
          query_id: event.payload.query_id,
          timestamp: new Date().toISOString(),
          file_path: event.payload.file_path,
          sql: '',
          status: 'success',
          row_count: null,
          duration_ms: 0,
          error: null,
        };
        prependHistory(entry);
      }),
    );

    unsubs.push(
      listen<QueryCompleted>(EVENT_NAMES.QUERY_COMPLETED, (event) => {
        // Upsert — replace the placeholder for this query_id if present
        const current = useAgentStore.getState().history;
        const idx = current.findIndex((h) => h.query_id === event.payload.query_id);
        if (idx >= 0) {
          const next = [...current];
          next[idx] = {
            ...next[idx],
            status: event.payload.status,
            row_count: event.payload.row_count,
            duration_ms: event.payload.duration_ms,
            error: event.payload.error,
          };
          setHistory(next);
        }
      }),
    );

    // History refresh (backend sends this when it persists a row)
    unsubs.push(
      listen(EVENT_NAMES.HISTORY_UPDATED, async () => {
        try {
          const entries = await invoke<QueryHistoryEntry[]>('get_query_history');
          setHistory(entries);
        } catch (err) {
          console.error('Failed to refresh history:', err);
        }
      }),
    );

    // Stats refresh
    unsubs.push(
      listen<AgentStats>(EVENT_NAMES.STATS_UPDATED, (event) => {
        setStats(event.payload);
      }),
    );

    // Auth expiration — raise the re-auth modal
    unsubs.push(
      listen(EVENT_NAMES.AUTH_EXPIRED, () => {
        setReAuthRequired(true);
      }),
    );

    // Sync results — toast + audit refresh + config reload
    unsubs.push(
      listen<SyncCompletedPayload>(EVENT_NAMES.SYNC_COMPLETED, (event) => {
        toast.success(
          `Synced ${event.payload.datasets} dataset${event.payload.datasets === 1 ? '' : 's'} from ${folderLabel(event.payload.folder)}`,
        );
        refreshAudit();
        refreshConfig(setConfig);
      }),
    );

    unsubs.push(
      listen<SyncFailedPayload>(EVENT_NAMES.SYNC_FAILED, (event) => {
        toast.error(`Sync failed: ${event.payload.error}`);
        refreshAudit();
        refreshConfig(setConfig);
      }),
    );

    return () => {
      // Fire-and-forget — unlisten returns a promise but we don't need to await
      unsubs.forEach((p) => p.then((fn) => fn()).catch(() => undefined));
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
}

function folderLabel(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

async function refreshAudit() {
  try {
    const audit = await invoke<unknown[]>('get_sync_audit');
    useAgentStore.getState().setAudit(audit as never);
  } catch (err) {
    console.error('Failed to refresh audit:', err);
  }
}

async function refreshConfig(setConfig: (config: AgentConfig) => void) {
  try {
    const config = await invoke<AgentConfig>('get_config');
    setConfig(config);
  } catch (err) {
    console.error('Failed to refresh config:', err);
  }
}
