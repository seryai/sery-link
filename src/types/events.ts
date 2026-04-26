// TypeScript mirror of the Rust event payloads in src-tauri/src/events.rs.
// Keep these in sync — the event name constants in particular must match
// exactly or the frontend will silently miss updates.

export const EVENT_NAMES = {
  SCAN_PROGRESS: 'scan_progress',
  SCAN_COMPLETE: 'scan_complete',
  DATASET_SCANNED: 'dataset_scanned',
  SCHEMA_CHANGED: 'schema_changed',
  WS_STATUS: 'ws_status',
  QUERY_STARTED: 'query_started',
  QUERY_COMPLETED: 'query_completed',
  HISTORY_UPDATED: 'history_updated',
  AUTH_EXPIRED: 'auth_expired',
  SYNC_COMPLETED: 'sync_completed',
  SYNC_FAILED: 'sync_failed',
  STATS_UPDATED: 'stats_updated',
} as const;

export interface ScanProgress {
  folder: string;
  current: number;
  total: number;
  current_file: string;
}

export interface ScanComplete {
  folder: string;
  datasets: number;
  columns: number;
  errors: number;
  total_bytes: number;
  duration_ms: number;
}

// Mirror of Rust DatasetMetadata + events::DatasetScanned.
export interface ColumnSchemaPayload {
  name: string;
  type: string;
  nullable: boolean;
}

export interface DatasetMetadataPayload {
  relative_path: string;
  file_format: string;
  size_bytes: number;
  row_count_estimate: number | null;
  schema: ColumnSchemaPayload[];
  last_modified: string;
  document_markdown?: string;
  sample_rows?: Record<string, unknown>[];
  samples_redacted: boolean;
}

export interface DatasetScannedPayload {
  folder: string;
  index: number;
  total: number;
  dataset: DatasetMetadataPayload;
}

// Mirror of Rust SearchMatchReason — tagged union for the UI badges.
export type SearchMatchReason =
  | { kind: 'filename' }
  | { kind: 'column'; name: string; col_type: string }
  | { kind: 'content'; snippet: string };

export interface SearchMatch {
  folder_path: string;
  relative_path: string;
  file_format: string;
  size_bytes: number;
  last_modified: string;
  row_count_estimate: number | null;
  column_count: number;
  match_reasons: SearchMatchReason[];
  score: number;
}

// Output of the `profile_dataset` Tauri command — one per column in the
// file. Values are strings so the frontend doesn't have to deal with
// numeric precision or timestamp formatting; DuckDB's SUMMARIZE emits
// min/max/avg/std as VARCHAR so all column types are representable.
export interface ColumnProfile {
  column_name: string;
  column_type: string;
  count: number | null;
  null_percentage: number | null;
  approx_unique: number | null;
  min: string | null;
  max: string | null;
  avg: string | null;
  std: string | null;
}

export interface WsStatus {
  status: 'online' | 'connecting' | 'offline' | 'error';
  detail: string | null;
}

export interface QueryStarted {
  query_id: string;
  file_path: string;
}

export interface QueryCompleted {
  query_id: string;
  file_path: string;
  status: 'success' | 'error';
  row_count: number | null;
  duration_ms: number;
  error: string | null;
}

export interface SyncCompletedPayload {
  folder: string;
  datasets: number;
}

// Mirror of Rust events::SchemaChanged. The `diff.changes` entries are
// a tagged union — each carries a `kind` discriminator matching the
// ColumnChange enum variants on the Rust side.
export type ColumnChange =
  | { kind: 'Added'; name: string; column_type: string }
  | { kind: 'Removed'; name: string; column_type: string }
  | {
      kind: 'TypeChanged';
      name: string;
      old_type: string;
      new_type: string;
    };

export interface SchemaDiff {
  changes: ColumnChange[];
}

export interface SchemaChangedPayload {
  // Populated by the Rust side via schema_notifications::record so the
  // store and the on-disk log agree on the same id — mark-read needs it.
  id: string;
  received_at: string; // RFC 3339
  workspace_id: string;
  dataset_path: string;
  dataset_name: string;
  added: number;
  removed: number;
  type_changed: number;
  diff: SchemaDiff;
  // Which machine in the workspace observed this change. Null for records written
  // before this field existed or when the origin agent is unknown.
  origin_agent_id: string | null;
}

// The shape returned by get_schema_notifications. Same shape as
// SchemaChangedPayload plus a persisted `read` flag.
export interface StoredSchemaNotification extends SchemaChangedPayload {
  read: boolean;
}

export interface SyncFailedPayload {
  folder: string;
  error: string;
}

export interface AgentStats {
  total_queries: number;
  queries_today: number;
  queries_today_date: string | null;
  successful_queries: number;
  failed_queries: number;
  total_bytes_read: number;
  last_query_at: string | null;
  uptime_started_at: string | null;
}

export type AuditKind = 'sync' | 'byok_call';

export interface AuditEntry {
  timestamp: string;
  // Defaults to 'sync' on entries written before v0.5.x.
  kind?: AuditKind;
  // Sync fields — empty/zero on byok_call entries.
  folder: string;
  dataset_count: number;
  column_count: number;
  total_bytes: number;
  // BYOK fields — only populated on byok_call entries.
  provider?: string;
  // The host the request actually targeted (e.g. 'api.anthropic.com').
  // The structural privacy proof — see audit.rs `record_byok_call`.
  host?: string;
  prompt_chars?: number;
  response_chars?: number;
  duration_ms?: number;
  // Shared.
  status: 'success' | 'error';
  error: string | null;
}

export interface ScanStats {
  datasets: number;
  columns: number;
  errors: number;
  total_bytes: number;
  duration_ms: number;
}

// Extended config types matching the new Rust schema.
export interface WatchedFolder {
  path: string;
  recursive: boolean;
  exclude_patterns: string[];
  max_file_size_mb: number;
  last_scan_at: string | null;
  last_scan_stats: ScanStats | null;
}

export interface AgentConfig {
  agent: {
    name: string;
    platform: string;
    hostname: string;
    agent_id: string | null;
  };
  watched_folders: WatchedFolder[];
  cloud: {
    api_url: string;
    websocket_url: string;
    web_url: string;
  };
  sync: {
    interval_seconds: number;
    auto_sync_on_change: boolean;
    fallback_scan_interval_seconds: number;
    /** ROADMAP F2 — opt-in for uploading extracted document text. Default
     *  false on the Rust side; older configs deserialize without this
     *  field, so it's typed as optional for backwards compat. */
    include_document_text?: boolean;
  };
  app: {
    theme: 'light' | 'dark' | 'system';
    launch_at_login: boolean;
    auto_update: boolean;
    notifications_enabled: boolean;
    first_run_completed: boolean;
    window_hide_notified: boolean;
    schema_change_toasts_enabled: boolean;
  };
}

export interface QueryHistoryEntry {
  query_id: string | null;
  timestamp: string;
  file_path: string;
  sql: string;
  status: 'success' | 'error';
  row_count: number | null;
  duration_ms: number;
  error: string | null;
}
