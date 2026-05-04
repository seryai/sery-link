// TypeScript mirror of the Rust event payloads in src-tauri/src/events.rs.
// Keep these in sync — the event name constants in particular must match
// exactly or the frontend will silently miss updates.

export const EVENT_NAMES = {
  SCAN_WALK_PROGRESS: 'scan_walk_progress',
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

/** Pass-1 (filename-walk) progress. Fires per file during the fast walk
 *  pass; the frontend renders this as a "Listing files: 1247 found"
 *  indicator that closes when pass 2 begins (or when scan_complete
 *  arrives, for cache-only / shallow-only folders). */
export interface ScanWalkProgress {
  folder: string;
  discovered: number;
}

/** Pass-2 (content extraction) progress. Same shape as before; the only
 *  semantic shift is that this no longer fires for cache hits and
 *  shallow-tier files — those finish entirely in pass 1. */
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

/** Lifecycle phase of a `DatasetScanned` event. See the Rust-side
 *  `DatasetPhase` enum for the full contract.
 *
 *  - `shallow`: filesystem-only placeholder; a follow-up `content` event
 *    is coming for the same `relative_path`.
 *  - `content`: final hydrated record. No further upgrade for this file
 *    in this scan. Cache hits and shallow-tier files emit `content`
 *    directly with no preceding `shallow` event. */
export type DatasetPhase = 'shallow' | 'content';

export interface DatasetScannedPayload {
  folder: string;
  index: number;
  total: number;
  dataset: DatasetMetadataPayload;
  /** Defaults to `'content'` on Rust ≥ v0.5.x with two-pass scanning;
   *  optional in TS so older agents still hydrate the type cleanly. */
  phase?: DatasetPhase;
}

// Mirror of Rust SearchMatchReason — tagged union for the UI badges.
export type SearchMatchReason =
  | { kind: 'filename' }
  | { kind: 'column'; name: string; col_type: string }
  | { kind: 'content'; snippet: string }
  | {
      // Matched a file Sery knows exists in Drive but didn't cache
      // (too big, Google-native, non-indexable extension). UI uses
      // this to show a "filename only" badge and disable click-
      // through since there's no parsed content to drill into.
      kind: 'skipped_drive';
      reason:
        | 'native_unexportable'
        | 'unsupported_extension'
        | 'too_large'
        | 'download_failed';
    };

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
  /** Whether this folder is exposed via the `--mcp-stdio` MCP server
   *  mode. Off by default; the Settings → MCP tab toggles it.
   *  Optional in TS so configs written by older sery-link versions
   *  (which didn't have the field) deserialize cleanly. */
  mcp_enabled?: boolean;
}

// ─── F42 Sources data model (mirrors src-tauri/src/sources.rs) ────
//
// Discriminated union with `kind` as the tag, matching the
// #[serde(tag = "kind", rename_all = "snake_case")] on the Rust side.
// Every protocol Sery Link can register as a source has a variant
// here. F43-F49 add new variants (sftp, webdav, b2, azure, gcs,
// dropbox, onedrive) following the same shape.

export type SourceKind =
  | {
      kind: 'local';
      path: string;
      recursive: boolean;
      exclude_patterns: string[];
      max_file_size_mb: number;
    }
  | { kind: 'https'; url: string }
  | { kind: 's3'; url: string }
  | { kind: 'google_drive'; account_id: string }
  | {
      // F43: SFTP server. Auth lives in the OS keychain keyed on
      // source_id (sftp_creds module on the Rust side); this TS
      // shape only carries the connection metadata.
      kind: 'sftp';
      host: string;
      port: number;
      username: string;
      base_path: string;
    }
  | {
      // F44: WebDAV server. Auth (Anonymous / Basic / Digest) in
      // the keychain via webdav_creds; only metadata here.
      kind: 'web_dav';
      server_url: string;
      base_path: string;
    }
  | {
      // F48: Dropbox. Personal Access Token in the keychain via
      // dropbox_creds; only base_path here.
      kind: 'dropbox';
      base_path: string;
    }
  | {
      // F46: Azure Blob Storage. SAS token in the keychain via
      // azure_blob_creds.
      kind: 'azure_blob';
      account_url: string;
      prefix: string;
    };

/** One bookmarked source in the Sources sidebar. */
export interface DataSource {
  /** Stable UUID generated on add; never reused. */
  id: string;
  /** User-editable display name (defaults to a sensible per-kind
   *  derivation; renamable via `rename_source`). */
  name: string;
  /** Protocol-specific configuration. */
  kind: SourceKind;
  /** MCP exposure toggle. */
  mcp_enabled: boolean;
  /** RFC3339 timestamp of last successful scan. */
  last_scan_at: string | null;
  last_scan_stats: ScanStats | null;
  /** Sidebar ordering — set by `reorder_sources`. */
  sort_order: number;
  /** Optional grouping; null = top-level. */
  group: string | null;
}

/** One ready-to-paste MCP config snippet returned by the
 *  `get_mcp_snippets` Tauri command. The frontend renders these as
 *  cards with a copy-to-clipboard button. */
export interface McpSnippet {
  /** Stable client identifier — `claude-desktop`, `cursor`, `continue`. */
  client: string;
  /** User-facing client name — `Claude Desktop`, `Cursor`, `Continue`. */
  label: string;
  /** Source-language hint for syntax highlighting. */
  language: 'json' | 'toml';
  /** The pretty-printed config block. */
  config: string;
  /** Where to paste it (file path / location, platform-aware). */
  config_path_hint: string;
}

export interface AgentConfig {
  agent: {
    name: string;
    platform: string;
    hostname: string;
    agent_id: string | null;
  };
  watched_folders: WatchedFolder[];
  /** F42 Sources sidebar — populated by Config::load's migration on
   *  first run after upgrade. Optional in TS so configs from before
   *  v0.7.0 (which lack the field) still deserialize. */
  sources?: DataSource[];
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
