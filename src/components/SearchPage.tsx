// Global search — the v1 hero page.
//
// Single input, three signals: filename, column name (tabular files),
// and document content (DOCX/PPTX). Results are ranked by the Rust
// `rank_matches` function and surfaced with badges explaining WHY each
// one hit so the user understands the ranking.
//
// Pure client-side rendering on top of the scan cache — no cloud round
// trip, works fully offline. The only Tauri command this page calls is
// `search_all_folders`. Navigating a result opens FolderDetail for its
// containing folder.

import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useNavigate } from 'react-router-dom';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  Columns3,
  Database,
  FileText,
  Folder as FolderIcon,
  Loader2,
  Search,
  Sparkles,
  SquareArrowOutUpRight,
  Type,
} from 'lucide-react';
import type { SearchMatch, SearchMatchReason } from '../types/events';

// Debounce before firing the backend query. 180 ms feels instant while
// still collapsing bursts of keypresses into one call — important
// because rank_matches iterates every cached entry.
const DEBOUNCE_MS = 180;

export function SearchPage() {
  const [query, setQuery] = useState('');
  const [results, setResults] = useState<SearchMatch[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const latestRequest = useRef(0);
  const navigate = useNavigate();

  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults([]);
      setLoading(false);
      setError(null);
      return;
    }

    const requestId = ++latestRequest.current;
    setLoading(true);
    const timer = setTimeout(async () => {
      try {
        const next = await invoke<SearchMatch[]>('search_all_folders', {
          query: trimmed,
        });
        // Drop the result if a newer query has already been kicked off —
        // otherwise a slow initial query can clobber a faster later one.
        if (requestId !== latestRequest.current) return;
        setResults(next);
        setError(null);
      } catch (err) {
        if (requestId !== latestRequest.current) return;
        setError(String(err));
        setResults([]);
      } finally {
        if (requestId === latestRequest.current) setLoading(false);
      }
    }, DEBOUNCE_MS);

    return () => clearTimeout(timer);
  }, [query]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <SearchHeader
        query={query}
        onQueryChange={setQuery}
        resultCount={results.length}
        loading={loading}
      />
      <div className="flex-1 overflow-hidden">
        {error ? (
          <div className="m-6 rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            Search failed: {error}
          </div>
        ) : query.trim() === '' ? (
          <EmptyPrompt />
        ) : results.length === 0 && !loading ? (
          <NoResults query={query} />
        ) : (
          <SearchResults
            results={results}
            onOpen={(match) =>
              navigate(
                `/folders/${encodeURIComponent(match.folder_path)}`,
              )
            }
          />
        )}
      </div>
    </div>
  );
}

// ─── Header ──────────────────────────────────────────────────────────────

function SearchHeader({
  query,
  onQueryChange,
  resultCount,
  loading,
}: {
  query: string;
  onQueryChange: (q: string) => void;
  resultCount: number;
  loading: boolean;
}) {
  return (
    <div className="border-b border-slate-200 bg-white px-6 py-5 dark:border-slate-800 dark:bg-slate-900">
      <h1 className="mb-3 flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
        <Sparkles className="h-6 w-6 text-purple-600 dark:text-purple-400" />
        Find anything
      </h1>
      <div className="relative">
        <Search className="pointer-events-none absolute left-4 top-1/2 h-5 w-5 -translate-y-1/2 text-slate-400" />
        <input
          autoFocus
          type="text"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          placeholder="Filename, column name, or text inside a document…"
          className="w-full rounded-xl border border-slate-200 bg-white py-3 pl-12 pr-12 text-base text-slate-900 placeholder-slate-400 shadow-sm focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/30 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
        />
        {loading && (
          <Loader2 className="absolute right-4 top-1/2 h-4 w-4 -translate-y-1/2 animate-spin text-slate-400" />
        )}
      </div>
      {query.trim() !== '' && !loading && (
        <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
          {resultCount === 0
            ? 'No matches yet'
            : `${resultCount} ${resultCount === 1 ? 'match' : 'matches'}`}
        </p>
      )}
    </div>
  );
}

