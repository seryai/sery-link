// Database source detail page — /db/:sourceId
//
// Reached by clicking a database source row in the Sources sidebar.
// Calls `introspect_db_schema` on mount to fetch the live table list
// from the connected database, then renders each table with its
// columns inline-expandable. No cloud connection required.

import { useEffect, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { ArrowLeft, ChevronRight, Database, Loader2 } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { legacyKindStringOf } from '../utils/sources';
import { SourceIcon } from './SourceIcon';

interface ColumnInfo {
  name: string;
  data_type: string;
  nullable: boolean;
}

interface TableSchema {
  table_name: string;
  columns: ColumnInfo[];
  row_count_estimate: number | null;
}

type LoadState =
  | { kind: 'loading' }
  | { kind: 'error'; message: string }
  | { kind: 'ok'; tables: TableSchema[] };

export function DatabaseDetail() {
  const { sourceId } = useParams<{ sourceId: string }>();
  const navigate = useNavigate();
  const { config } = useAgentStore();

  const decodedId = sourceId ? decodeURIComponent(sourceId) : '';
  const source = config?.sources?.find((s) => s.id === decodedId);

  const [state, setState] = useState<LoadState>({ kind: 'loading' });
  // Set of table names that are expanded to show columns.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!decodedId) return;
    let cancelled = false;
    setState({ kind: 'loading' });

    invoke<TableSchema[]>('introspect_db_schema', { sourceId: decodedId })
      .then((tables) => {
        if (!cancelled) setState({ kind: 'ok', tables });
      })
      .catch((err) => {
        if (!cancelled) setState({ kind: 'error', message: String(err) });
      });

    return () => {
      cancelled = true;
    };
  }, [decodedId]);

  const toggleTable = (tableName: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(tableName)) {
        next.delete(tableName);
      } else {
        next.add(tableName);
      }
      return next;
    });
  };

  // Source-not-found guard (mirrors FolderDetail pattern).
  if (!source) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
          <button
            onClick={() => navigate('/sources')}
            className="inline-flex items-center gap-2 text-sm text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
          >
            <ArrowLeft className="h-4 w-4" /> Back to sources
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">
              This source isn't connected.
            </p>
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              {decodedId || '(no id)'}
            </p>
          </div>
        </div>
      </div>
    );
  }

  const iconKind = legacyKindStringOf(source);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header — matches FolderDetail layout exactly */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <button
          onClick={() => navigate('/sources')}
          className="mb-3 inline-flex items-center gap-2 text-xs text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
        >
          <ArrowLeft className="h-3.5 w-3.5" /> All sources
        </button>
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
                  {' '}
                  &middot; {state.tables.length}{' '}
                  {state.tables.length === 1 ? 'table' : 'tables'}
                </span>
              )}
            </p>
          </div>
        </div>
      </div>

      {/* Body */}
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
            <p className="text-sm text-slate-600 dark:text-slate-400">
              No tables found in this database.
            </p>
          </div>
        )}

        {state.kind === 'ok' && state.tables.length > 0 && (
          <div className="space-y-1">
            {state.tables.map((table) => {
              const isOpen = expanded.has(table.table_name);
              return (
                <div
                  key={table.table_name}
                  className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900"
                >
                  {/* Table header row — click to expand/collapse */}
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
                        {table.columns.length === 1 ? 'column' : 'columns'}
                        {table.row_count_estimate !== null && (
                          <> &middot; ~{table.row_count_estimate.toLocaleString()} rows</>
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

                  {/* Column list — visible when expanded */}
                  {isOpen && (
                    <div className="border-t border-slate-100 px-4 pb-2 pt-1 dark:border-slate-800">
                      {table.columns.map((col) => (
                        <div
                          key={col.name}
                          className="flex items-baseline gap-2 border-b border-slate-100 py-1 last:border-0 dark:border-slate-800"
                        >
                          <span className="min-w-0 flex-1 truncate text-xs font-medium text-slate-700 dark:text-slate-300">
                            {col.name}
                          </span>
                          <span className="shrink-0 font-mono text-xs text-slate-400 dark:text-slate-500">
                            {col.data_type}
                          </span>
                          {col.nullable && (
                            <span className="shrink-0 text-[10px] text-slate-300 dark:text-slate-600">
                              nullable
                            </span>
                          )}
                        </div>
                      ))}
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
