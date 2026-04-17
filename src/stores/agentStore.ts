// Global agent state.
//
// The store mirrors everything the backend knows that the UI cares about:
// connection status, config, stats, audit log, live history, in-flight scans,
// and the "re-auth modal" flag. Everything else is derived from this.

import { create } from 'zustand';
import type {
  AgentConfig,
  AgentStats,
  AuditEntry,
  QueryHistoryEntry,
  ScanProgress,
  SchemaChangedPayload,
} from '../types/events';

// One stored notification = one schema_changed event + a client-side id
// + an unread flag so the UI can badge the tab and mark as read on view.
// Bounded to MAX_KEEP entries to avoid unbounded growth.
export interface SchemaNotification extends SchemaChangedPayload {
  id: string;
  received_at: string; // ISO 8601
  read: boolean;
}

const MAX_NOTIFICATIONS_KEEP = 200;

export interface AgentToken {
  access_token: string;
  agent_id: string;
  workspace_id: string;
  expires_in?: number;
}

export type ConnectionStatus = 'online' | 'offline' | 'connecting' | 'error';

export interface ScanState {
  folder: string;
  current: number;
  total: number;
  currentFile: string;
}

interface AgentState {
  // Auth
  authenticated: boolean;
  agentInfo: AgentToken | null;

  // Config
  config: AgentConfig | null;

  // Connection
  connectionStatus: ConnectionStatus;
  connectionDetail: string | null;

  // Stats + audit
  stats: AgentStats | null;
  audit: AuditEntry[];

  // Live history (additive from events + initial snapshot)
  history: QueryHistoryEntry[];

  // In-flight folder scans keyed by folder path
  scansInFlight: Record<string, ScanState>;

  // Re-auth modal flag (raised by auth_expired event)
  reAuthRequired: boolean;

  // Onboarding flag
  onboardingComplete: boolean;

  // Schema-change notifications (newest first). Populated by the
  // schema_changed event; persisted to disk in a follow-up.
  schemaNotifications: SchemaNotification[];

  // Loading / error
  isLoading: boolean;
  error: string | null;

  // Actions
  setAuthenticated: (value: boolean) => void;
  setAgentInfo: (info: AgentToken | null) => void;
  setConfig: (config: AgentConfig) => void;
  setConnectionStatus: (status: ConnectionStatus, detail?: string | null) => void;
  setStats: (stats: AgentStats) => void;
  setAudit: (audit: AuditEntry[]) => void;
  setHistory: (entries: QueryHistoryEntry[]) => void;
  prependHistory: (entry: QueryHistoryEntry) => void;
  applyScanProgress: (p: ScanProgress) => void;
  clearScanProgress: (folder: string) => void;
  setReAuthRequired: (v: boolean) => void;
  setOnboardingComplete: (v: boolean) => void;
  addSchemaNotification: (payload: SchemaChangedPayload) => void;
  markSchemaNotificationRead: (id: string) => void;
  markAllSchemaNotificationsRead: () => void;
  clearSchemaNotifications: () => void;
  setLoading: (value: boolean) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

const initial = {
  authenticated: false,
  agentInfo: null,
  config: null,
  connectionStatus: 'offline' as ConnectionStatus,
  connectionDetail: null,
  stats: null,
  audit: [] as AuditEntry[],
  history: [] as QueryHistoryEntry[],
  scansInFlight: {} as Record<string, ScanState>,
  reAuthRequired: false,
  onboardingComplete: false,
  schemaNotifications: [] as SchemaNotification[],
  isLoading: false,
  error: null,
};

export const useAgentStore = create<AgentState>((set) => ({
  ...initial,

  setAuthenticated: (value) => set({ authenticated: value }),
  setAgentInfo: (info) => set({ agentInfo: info }),
  setConfig: (config) =>
    set({ config, onboardingComplete: config.app.first_run_completed }),
  setConnectionStatus: (status, detail = null) =>
    set({ connectionStatus: status, connectionDetail: detail }),
  setStats: (stats) => set({ stats }),
  setAudit: (audit) => set({ audit }),
  setHistory: (entries) => set({ history: entries }),
  prependHistory: (entry) =>
    set((state) => ({ history: [entry, ...state.history].slice(0, 500) })),
  applyScanProgress: (p) =>
    set((state) => ({
      scansInFlight: {
        ...state.scansInFlight,
        [p.folder]: {
          folder: p.folder,
          current: p.current,
          total: p.total,
          currentFile: p.current_file,
        },
      },
    })),
  clearScanProgress: (folder) =>
    set((state) => {
      const next = { ...state.scansInFlight };
      delete next[folder];
      return { scansInFlight: next };
    }),
  setReAuthRequired: (v) => set({ reAuthRequired: v }),
  setOnboardingComplete: (v) => set({ onboardingComplete: v }),
  addSchemaNotification: (payload) =>
    set((state) => {
      const entry: SchemaNotification = {
        ...payload,
        id:
          typeof crypto !== 'undefined' && 'randomUUID' in crypto
            ? crypto.randomUUID()
            : `${Date.now()}-${Math.random().toString(36).slice(2)}`,
        received_at: new Date().toISOString(),
        read: false,
      };
      return {
        schemaNotifications: [entry, ...state.schemaNotifications].slice(
          0,
          MAX_NOTIFICATIONS_KEEP,
        ),
      };
    }),
  markSchemaNotificationRead: (id) =>
    set((state) => ({
      schemaNotifications: state.schemaNotifications.map((n) =>
        n.id === id ? { ...n, read: true } : n,
      ),
    })),
  markAllSchemaNotificationsRead: () =>
    set((state) => ({
      schemaNotifications: state.schemaNotifications.map((n) => ({
        ...n,
        read: true,
      })),
    })),
  clearSchemaNotifications: () => set({ schemaNotifications: [] }),
  setLoading: (value) => set({ isLoading: value }),
  setError: (error) => set({ error }),
  reset: () => set(initial),
}));