// ─── Empty / no-results states ──────────────────────────────────────────

function EmptyPrompt() {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="max-w-lg text-center">
        <Search
          className="mx-auto mb-4 h-12 w-12 text-slate-300 dark:text-slate-600"
          strokeWidth={1.5}
        />
        <h2 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-50">
          Search across every folder you watch
        </h2>
        <p className="mb-6 text-sm text-slate-600 dark:text-slate-400">
          Type a filename, a column name (for spreadsheets and CSVs), or a word
          that appears inside a document. Sery Link searches every file it has
          scanned — fully offline, fully private.
        </p>
        <div className="grid grid-cols-1 gap-3 text-left text-sm sm:grid-cols-3">
          <HintCard
            icon={<Type className="h-4 w-4" />}
            title="Filename"
            example="quarterly-report"
          />
          <HintCard
            icon={<Columns3 className="h-4 w-4" />}
            title="Column name"
            example="customer_id"
          />
          <HintCard
            icon={<FileText className="h-4 w-4" />}
            title="Inside text"
            example="Anthropic"
          />
        </div>
      </div>
    </div>
  );
}

function HintCard({
  icon,
  title,
  example,
}: {
  icon: React.ReactNode;
  title: string;
  example: string;
}) {
  return (
    <div className="rounded-lg border border-slate-200 bg-white p-3 dark:border-slate-700 dark:bg-slate-800">
      <div className="mb-1 flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {icon}
        {title}
      </div>
      <code className="text-xs text-slate-700 dark:text-slate-200">
        {example}
      </code>
    </div>
  );
}

function NoResults({ query }: { query: string }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="max-w-md text-center">
        <Search
          className="mx-auto mb-4 h-10 w-10 text-slate-300 dark:text-slate-600"
          strokeWidth={1.5}
        />
        <h2 className="mb-1 text-base font-semibold text-slate-900 dark:text-slate-50">
          No matches for <span className="font-mono">{query}</span>
        </h2>
        <p className="text-sm text-slate-600 dark:text-slate-400">
          Try a shorter query, a column name, or a word from inside a document.
          Files that haven't been scanned yet won't appear — visit a folder
          once to add it to the search index.
        </p>
      </div>
    </div>
  );
}

// ─── Results list (virtualized) ─────────────────────────────────────────

function SearchResults({
  results,
  onOpen,
}: {
  results: SearchMatch[];
  onOpen: (match: SearchMatch) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);

  // Variable heights — one-reason rows vs three-reason rows differ by
  // a few line heights. Virtualizer auto-measures on mount via
  // measureElement so we just need a reasonable initial estimate.
  const virtualizer = useVirtualizer({
    count: results.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 96,
    overscan: 8,
    getItemKey: (index) =>
      `${results[index]?.folder_path ?? ''}::${results[index]?.relative_path ?? ''}`,
  });

  const items = virtualizer.getVirtualItems();

  return (
    <div ref={scrollRef} className="h-full overflow-y-auto px-6 py-4">
      <div
        style={{
          height: `${virtualizer.getTotalSize()}px`,
          position: 'relative',
          width: '100%',
        }}
      >
        {items.map((v) => {
          const match = results[v.index];
          if (!match) return null;
          return (
            <div
              key={v.key}
              data-index={v.index}
              ref={virtualizer.measureElement}
              style={{
                position: 'absolute',
                top: 0,
                left: 0,
                width: '100%',
                transform: `translateY(${v.start}px)`,
                paddingBottom: '8px',
              }}
            >
              <ResultCard match={match} onOpen={() => onOpen(match)} />
            </div>
          );
        })}
      </div>
    </div>
  );
}

