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
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import {
  AlertCircle,
  Columns3,
  Database,
  FileText,
  Folder as FolderIcon,
  FolderPlus,
  Loader2,
  Search,
  Sparkles,
  SquareArrowOutUpRight,
  Type,
  X,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import type {
  AgentConfig,
  SearchMatch,
  SearchMatchReason,
} from '../types/events';

// Debounce before firing the backend query. 180 ms feels instant while
// still collapsing bursts of keypresses into one call — important
// because rank_matches iterates every cached entry.
const DEBOUNCE_MS = 180;

export function SearchPage() {
  // Query + results live in the global store so switching tabs and
  // coming back doesn't wipe the user's search — it felt broken that
  // typing a long query, clicking Folders, and returning cleared
  // everything. `loading` and `error` stay local since they're only
  // meaningful while this page is mounted.
  const {
    searchQuery,
    searchResults,
    setSearchQuery,
    setSearchResults,
    config,
    scansInFlight,
    setConfig,
    authenticated,
  } = useAgentStore();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const latestRequest = useRef(0);
  const navigate = useNavigate();
  const toast = useToast();

  // Cloud-upsell card dismissal — closes I5 from UI_AUDIT_2026_05.md.
  // Local-only users had no surface explaining the cloud workspace
  // upgrade existed; conversion can't happen if the upsell is invisible.
  // The card is one-time-dismissible via localStorage so it doesn't nag.
  const [upsellDismissed, setUpsellDismissed] = useState(() => {
    try {
      return window.localStorage.getItem('sery.upsell.cloud.dismissed') === '1';
    } catch {
      return false;
    }
  });
  const dismissUpsell = () => {
    setUpsellDismissed(true);
    try {
      window.localStorage.setItem('sery.upsell.cloud.dismissed', '1');
    } catch {
      // localStorage failure is fine; the card stays dismissed for the session.
    }
  };
  const showUpsell =
    !authenticated &&
    !upsellDismissed &&
    searchResults.length > 0;

  // Is any folder currently being scanned? Used to nudge the user
  // that zero-results might just mean "not indexed yet."
  const indexingProgress = useMemo(() => {
    const scans = Object.values(scansInFlight);
    if (scans.length === 0) return null;
    return {
      folders: scans.length,
      processed: scans.reduce((sum, s) => sum + s.current, 0),
      total: scans.reduce((sum, s) => sum + s.total, 0),
    };
  }, [scansInFlight]);

  const hasFolders = (config?.watched_folders.length ?? 0) > 0;

  const addFolder = async () => {
    try {
      let defaultPath: string | undefined;
      try {
        defaultPath = await documentDir();
      } catch {
        defaultPath = undefined;
      }
      const selected = await openDialog({
        directory: true,
        multiple: false,
        defaultPath,
      });
      if (typeof selected !== 'string') return;
      await invoke('add_watched_folder', { path: selected, recursive: true });
      const updated = await invoke<AgentConfig>('get_config');
      setConfig(updated);
      toast.success('Folder added');
      invoke('rescan_folder', { folderPath: selected }).catch((err) => {
        console.error('Initial scan failed:', err);
      });
    } catch (err) {
      toast.error(`Couldn't add folder: ${err}`);
    }
  };

  useEffect(() => {
    const trimmed = searchQuery.trim();
    if (!trimmed) {
      setSearchResults([]);
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
        setSearchResults(next);
        setError(null);
      } catch (err) {
        if (requestId !== latestRequest.current) return;
        setError(String(err));
        setSearchResults([]);
      } finally {
        if (requestId === latestRequest.current) setLoading(false);
      }
    }, DEBOUNCE_MS);

    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [searchQuery]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <SearchHeader
        query={searchQuery}
        onQueryChange={setSearchQuery}
        resultCount={searchResults.length}
        loading={loading}
        indexingProgress={indexingProgress}
      />
      <div className="flex-1 overflow-hidden">
        {error ? (
          <div className="m-6 rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            Search failed: {error}
          </div>
        ) : !hasFolders ? (
          <NoFoldersEmpty onAddFolder={addFolder} />
        ) : searchQuery.trim() === '' ? (
          <EmptyPrompt />
        ) : searchResults.length === 0 && !loading ? (
          <NoResults
            query={searchQuery}
            indexing={indexingProgress !== null}
          />
        ) : (
          <div className="flex h-full flex-col">
            {showUpsell && <CloudUpsellCard onDismiss={dismissUpsell} />}
            <div className="flex-1 overflow-hidden">
              <SearchResults
                results={searchResults}
                onOpen={(match) =>
                  // Navigate to the FILE detail (new drill-down level), not
                  // the containing folder. The file route encodes both the
                  // folder path and the relative file path so the file
                  // detail page can find its cached row without ambiguity.
                  navigate(
                    `/folders/${encodeURIComponent(match.folder_path)}/files/${encodeURIComponent(match.relative_path)}`,
                  )
                }
              />
            </div>
          </div>
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
  indexingProgress,
}: {
  query: string;
  onQueryChange: (q: string) => void;
  resultCount: number;
  loading: boolean;
  indexingProgress: IndexingProgress | null;
}) {
  return (
    <div className="border-b border-slate-200 bg-white px-6 py-5 dark:border-slate-800 dark:bg-slate-900">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
          <Sparkles className="h-6 w-6 text-purple-600 dark:text-purple-400" />
          Find anything
        </h1>
        {indexingProgress && <IndexingPill progress={indexingProgress} />}
      </div>
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

type IndexingProgress = { folders: number; processed: number; total: number };

function IndexingPill({ progress }: { progress: IndexingProgress }) {
  // total can lag the true count early in a scan. Show raw processed
  // count until we have a total, then show "N of M".
  const label =
    progress.total > 0
      ? `Indexing… ${progress.processed.toLocaleString()} of ${progress.total.toLocaleString()} files`
      : `Indexing… ${progress.processed.toLocaleString()} files so far`;
  return (
    <span className="inline-flex items-center gap-1.5 rounded-full border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-800 dark:border-amber-900/50 dark:bg-amber-950/30 dark:text-amber-200">
      <Loader2 className="h-3 w-3 animate-spin" />
      {label}
    </span>
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

function NoResults({
  query,
  indexing,
}: {
  query: string;
  indexing: boolean;
}) {
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
        {indexing ? (
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Sery is still indexing your folders — results will appear here as
            more files are scanned. Worth retrying in a moment.
          </p>
        ) : (
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Try a shorter query, a column name, or a word from inside a
            document.
          </p>
        )}
      </div>
    </div>
  );
}

function NoFoldersEmpty({ onAddFolder }: { onAddFolder: () => void }) {
  return (
    <div className="flex h-full items-center justify-center p-8">
      <div className="max-w-md text-center">
        <FolderPlus
          className="mx-auto mb-4 h-12 w-12 text-slate-300 dark:text-slate-600"
          strokeWidth={1.5}
        />
        <h2 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-50">
          Add a folder to start searching
        </h2>
        <p className="mb-6 text-sm text-slate-600 dark:text-slate-400">
          Sery indexes every CSV, spreadsheet, and document in the folder
          you pick — locally, with nothing uploaded. Once it's indexed you
          can search by filename, column name, or text inside a document.
        </p>
        <button
          onClick={onAddFolder}
          className="inline-flex items-center gap-2 rounded-lg bg-purple-600 px-5 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700"
        >
          <FolderPlus className="h-4 w-4" />
          Pick a folder
        </button>
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
  // Filename-only Drive matches have no folder_path and no cached
  // content to drill into — the row should look slightly muted
  // and not respond to clicks. The badge already explains why.
  const isSkipped = match.match_reasons.some((r) => r.kind === 'skipped_drive');

  const icon = useMemo(() => {
    const docExts = ['docx', 'pptx', 'html', 'htm', 'ipynb', 'pdf'];
    return docExts.includes(match.file_format.toLowerCase()) ? (
      <FileText className="h-5 w-5 text-slate-500 dark:text-slate-400" />
    ) : (
      <Database className="h-5 w-5 text-purple-600 dark:text-purple-400" />
    );
  }, [match.file_format]);

  const folderName = useMemo(() => {
    if (isSkipped) return 'Google Drive';
    const parts = match.folder_path.split(/[\\/]/).filter(Boolean);
    return parts[parts.length - 1] || match.folder_path;
  }, [match.folder_path, isSkipped]);

  const Tag: 'button' | 'div' = isSkipped ? 'div' : 'button';
  const interactiveProps = isSkipped
    ? {}
    : { onClick: onOpen, type: 'button' as const };

  return (
    <Tag
      {...interactiveProps}
      className={`group block w-full rounded-xl border p-4 text-left transition-all ${
        isSkipped
          ? 'cursor-default border-slate-200 bg-slate-50/60 dark:border-slate-800 dark:bg-slate-900/40'
          : 'border-slate-200 bg-white hover:border-purple-300 hover:shadow-sm dark:border-slate-800 dark:bg-slate-900 dark:hover:border-purple-700'
      }`}
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
        {!isSkipped && (
          <SquareArrowOutUpRight
            className="mt-1 h-4 w-4 flex-shrink-0 text-slate-300 transition group-hover:text-purple-500 dark:text-slate-600"
            strokeWidth={1.5}
          />
        )}
      </div>
    </Tag>
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
    case 'skipped_drive': {
      // Honest signal: Drive knows about the file but Sery didn't
      // cache its bytes. We tell the user WHY so they can decide
      // whether to bump a setting / open the file in Drive natively
      // / etc. Yellow tone matches the "warning, but not error"
      // semantics of Settings → Storage's skipped card.
      const label = (() => {
        switch (reason.reason) {
          case 'native_unexportable':
            return 'Google Doc / Form / Drawing — not indexed';
          case 'unsupported_extension':
            return 'Filename only — file type not indexed';
          case 'too_large':
            return 'Filename only — over 1 GiB cap';
          case 'download_failed':
            return 'Filename only — download failed, will retry';
        }
      })();
      return (
        <span className="inline-flex items-center gap-1 rounded-md bg-amber-50 px-2 py-0.5 text-[11px] font-medium text-amber-800 dark:bg-amber-900/30 dark:text-amber-200">
          <AlertCircle className="h-3 w-3" />
          {label}
        </span>
      );
    }
  }
}

// ─── Cloud upsell ──────────────────────────────────────────────────────

/* CloudUpsellCard — surfaces the Sery Cloud workspace ($19/mo)
 * to local-only users who've successfully searched. The audit
 * (UI_AUDIT_2026_05.md, item I5) flagged that the upsell never
 * appeared in normal use; conversion can't happen if users don't
 * know the path exists. One-time dismissible via localStorage,
 * tracked by the parent component.
 */
function CloudUpsellCard({ onDismiss }: { onDismiss: () => void }) {
  return (
    <div className="mx-6 mt-4 mb-2 flex items-start gap-3 rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
      <Sparkles className="mt-0.5 h-4 w-4 flex-shrink-0 text-purple-600 dark:text-purple-300" />
      <div className="flex-1 text-xs text-slate-700 dark:text-slate-300">
        <span className="font-medium text-slate-900 dark:text-slate-100">
          Want AI chat across your sources?
        </span>{' '}
        Connect to Sery Cloud — $19/mo unlocks AI chat at
        app.sery.ai/chat plus cross-machine search across everything you index.{' '}
        <a
          href="https://app.sery.ai/settings/workspace-keys"
          target="_blank"
          rel="noopener noreferrer"
          className="font-medium text-purple-700 underline hover:text-purple-900 dark:text-purple-300 dark:hover:text-purple-100"
        >
          Get a workspace key →
        </a>
      </div>
      <button
        onClick={onDismiss}
        className="flex-shrink-0 rounded p-0.5 text-slate-400 hover:bg-purple-100 hover:text-slate-600 dark:hover:bg-purple-900/40"
        aria-label="Dismiss"
        title="Dismiss"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  );
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
