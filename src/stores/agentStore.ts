// Global agent state.
//
// The store mirrors everything the backend knows that the UI cares about:
// connection status, config, stats, audit log, live history, in-flight scans,
// and the "re-auth modal" flag. Everything else is derived from this.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import type {
  AgentConfig,
  AgentStats,
  AuditEntry,
  QueryHistoryEntry,
  ScanProgress,
  SchemaChangedPayload,
  StoredSchemaNotification,
} from '../types/events';

// Stored notifications come from the Rust side already carrying an id
// + received_at. The in-memory slice keeps the same shape.
export type SchemaNotification = StoredSchemaNotification;

// One Q&A turn on the Ask page. Lifted out of the component so the
// conversation + draft input survive sidebar navigation away and
// back. Mirror of the local interface in Ask.tsx.
export type AskSqlOutcome =
  | {
      kind: 'rows';
      columns: string[];
      rows: string[][];
      total_rows: number;
      truncated: boolean;
    }
  | { kind: 'empty' }
  | { kind: 'insufficient_data'; reason: string }
  | { kind: 'error'; message: string }
  | { kind: 'no_sql_generated' };

export interface AskSqlAttempt {
  sql: string;
  outcome: AskSqlOutcome;
}

/** Format-filter chip values for FolderDetail. `'all'` is the
 *  no-filter default; the rest each map to a set of file_format
 *  strings handled in the consumer (groups e.g. xlsx + xls into
 *  the "excel" chip). */
export type FolderFormatFilter =
  | 'all'
  | 'csv'
  | 'parquet'
  | 'excel'
  | 'documents'
  | 'other';

/** Recency-filter chip values. Maps to a wall-clock cutoff
 *  applied against each dataset's last_modified timestamp. */
export type FolderRecencyFilter = 'any' | '24h' | '7d' | '30d';

export interface AskTurn {
  id: number;
  question: string;
  answer: string;
  provider: string;
  usage: { input_tokens: number; output_tokens: number } | null;
  asked_at: string;
  sql_attempt: AskSqlAttempt | null;
  considered_table_count: number;
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
  setSchemaNotifications: (entries: SchemaNotification[]) => void;
  addSchemaNotification: (payload: SchemaChangedPayload) => void;
  markSchemaNotificationRead: (id: string) => Promise<void>;
  markAllSchemaNotificationsRead: () => Promise<void>;
  clearSchemaNotifications: () => Promise<void>;