function ResultCard({
  match,
  onOpen,
}: {
  match: SearchMatch;
  onOpen: () => void;
}) {
  const icon = useMemo(() => {
    const docExts = ['docx', 'pptx', 'html', 'htm', 'ipynb', 'pdf'];
    return docExts.includes(match.file_format.toLowerCase()) ? (
      <FileText className="h-5 w-5 text-slate-500 dark:text-slate-400" />
    ) : (
      <Database className="h-5 w-5 text-purple-600 dark:text-purple-400" />
    );
  }, [match.file_format]);

  const folderName = useMemo(() => {
    const parts = match.folder_path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] || match.folder_path;
  }, [match.folder_path]);

  return (
    <button
      onClick={onOpen}
      className="group block w-full rounded-xl border border-slate-200 bg-white p-4 text-left transition-all hover:border-purple-300 hover:shadow-sm dark:border-slate-800 dark:bg-slate-900 dark:hover:border-purple-700"
    >
      <div className="flex items-start gap-3">
        <div className="mt-0.5 flex-shrink-0">{icon}</div>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-2">
            <h3 className="truncate text-sm font-semibold text-slate-900 dark:text-slate-50">
              {match.relative_path}
            </h3>
            <span className="shrink-0 text-[10px] uppercase text-slate-500 dark:text-slate-400">
              {match.file_format}
            </span>
          </div>
          <div className="mt-0.5 flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400">
            <FolderIcon className="h-3 w-3" strokeWidth={1.5} />
            <span className="truncate" title={match.folder_path}>
              {folderName}
            </span>
            <span className="text-slate-300 dark:text-slate-600">·</span>
            <span>{formatBytes(match.size_bytes)}</span>
            {match.row_count_estimate !== null &&
              match.row_count_estimate !== undefined && (
                <>
                  <span className="text-slate-300 dark:text-slate-600">·</span>
                  <span>
                    {match.row_count_estimate.toLocaleString()} rows
                  </span>
                </>
              )}
            {match.column_count > 0 && (
              <>
                <span className="text-slate-300 dark:text-slate-600">·</span>
                <span>
                  {match.column_count} col
                  {match.column_count === 1 ? '' : 's'}
                </span>
              </>
            )}
          </div>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {match.match_reasons.map((reason, i) => (
              <ReasonBadge key={i} reason={reason} />
            ))}
          </div>
        </div>
        <SquareArrowOutUpRight
          className="mt-1 h-4 w-4 flex-shrink-0 text-slate-300 transition group-hover:text-purple-500 dark:text-slate-600"
          strokeWidth={1.5}
        />
      </div>
    </button>
  );
}

function ReasonBadge({ reason }: { reason: SearchMatchReason }) {
  switch (reason.kind) {
    case 'filename':
      return (
        <span className="inline-flex items-center gap-1 rounded-md bg-slate-100 px-2 py-0.5 text-[11px] font-medium text-slate-700 dark:bg-slate-800 dark:text-slate-200">
          <Type className="h-3 w-3" />
          filename
        </span>
      );
    case 'column':
      return (
        <span className="inline-flex items-center gap-1 rounded-md bg-purple-50 px-2 py-0.5 text-[11px] font-medium text-purple-700 dark:bg-purple-900/40 dark:text-purple-200">
          <Columns3 className="h-3 w-3" />
          column:{' '}
          <code className="rounded bg-white/60 px-1 font-mono dark:bg-black/20">
            {reason.name}
          </code>
          <span className="text-purple-400 dark:text-purple-500">
            {reason.col_type}
          </span>
        </span>
      );
    case 'content':
      return (
        <span className="inline-flex max-w-full items-start gap-1 rounded-md bg-amber-50 px-2 py-0.5 text-[11px] font-medium text-amber-800 dark:bg-amber-900/30 dark:text-amber-200">
          <FileText className="h-3 w-3 flex-shrink-0 translate-y-[1px]" />
          <span className="truncate">
            “{reason.snippet}”
          </span>
        </span>
      );
  }
}

// ─── Helpers ───────────────────────────────────────────────────────────

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(
    Math.floor(Math.log(bytes) / Math.log(1024)),
    units.length - 1,
  );
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}
