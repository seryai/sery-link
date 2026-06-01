// Database source detail page — /db/:sourceId

import { useEffect, useState } from 'react';
import { useParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { ChevronRight, Database, Loader2, Key, Link, List } from 'lucide-react';
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

  useEffect(() => {
    if (!decodedId) return;
    let cancelled = false;
    setState({ kind: 'loading' });

    invoke<TableSchema[]>('introspect_db_schema', { sourceId: decodedId })
      .then((tables) => {
        if (!cancelled) {
          setState({ kind: 'ok', tables });
          // Reload config so last_scan_at / last_scan_stats written by
          // introspect_db_schema appear in the sidebar immediately.
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

  const toggleTable = (tableName: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(tableName)) next.delete(tableName);
      else next.add(tableName);
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

  const totalSize =
    state.kind === 'ok'
      ? state.tables.reduce((sum, t) => sum + (t.size_bytes ?? 0), 0)
      : 0;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <SourceIcon kind={iconKind} size="lg" />
              <span className="truncate">{source.name}</span>
            </h1>
            <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
              <span className="font-medium capitalize text-slate-600 dark:text-slate-300">
                {source.kind.kind}
              </span>
              {state.kind === 'ok' && (
                <span>
                  {' '}&middot; {state.tables.length}{' '}
                  {state.tables.length === 1 ? 'table' : 'tables'}
                  {totalSize > 0 && (
                    <> &middot; {formatBytes(totalSize)}</>
                  )}
                </span>
              )}
            </p>
          </div>
        </div>
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

              return (
                <div
                  key={table.table_name}
                  className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900"
                >
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
                        {table.columns.length}{' '}
                        {table.columns.length === 1 ? 'col' : 'cols'}
                        {table.size_bytes != null && table.size_bytes > 0 && (
                          <> &middot; {formatBytes(table.size_bytes)}</>
                        )}
                        {table.row_count_estimate != null && (
                          <> &middot; ~{table.row_count_estimate.toLocaleString()} rows</>
                        )}
                        {idxCount > 0 && (
                          <> &middot; {idxCount} {idxCount === 1 ? 'index' : 'indexes'}</>
                        )}
                        {fkCount > 0 && (
                          <> &middot; {fkCount} FK</>
                        )}
                      </span>
                    </div>
                    <ChevronRight
                      className={`h-4 w-4 shrink-0 text-slate-300 transition-transform group-hover:text-purple-500 dark:text-slate-600 ${
                        isOpen ? 'rotate-90' : ''
                      }`}
                      strokeWidth={1.5}
                    />
                  </button>

                  {isOpen && (
                    <div className="border-t border-slate-100 pb-2 dark:border-slate-800">
                      {/* Columns */}
                      <div className="px-4 pt-1">
                        {table.columns.map((col) => (
                          <div
                            key={col.name}
                            className="flex items-baseline gap-2 border-b border-slate-100 py-1 last:border-0 dark:border-slate-800"
                          >
                            {col.is_primary_key && (
                              <Key className="h-3 w-3 shrink-0 text-amber-500" />
                            )}
                            {!col.is_primary_key && (
                              <span className="h-3 w-3 shrink-0" />
                            )}
                            <span className="min-w-0 flex-1 truncate text-xs font-medium text-slate-700 dark:text-slate-300">
                              {col.name}
                            </span>
                            <span className="shrink-0 font-mono text-xs text-slate-400 dark:text-slate-500">
                              {col.data_type}
                            </span>
                            {col.default_value && (
                              <span className="shrink-0 text-[10px] text-slate-300 dark:text-slate-600">
                                ={col.default_value}
                              </span>
                            )}
                            {col.nullable && !col.is_primary_key && (
                              <span className="shrink-0 text-[10px] text-slate-300 dark:text-slate-600">
                                null
                              </span>
                            )}
                          </div>
                        ))}
                      </div>

                      {/* Indexes */}
                      {(table.indexes?.length ?? 0) > 0 && (
                        <div className="mt-2 border-t border-slate-100 px-4 pt-2 dark:border-slate-800">
                          <p className="mb-1 flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400 dark:text-slate-500">
                            <List className="h-3 w-3" /> Indexes
                          </p>
                          {table.indexes!.map((idx) => (
                            <div key={idx.name} className="flex items-center gap-2 py-0.5 text-xs">
                              <span className="font-mono text-slate-600 dark:text-slate-300">{idx.name}</span>
                              <span className="text-slate-400">({idx.columns.join(', ')})</span>
                              {idx.unique && (
                                <span className="rounded bg-blue-50 px-1 text-[10px] text-blue-500 dark:bg-blue-900/30 dark:text-blue-400">
                                  UNIQUE
                                </span>
                              )}
                            </div>
                          ))}
                        </div>
                      )}

                      {/* Foreign keys */}
                      {(table.foreign_keys?.length ?? 0) > 0 && (
                        <div className="mt-2 border-t border-slate-100 px-4 pt-2 dark:border-slate-800">
                          <p className="mb-1 flex items-center gap-1 text-[10px] font-semibold uppercase tracking-wide text-slate-400 dark:text-slate-500">
                            <Link className="h-3 w-3" /> Foreign Keys
                          </p>
                          {table.foreign_keys!.map((fk) => (
                            <div key={fk.name} className="py-0.5 text-xs text-slate-500 dark:text-slate-400">
                              <span className="font-mono text-slate-600 dark:text-slate-300">
                                ({fk.columns.join(', ')})
                              </span>
                              {' → '}
                              <span className="font-mono text-purple-500 dark:text-purple-400">
                                {fk.ref_table}
                              </span>
                              <span className="font-mono text-slate-400">
                                ({fk.ref_columns.join(', ')})
                              </span>
                            </div>
                          ))}
                        </div>
                      )}

                      {/* PK summary if not shown per-column */}
                      {pkCols.length > 0 && (
                        <div className="mt-1 px-4 pb-1">
                          <span className="text-[10px] text-amber-500">
                            PK: {pkCols.map((c) => c.name).join(', ')}
                          </span>
                        </div>
                      )}
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
