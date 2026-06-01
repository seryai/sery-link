// Database source detail page — /db/:sourceId

import { useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { ChevronRight, Database, Loader2, Key, Link, List, RefreshCw } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { legacyKindStringOf } from '../utils/sources';
import { SourceIcon } from './SourceIcon';
import type { AgentConfig } from '../types/events';

interface ColumnInfo {
  name: string;
  data_type: string;
  nullable: boolean;
  is_primary_key: boolean;
  default_value?: string;
  comment?: string;
}

interface IndexInfo {
  name: string;
  columns: string[];
  unique: boolean;
  primary: boolean;
}

interface ForeignKeyInfo {
  name: string;
  columns: string[];
  ref_table: string;
  ref_columns: string[];
}

interface TableSchema {
  table_name: string;
  columns: ColumnInfo[];
  row_count_estimate: number | null;
  size_bytes?: number;
  indexes?: IndexInfo[];
  foreign_keys?: ForeignKeyInfo[];
}

interface DbTableProfile {
  columns: unknown[];
  sample_rows: Record<string, string>[];
  sample_size: number;
}

type SampleState =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ok'; data: DbTableProfile }
  | { kind: 'error'; message: string };

type LoadState =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'ok'; tables: TableSchema[] };

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function DatabaseDetail() {
  const { sourceId } = useParams<{ sourceId: string }>();
  const { config, setConfig } = useAgentStore();

  const decodedId = sourceId ? decodeURIComponent(sourceId) : '';
  const source = config?.sources?.find((s) => s.id === decodedId);

  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [samples, setSamples] = useState<Map<string, SampleState>>(new Map());
  const [syncing, setSyncing] = useState(false);

  useEffect(() => {
    if (!decodedId) return;
    let cancelled = false;
    setState({ kind: 'loading' });

    invoke<TableSchema[]>('introspect_db_schema', { sourceId: decodedId })
      .then((tables) => {
        if (!cancelled) {
          setState({ kind: 'ok', tables });
          invoke<AgentConfig>('get_config')
            .then((cfg) => { if (!cancelled) setConfig(cfg); })
            .catch(() => undefined);
        }
      })
      .catch((err) => {
        if (!cancelled) setState({ kind: 'error', message: String(err) });
      });

    return () => { cancelled = true; };
  }, [decodedId]);

  const syncToCloud = async () => {
    setSyncing(true);
    try {
      await invoke('rescan_source_by_id', { sourceId: decodedId });
      // Re-introspect so the UI reflects any schema changes.
      const tables = await invoke<TableSchema[]>('introspect_db_schema', { sourceId: decodedId });
      setState({ kind: 'ok', tables });
    } catch (err) {
      setState({ kind: 'error', message: String(err) });
    } finally {
      setSyncing(false);
    }
  };

  const loadSample = (tableName: string) => {
    setSamples((prev) => {
      const existing = prev.get(tableName);
      if (existing && existing.kind !== 'idle') return prev;
      const next = new Map(prev);
      next.set(tableName, { kind: 'loading' });
      return next;
    });
    invoke<DbTableProfile>('profile_db_table', { sourceId: decodedId, tableName })
      .then((data) => setSamples((prev) => { const m = new Map(prev); m.set(tableName, { kind: 'ok', data }); return m; }))
      .catch((err) => setSamples((prev) => { const m = new Map(prev); m.set(tableName, { kind: 'error', message: String(err) }); return m; }));
  };

  const toggleTable = (tableName: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(tableName)) {
        next.delete(tableName);
      } else {
        next.add(tableName);
        loadSample(tableName);
      }
      return next;
    });
  };

  if (!source) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="flex-1 overflow-y-auto p-6">
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">This source isn't connected.</p>
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">{decodedId || '(no id)'}</p>
          </div>
        </div>
      </div>
    );
  }

  const iconKind = legacyKindStringOf(source);
  const totalSize = state.kind === 'ok'
    ? state.tables.reduce((sum, t) => sum + (t.size_bytes ?? 0), 0)
    : 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between gap-3">
          <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
            <SourceIcon kind={iconKind} size="lg" />
            <span className="truncate">{source.name}</span>
          </h1>
          <button
            onClick={syncToCloud}
            disabled={syncing}
            title="Re-introspect schema and sync to cloud"
            className="flex items-center gap-1.5 rounded-md border border-slate-200 bg-white px-2.5 py-1.5 text-xs font-medium text-slate-600 shadow-sm transition-colors hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${syncing ? 'animate-spin' : ''}`} />
            {syncing ? 'Syncing…' : 'Sync'}
          </button>
        </div>
        <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
          <span className="font-medium capitalize text-slate-600 dark:text-slate-300">{source.kind.kind}</span>
          {state.kind === 'ok' && (
            <span>
              {' '}&middot; {state.tables.length} {state.tables.length === 1 ? 'table' : 'tables'}
              {totalSize > 0 && <> &middot; {formatBytes(totalSize)}</>}
            </span>
          )}
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        {state.kind === 'loading' && (
          <div className="flex items-center gap-2 text-sm text-slate-500 dark:text-slate-400">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading schema…
          </div>
        )}
        {state.kind === 'error' && (
          <div className="rounded-md border border-rose-300 bg-rose-50 p-4 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            <p className="font-medium">Could not load schema</p>
            <p className="mt-1">{state.message}</p>
          </div>
        )}
        {state.kind === 'ok' && state.tables.length === 0 && (
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">No tables found in this database.</p>
          </div>
        )}
        {state.kind === 'ok' && state.tables.length > 0 && (
          <div className="space-y-1">
            {state.tables.map((table) => {
              const isOpen = expanded.has(table.table_name);
              const pkCols = table.columns.filter((c) => c.is_primary_key);
              const fkCount = table.foreign_keys?.length ?? 0;
              const idxCount = table.indexes?.length ?? 0;
              const sample = samples.get(table.table_name) ?? { kind: 'idle' };
              // whether any column has a comment
              const hasComments = table.columns.some((c) => c.comment);

              return (
                <div key={table.table_name} className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
                  {/* Table header row */}
                  <button
                    onClick={() => toggleTable(table.table_name)}
                    className="group flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors hover:bg-slate-50 dark:hover:bg-slate-800/60"
                  >
                    <Database className="h-4 w-4 shrink-0 text-purple-500 dark:text-purple-400" />
                    <div className="min-w-0 flex-1">
                      <span className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
                        {table.table_name}
                      </span>
                      <span className="ml-2 text-xs text-slate-400 dark:text-slate-500">
                        {table.columns.length} {table.columns.length === 1 ? 'col' : 'cols'}
                        {table.size_bytes != null && table.size_bytes > 0 && <> &middot; {formatBytes(table.size_bytes)}</>}
                        {table.row_count_estimate != null && <> &middot; ~{table.row_count_estimate.toLocaleString()} rows</>}
                        {idxCount > 0 && <> &middot; {idxCount} {idxCount === 1 ? 'index' : 'indexes'}</>}
                        {fkCount > 0 && <> &middot; {fkCount} FK</>}
                      </span>
                    </div>
                    <ChevronRight
                      className={`h-4 w-4 shrink-0 text-slate-300 transition-transform group-hover:text-purple-500 dark:text-slate-600 ${isOpen ? 'rotate-90' : ''}`}
                      strokeWidth={1.5}
                    />
                  </button>

                  {isOpen && (
                    <div className="border-t border-slate-100 dark:border-slate-800">

                      {/* Column structure table */}
                      <div className="overflow-x-auto">
                        <table className="w-full text-xs">
                          <thead>
                            <tr className="bg-slate-50 dark:bg-slate-800/50">
                              <th className="px-4 py-2 text-left font-medium text-slate-500 dark:text-slate-400">Column</th>
                              <th className="px-4 py-2 text-left font-medium text-slate-500 dark:text-slate-400">Type</th>
                              <th className="px-4 py-2 text-center font-medium text-slate-500 dark:text-slate-400">Null</th>
                              <th className="px-4 py-2 text-center font-medium text-slate-500 dark:text-slate-400">PK</th>
                              <th className="px-4 py-2 text-left font-medium text-slate-500 dark:text-slate-400">Default</th>
                              {hasComments && (
                                <th className="px-4 py-2 text-left font-medium text-slate-500 dark:text-slate-400">Comment</th>
                              )}
                            </tr>
                          </thead>
                          <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
                            {table.columns.map((col) => (
                              <tr key={col.name} className="hover:bg-slate-50 dark:hover:bg-slate-800/40">
                                <td className="px-4 py-1.5 font-mono font-medium text-slate-800 dark:text-slate-100">
                                  {col.name}
                                </td>
                                <td className="px-4 py-1.5 font-mono text-slate-500 dark:text-slate-400">
                                  {col.data_type}
                                </td>
                                <td className="px-4 py-1.5 text-center">
                                  {col.nullable
                                    ? <span className="text-amber-500">✓</span>
                                    : <span className="text-slate-300 dark:text-slate-600">—</span>}
                                </td>
                                <td className="px-4 py-1.5 text-center">
                                  {col.is_primary_key
                                    ? <Key className="inline h-3 w-3 text-amber-500" />
                                    : <span className="text-slate-300 dark:text-slate-600">—</span>}
                                </td>
                                <td className="max-w-[160px] truncate px-4 py-1.5 font-mono text-slate-400 dark:text-slate-500" title={col.default_value}>
                                  {col.default_value ?? <span className="text-slate-300 dark:text-slate-600">—</span>}
                                </td>
                                {hasComments && (
                                  <td className="max-w-[200px] truncate px-4 py-1.5 text-slate-500 dark:text-slate-400" title={col.comment}>
                                    {col.comment ?? ''}
                                  </td>
                                )}
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>

                      {/* Indexes */}
                      {idxCount > 0 && (
                        <div className="border-t border-slate-100 px-4 py-2 dark:border-slate-800">
                          <p className="mb-1 flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400 dark:text-slate-500">
                            <List className="h-3 w-3" /> Indexes
                          </p>
                          {table.indexes!.map((idx) => (
                            <div key={idx.name} className="flex items-center gap-2 py-0.5 text-xs">
                              <span className="font-mono text-slate-600 dark:text-slate-300">{idx.name}</span>
                              <span className="text-slate-400">({idx.columns.join(', ')})</span>
                              {idx.unique && <span className="rounded bg-blue-50 px-1 text-[10px] text-blue-500 dark:bg-blue-900/30 dark:text-blue-400">UNIQUE</span>}
                            </div>
                          ))}
                        </div>
                      )}

                      {/* Foreign keys */}
                      {fkCount > 0 && (
                        <div className="border-t border-slate-100 px-4 py-2 dark:border-slate-800">
                          <p className="mb-1 flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400 dark:text-slate-500">
                            <Link className="h-3 w-3" /> Foreign Keys
                          </p>
                          {table.foreign_keys!.map((fk) => (
                            <div key={fk.name} className="py-0.5 text-xs text-slate-500 dark:text-slate-400">
                              <span className="font-mono text-slate-600 dark:text-slate-300">({fk.columns.join(', ')})</span>
                              {' → '}
                              <span className="font-mono text-purple-500 dark:text-purple-400">{fk.ref_table}</span>
                              <span className="font-mono text-slate-400">({fk.ref_columns.join(', ')})</span>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* PK summary */}
                      {pkCols.length > 0 && (
                        <div className="border-t border-slate-100 px-4 py-1 dark:border-slate-800">
                          <span className="text-[10px] text-amber-500">PK: {pkCols.map((c) => c.name).join(', ')}</span>
                        </div>
                      )}

                      {/* Sample rows */}
                      <div className="border-t border-slate-100 dark:border-slate-800">
                        <div className="flex items-center gap-1.5 px-4 py-2 text-[11px] font-semibold uppercase tracking-wide text-slate-400 dark:text-slate-500">
                          Sample rows
                          {sample.kind === 'loading' && <Loader2 className="h-3 w-3 animate-spin" />}
                        </div>
                        {sample.kind === 'error' && (
                          <p className="px-4 pb-2 text-[11px] text-rose-600 dark:text-rose-400">{sample.message}</p>
                        )}
                        {sample.kind === 'ok' && sample.data.sample_rows.length === 0 && (
                          <p className="px-4 pb-2 text-xs text-slate-400 dark:text-slate-500">No rows found.</p>
                        )}
                        {sample.kind === 'ok' && sample.data.sample_rows.length > 0 && (
                          <div className="overflow-x-auto pb-2">
                            <table className="w-full text-xs">
                              <thead>
                                <tr className="bg-slate-50 dark:bg-slate-800/50">
                                  {table.columns.map((col) => (
                                    <th key={col.name} className="px-3 py-1.5 text-left font-mono font-medium text-slate-500 dark:text-slate-400 whitespace-nowrap">
                                      {col.name}
                                    </th>
                                  ))}
                                </tr>
                              </thead>
                              <tbody className="divide-y divide-slate-100 dark:divide-slate-800">
                                {sample.data.sample_rows.map((row, ri) => (
                                  <tr key={ri} className="hover:bg-slate-50 dark:hover:bg-slate-800/40">
                                    {table.columns.map((col) => (
                                      <td key={col.name} className="max-w-[160px] truncate px-3 py-1 text-slate-700 dark:text-slate-300 whitespace-nowrap" title={row[col.name]}>
                                        {row[col.name] !== '' && row[col.name] !== undefined
                                          ? row[col.name]
                                          : <span className="text-slate-300 dark:text-slate-600">null</span>}
                                      </td>
                                    ))}
                                  </tr>
                                ))}
                              </tbody>
                            </table>
                          </div>
                        )}
                      </div>

                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