  // Global search state — lifted out of SearchPage so the query + last
  // results survive navigating away and back via the sidebar.
  searchQuery: string;
  searchResults: import('../types/events').SearchMatch[];
  setSearchQuery: (q: string) => void;
  setSearchResults: (results: import('../types/events').SearchMatch[]) => void;
  // Ask-page draft + conversation. Lifted out of Ask.tsx so the user
  // doesn't lose what they were typing (or the previous answers)
  // when they switch tabs to look something up. Conversation can
  // grow large — `clearAskTurns` lets the user start fresh from
  // the UI's "Clear" affordance.
  askDraft: string;
  askTurns: AskTurn[];
  setAskDraft: (s: string) => void;
  setAskTurns: (turns: AskTurn[]) => void;
  appendAskTurn: (turn: AskTurn) => void;
  clearAskTurns: () => void;
  // Per-folder filter input (FolderDetail's "Filter files by name…").
  // Keyed by folder.path so each folder remembers its own filter
  // independently. Map keeps the schema flexible for very-many-
  // folders use cases without ballooning the store object.
  folderSearch: Record<string, string>;
  setFolderSearch: (folderPath: string, query: string) => void;
  // Per-folder format filter ('all' or one of the format chips). Same
  // keying as folderSearch — survives tab switches but doesn't bleed
  // between folders.
  folderFormat: Record<string, FolderFormatFilter>;
  setFolderFormat: (folderPath: string, value: FolderFormatFilter) => void;
  // Per-folder recency filter (any / 24h / 7d / 30d).
  folderRecency: Record<string, FolderRecencyFilter>;
  setFolderRecency: (folderPath: string, value: FolderRecencyFilter) => void;
  // Results page (History) filter chips + search. The 'error'
  // value matches the existing local type in History.tsx — keeping
  // the string identical so the lift is a drop-in.
  historyFilter: 'all' | 'success' | 'error';
  historySearch: string;
  setHistoryFilter: (f: 'all' | 'success' | 'error') => void;
  setHistorySearch: (s: string) => void;
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
  searchQuery: '',
  searchResults: [] as import('../types/events').SearchMatch[],
  askDraft: '',
  askTurns: [] as AskTurn[],
  folderSearch: {} as Record<string, string>,
  folderFormat: {} as Record<string, FolderFormatFilter>,
  folderRecency: {} as Record<string, FolderRecencyFilter>,
  historyFilter: 'all' as 'all' | 'success' | 'error',
  historySearch: '',
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
  setSearchQuery: (q) => set({ searchQuery: q }),
  setSearchResults: (results) => set({ searchResults: results }),
  setAskDraft: (s) => set({ askDraft: s }),
  setAskTurns: (turns) => set({ askTurns: turns }),
  // Newest turn first — matches the UI which renders most-recent
  // at the top so the user doesn't have to scroll past old answers.
  appendAskTurn: (turn) => set((state) => ({ askTurns: [turn, ...state.askTurns] })),
  clearAskTurns: () => set({ askTurns: [] }),
  setFolderSearch: (folderPath, query) =>
    set((state) => ({
      folderSearch: { ...state.folderSearch, [folderPath]: query },
    })),
  setFolderFormat: (folderPath, value) =>
    set((state) => ({
      folderFormat: { ...state.folderFormat, [folderPath]: value },
    })),
  setFolderRecency: (folderPath, value) =>
    set((state) => ({
      folderRecency: { ...state.folderRecency, [folderPath]: value },
    })),
  setHistoryFilter: (f) => set({ historyFilter: f }),
  setHistorySearch: (s) => set({ historySearch: s }),
  setSchemaNotifications: (entries) =>
    set({ schemaNotifications: entries.slice(0, MAX_NOTIFICATIONS_KEEP) }),
  addSchemaNotification: (payload) =>
    set((state) => {
      // Dedupe on id — schema_changed carries the id assigned by the
      // Rust side at record-time. If the event re-fires for any reason,
      // we don't want a duplicate entry.
      if (state.schemaNotifications.some((n) => n.id === payload.id)) {
        return {};
      }
      const entry: SchemaNotification = { ...payload, read: false };
      return {
        schemaNotifications: [entry, ...state.schemaNotifications].slice(
          0,
          MAX_NOTIFICATIONS_KEEP,
        ),
      };
    }),
  markSchemaNotificationRead: async (id) => {
    // Optimistic UI update, then persist. If persistence fails, log
    // and keep the optimistic state — the user's intent is clear.
    set((state) => ({
      schemaNotifications: state.schemaNotifications.map((n) =>
        n.id === id ? { ...n, read: true } : n,
      ),
    }));
    try {
      await invoke('mark_schema_notification_read', { id });
    } catch (err) {
      console.error('mark_schema_notification_read failed:', err);
    }
  },
  markAllSchemaNotificationsRead: async () => {
    set((state) => ({
      schemaNotifications: state.schemaNotifications.map((n) => ({
        ...n,
        read: true,
      })),
    }));
    try {
      await invoke('mark_all_schema_notifications_read');
    } catch (err) {
      console.error('mark_all_schema_notifications_read failed:', err);
    }
  },
  clearSchemaNotifications: async () => {
    set({ schemaNotifications: [] });
    try {
      await invoke('clear_schema_notifications');
    } catch (err) {
      console.error('clear_schema_notifications failed:', err);
    }
  },
  setLoading: (value) => set({ isLoading: value }),
  setError: (error) => set({ error }),
  reset: () => set(initial),
}));
