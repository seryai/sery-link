// Query history view — event-driven, no polling.
//
// On mount we ask the backend for the persisted history once. After that,
// the `history_updated` event listener (in useAgentEvents) keeps the store
// in sync as new queries stream in. Filters/search run purely on the
// client-side store snapshot.

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertCircle,
  BarChart3,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock,
  Download,
  Search,
  Trash2,
  XCircle,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import type { QueryHistoryEntry } from '../types/events';

type Filter = 'all' | 'success' | 'error';

export function History() {
  const { history, setHistory } = useAgentStore();
  const toast = useToast();
  const [filter, setFilter] = useState<Filter>('all');
  const [search, setSearch] = useState('');
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [showStats, setShowStats] = useState(false);

  // Initial load — the event listener keeps it fresh afterwards
  useEffect(() => {
    let cancelled = false;
    invoke<QueryHistoryEntry[]>('get_query_history', { limit: 500 })
      .then((entries) => {
        if (!cancelled) setHistory(entries);
      })
      .catch((err) => {
        console.error('Failed to load history:', err);
      });
    return () => {
      cancelled = true;
    };
  }, [setHistory]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return history.filter((e) => {
      if (filter === 'success' && e.status !== 'success') return false;
      if (filter === 'error' && e.status !== 'error') return false;
      if (!q) return true;
      return (
        e.file_path.toLowerCase().includes(q) ||
        e.sql.toLowerCase().includes(q) ||
        (e.error ?? '').toLowerCase().includes(q)
      );
    });
  }, [history, filter, search]);

  const counts = useMemo(() => {
    let ok = 0;
    let err = 0;
    for (const e of history) {
      if (e.status === 'success') ok++;
      else err++;
    }
    return { all: history.length, ok, err };
  }, [history]);

  const toggle = (id: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleClear = async () => {
    if (!window.confirm('Clear all query history? This cannot be undone.')) return;
    try {
      await invoke('clear_query_history');
      setHistory([]);
      toast.success('History cleared');
    } catch (err) {
      toast.error(`Couldn't clear history: ${err}`);
    }
  };

  const handleExport = () => {
    // Convert history to CSV
    const headers = [
      'Timestamp',
      'File Path',
      'SQL',
      'Status',
      'Row Count',
      'Duration (ms)',
      'Error',
    ];
    const rows = filtered.map((e) => [
      e.timestamp,
      e.file_path,
      e.sql.replace(/"/g, '""'), // Escape quotes
      e.status,
      e.row_count?.toString() ?? '',
      e.duration_ms.toString(),
      (e.error ?? '').replace(/"/g, '""'),
    ]);

    const csv = [
      headers.map((h) => `"${h}"`).join(','),
      ...rows.map((r) => r.map((c) => `"${c}"`).join(',')),
    ].join('\n');

    // Download
    const blob = new Blob([csv], { type: 'text/csv' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `query-history-${new Date().toISOString().split('T')[0]}.csv`;
    a.click();
    URL.revokeObjectURL(url);
    toast.success(`Exported ${filtered.length} queries to CSV`);
  };

  return (
    <div className="mx-auto max-w-5xl p-8">
      {/* Header */}
      <div className="mb-6 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold text-slate-900 dark:text-slate-50">
            Query history
          </h1>
          <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
            Every SQL the cloud has asked this agent to run locally.
          </p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setShowStats(!showStats)}
            disabled={history.length === 0}
            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <BarChart3 className="h-4 w-4" />
            {showStats ? 'Hide' : 'Stats'}
          </button>
          <button
            onClick={handleExport}
            disabled={filtered.length === 0}
            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <Download className="h-4 w-4" />
            Export
          </button>
          <button
            onClick={handleClear}
            disabled={history.length === 0}
            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <Trash2 className="h-4 w-4" />
            Clear
          </button>
        </div>
      </div>

      {/* Statistics */}
      {showStats && <Statistics history={history} />}

      {/* Filter bar */}
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <div className="inline-flex rounded-lg border border-slate-200 bg-white p-0.5 dark:border-slate-800 dark:bg-slate-900">
          <FilterPill
            label="All"
            count={counts.all}
            active={filter === 'all'}
            onClick={() => setFilter('all')}
          />
          <FilterPill
            label="Success"
            count={counts.ok}
            active={filter === 'success'}
            onClick={() => setFilter('success')}
          />
          <FilterPill
            label="Errors"
            count={counts.err}
            active={filter === 'error'}
            onClick={() => setFilter('error')}
          />
        </div>

        <div className="relative flex-1 min-w-[200px]">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Search file path, SQL, or error…"
            className="w-full rounded-lg border border-slate-200 bg-white py-2 pl-9 pr-3 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-100 dark:placeholder-slate-500"
          />
        </div>
      </div>

      {/* List */}
      {filtered.length === 0 ? (
        <EmptyState hasAny={history.length > 0} />
      ) : (
        <div className="overflow-hidden rounded-xl border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
          {filtered.map((entry, idx) => {
            const id = `${entry.query_id ?? 'anon'}-${entry.timestamp}-${idx}`;
            const isOpen = expanded.has(id);
            return (
              <HistoryRow
                key={id}
                entry={entry}
                isOpen={isOpen}
                onToggle={() => toggle(id)}
                isLast={idx === filtered.length - 1}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}

// ─── Row ────────────────────────────────────────────────────────────────────

function HistoryRow({
  entry,
  isOpen,
  onToggle,
  isLast,
}: {
  entry: QueryHistoryEntry;
  isOpen: boolean;
  onToggle: () => void;
  isLast: boolean;
}) {
  const ok = entry.status === 'success';
  const hasSql = entry.sql.trim().length > 0;
  const hasError = !!entry.error;

  return (
    <div
      className={`${isLast ? '' : 'border-b border-slate-100 dark:border-slate-800'}`}
    >
      <button
        onClick={onToggle}
        disabled={!hasSql && !hasError}
        className="flex w-full items-start gap-3 px-4 py-3 text-left transition-colors hover:bg-slate-50 disabled:cursor-default dark:hover:bg-slate-800/50"
      >
        <div className="mt-0.5 shrink-0">
          {ok ? (
            <CheckCircle2 className="h-5 w-5 text-emerald-500" />
          ) : (
            <XCircle className="h-5 w-5 text-rose-500" />
          )}
        </div>

        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[11px] text-slate-500 dark:text-slate-400">
            <span title={new Date(entry.timestamp).toLocaleString()}>
              {formatRelativeTime(entry.timestamp)}
            </span>
            <span>·</span>
            <span>{entry.duration_ms} ms</span>
            {ok && entry.row_count !== null && (
              <>
                <span>·</span>
                <span>{entry.row_count.toLocaleString()} rows</span>
              </>
            )}
          </div>
          <div
            className="mt-0.5 truncate font-mono text-xs text-slate-700 dark:text-slate-300"
            title={entry.file_path}
          >
            {entry.file_path}
          </div>
        </div>

        {(hasSql || hasError) && (
          <div className="mt-0.5 shrink-0 text-slate-400">
            {isOpen ? (
              <ChevronDown className="h-4 w-4" />
            ) : (
              <ChevronRight className="h-4 w-4" />
            )}
          </div>
        )}
      </button>

      {isOpen && (
        <div className="border-t border-slate-100 bg-slate-50 px-4 py-3 dark:border-slate-800 dark:bg-slate-950/40">
          {hasSql && (
            <pre className="max-h-64 overflow-auto rounded-md border border-slate-200 bg-white p-3 font-mono text-[11px] leading-relaxed text-slate-700 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-200">
              {entry.sql}
            </pre>
          )}
          {hasError && (
            <div className="mt-2 flex items-start gap-2 rounded-md border border-rose-200 bg-rose-50 p-3 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
              <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
              <span className="font-mono">{entry.error}</span>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Bits ───────────────────────────────────────────────────────────────────

function FilterPill({
  label,
  count,
  active,
  onClick,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-3 py-1 text-sm font-medium transition-colors ${
        active
          ? 'bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300'
          : 'text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100'
      }`}
    >
      {label}
      <span
        className={`ml-1.5 rounded-full px-1.5 py-0.5 text-[10px] ${
          active
            ? 'bg-white text-purple-700 dark:bg-purple-950 dark:text-purple-300'
            : 'bg-slate-100 text-slate-500 dark:bg-slate-800 dark:text-slate-400'
        }`}
      >
        {count}
      </span>
    </button>
  );
}

function EmptyState({ hasAny }: { hasAny: boolean }) {
  return (
    <div className="rounded-xl border border-dashed border-slate-300 bg-slate-50 py-16 text-center dark:border-slate-700 dark:bg-slate-900">
      <Clock className="mx-auto mb-3 h-10 w-10 text-slate-300 dark:text-slate-600" />
      <p className="text-sm font-medium text-slate-700 dark:text-slate-300">
        {hasAny ? 'No queries match your filter' : 'No queries yet'}
      </p>
      <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
        {hasAny
          ? 'Try clearing the search or switching tabs.'
          : 'When Sery runs queries against your local files, they will show up here.'}
      </p>
    </div>
  );
}

function Statistics({ history }: { history: QueryHistoryEntry[] }) {
  const stats = useMemo(() => {
    let totalSuccess = 0;
    let totalError = 0;
    let totalDuration = 0;
    let totalRows = 0;
    const filesMap = new Map<string, number>();

    for (const entry of history) {
      if (entry.status === 'success') {
        totalSuccess++;
        if (entry.row_count) totalRows += entry.row_count;
      } else {
        totalError++;
      }
      totalDuration += entry.duration_ms;

      // Track queries per file
      const count = filesMap.get(entry.file_path) || 0;
      filesMap.set(entry.file_path, count + 1);
    }

    // Top 5 most queried files
    const topFiles = Array.from(filesMap.entries())
      .sort((a, b) => b[1] - a[1])
      .slice(0, 5);

    return {
      total: history.length,
      success: totalSuccess,
      error: totalError,
      avgDuration: history.length > 0 ? Math.round(totalDuration / history.length) : 0,
      totalRows,
      topFiles,
    };
  }, [history]);

  return (
    <div className="mb-6 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
      {/* Total Queries */}
      <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="text-xs font-medium text-slate-500 dark:text-slate-400">
          Total Queries
        </div>
        <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-slate-100">
          {stats.total.toLocaleString()}
        </div>
        <div className="mt-1 flex gap-4 text-xs">
          <span className="text-emerald-600 dark:text-emerald-400">
            ✓ {stats.success}
          </span>
          <span className="text-rose-600 dark:text-rose-400">
            ✗ {stats.error}
          </span>
        </div>
      </div>

      {/* Success Rate */}
      <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="text-xs font-medium text-slate-500 dark:text-slate-400">
          Success Rate
        </div>
        <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-slate-100">
          {stats.total > 0
            ? Math.round((stats.success / stats.total) * 100)
            : 0}
          %
        </div>
        <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-slate-100 dark:bg-slate-800">
          <div
            className="h-full bg-emerald-500"
            style={{
              width: `${stats.total > 0 ? (stats.success / stats.total) * 100 : 0}%`,
            }}
          />
        </div>
      </div>

      {/* Avg Duration */}
      <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="text-xs font-medium text-slate-500 dark:text-slate-400">
          Avg Duration
        </div>
        <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-slate-100">
          {stats.avgDuration}
          <span className="ml-1 text-sm font-normal text-slate-500">ms</span>
        </div>
      </div>

      {/* Total Rows */}
      <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="text-xs font-medium text-slate-500 dark:text-slate-400">
          Total Rows
        </div>
        <div className="mt-1 text-2xl font-bold text-slate-900 dark:text-slate-100">
          {stats.totalRows.toLocaleString()}
        </div>
      </div>

      {/* Top Files */}
      {stats.topFiles.length > 0 && (
        <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900 sm:col-span-2 lg:col-span-4">
          <div className="mb-3 text-xs font-medium text-slate-500 dark:text-slate-400">
            Most Queried Files
          </div>
          <div className="space-y-2">
            {stats.topFiles.map(([file, count]) => (
              <div key={file} className="flex items-center justify-between gap-4">
                <div className="min-w-0 flex-1 truncate font-mono text-xs text-slate-700 dark:text-slate-300" title={file}>
                  {file}
                </div>
                <div className="shrink-0 text-xs font-medium text-slate-500 dark:text-slate-400">
                  {count} {count === 1 ? 'query' : 'queries'}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return 'just now';
  const diff = Date.now() - then;
  if (diff < 0) return 'just now';
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 7) return `${day}d ago`;
  return new Date(iso).toLocaleDateString();
}
