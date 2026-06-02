// Source detail page — /folders/:folderId (the URL param is a
// path or URL the path-keyed scanner indexes; route prefix kept
// for now while cache-and-scan kinds get their own resolver).
//
// Reached by clicking a row on the Sources sidebar. Shows every
// dataset Sery indexed inside this folder, with filename search,
// per-dataset schema preview, sample-row preview, and a "locate in
// Finder" shortcut.
//
// Deliberately useful WITHOUT a cloud connection — we render entirely
// from the local scan cache (~/.sery/scan_cache.db) for instant paint,
// then refresh in the background. Per-file `dataset_scanned` events
// stream rows in as they land so even a first-time scan of a huge
// folder shows progress instead of a blank spinner. No workspace_id
// required, no cloud round-trips.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowDown,
  ArrowUpDown,
  FileSpreadsheet,
  FileText,
  Loader2,
  RefreshCw,
  Search,
  SquareArrowOutUpRight,
  Table2,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import type {
  FolderFormatFilter,
  FolderRecencyFilter,
  FolderSort,
} from '../stores/agentStore';
import { useToast } from './Toast';
import {
  classifySource,
  filenameFromUrl,
  isRemoteUrl,
  sourceKindLabel,
} from '../utils/url';
import { SourceIcon } from './SourceIcon';
import {
  EVENT_NAMES,
  type DatasetScannedPayload,
  type ScanComplete,
  type ScanProgress,
  type ScanWalkProgress,
  type DatasetMetadataPayload as DatasetMetadata,
} from '../types/events';

/** Which phase of the two-pass scan we're rendering.
 *
 *  - `idle`: no scan running; the file list reflects the cache.
 *  - `walking`: pass 1 in progress — running file count, no total yet.
 *  - `extracting`: pass 2 in progress — accurate `current/total` for
 *    files that need content extraction (cache hits + shallow-tier files
 *    aren't counted here because they finished in pass 1). */
type ScanState =
  | { kind: 'idle' }
  | { kind: 'walking'; discovered: number }
  | { kind: 'extracting'; current: number; total: number };

export function FolderDetail() {
  const { folderId } = useParams<{ folderId: string }>();
  const toast = useToast();
  const { config } = useAgentStore();
  // Per-folder filter input lifted to the store so it survives
  // navigation away and back. Keyed by folderPath so different
  // folders' filters don't clobber each other.
  const folderSearchMap = useAgentStore((s) => s.folderSearch);
  const setFolderSearch = useAgentStore((s) => s.setFolderSearch);
  const folderFormatMap = useAgentStore((s) => s.folderFormat);
  const setFolderFormat = useAgentStore((s) => s.setFolderFormat);
  const folderRecencyMap = useAgentStore((s) => s.folderRecency);
  const setFolderRecency = useAgentStore((s) => s.setFolderRecency);
  const folderSortMap = useAgentStore((s) => s.folderSort);
  const setFolderSort = useAgentStore((s) => s.setFolderSort);

  const folderPath = folderId ? decodeURIComponent(folderId) : '';
  const folder = config?.watched_folders.find((f) => f.path === folderPath);
  const search = folderSearchMap[folderPath] ?? '';
  const setSearch = (q: string) => setFolderSearch(folderPath, q);
  // Format chip selection. Default 'all' = no format filter.
  const formatFilter = folderFormatMap[folderPath] ?? 'all';
  const setFormatFilter = (v: FolderFormatFilter) => setFolderFormat(folderPath, v);
  const recencyFilter = folderRecencyMap[folderPath] ?? 'any';
  const setRecencyFilter = (v: FolderRecencyFilter) => setFolderRecency(folderPath, v);
  const sort = folderSortMap[folderPath] ?? 'name';
  const setSort = (v: FolderSort) => setFolderSort(folderPath, v);

  // Map<relative_path, DatasetMetadata> — keeps incremental updates O(1)
  // by upserting on each dataset_scanned event. Converted to an array
  // via the `datasets` memo below.
  const [datasetMap, setDatasetMap] = useState<Map<string, DatasetMetadata>>(
    new Map(),
  );
  const [scanState, setScanState] = useState<ScanState>({ kind: 'idle' });
  // True while the initial get_cached_folder_metadata call is in flight.
  // Suppresses the "No indexable files found" empty state during cache load
  // so the user doesn't see a flicker on every revisit after restart.
  const [cacheLoading, setCacheLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const initialLoadRef = useRef(false);
  const scanCompleteTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const datasets = useMemo(
    () =>
      Array.from(datasetMap.values()).sort((a, b) =>
        a.relative_path.localeCompare(b.relative_path),
      ),
    [datasetMap],
  );

  const startRescan = async () => {
    // Optimistically enter the walk phase so the UI shows *something*
    // before the first scan_walk_progress event arrives. The first
    // event will replace the discovered count almost immediately.
    setScanState({ kind: 'walking', discovered: 0 });
    setError(null);
    try {
      await invoke('rescan_folder', { folderPath });
    } catch (err) {
      setError(String(err));
      setScanState({ kind: 'idle' });
    }
  };

  // Mount: paint from cache. Only kick off a rescan automatically when
  // the cache is empty (first visit) — otherwise the cached rows are
  // the source of truth and the file watcher keeps them fresh as files
  // change. If the user wants a forced refresh they can click Rescan.
  useEffect(() => {
    if (!folderPath) return;
    let cancelled = false;
    initialLoadRef.current = true;

    setCacheLoading(true);
    (async () => {
      let hadCache = false;
      try {
        const cached = await invoke<DatasetMetadata[]>(
          'get_cached_folder_metadata',
          { folderPath },
        );
        if (cancelled) return;
        const map = new Map<string, DatasetMetadata>();
        for (const d of cached) map.set(d.relative_path, d);
        setDatasetMap(map);
        hadCache = cached.length > 0;
      } catch (err) {
        console.error('Failed to load cached folder metadata:', err);
      }
      if (cancelled) return;
      setCacheLoading(false);
      if (!hadCache) {
        // First visit to this folder — nothing cached yet, so trigger
        // the initial scan so the user sees their files.
        void startRescan();
      }
    })();

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [folderPath]);

  // Subscribe to scanner events for this folder. Each listener filters
  // by `folder` so events from the watcher or other folders' rescans
  // don't leak into this view.
  useEffect(() => {
    if (!folderPath) return;
    const unlisteners: Array<() => void> = [];

    // Pass-1 walk progress. Drives the running discovered count.
    // Doesn't tell us the total — the walk is still in progress when
    // these fire — so the UI shows "Listing files: 1247 found" without
    // a percent bar.
    void listen<ScanWalkProgress>(EVENT_NAMES.SCAN_WALK_PROGRESS, (evt) => {
      if (evt.payload.folder !== folderPath) return;
      setScanState((prev) =>
        prev.kind === 'extracting'
          ? prev // pass 2 already started; ignore late walk events
          : { kind: 'walking', discovered: evt.payload.discovered },
      );
    }).then((off) => unlisteners.push(off));

    // Per-file dataset events. Both passes upsert by relative_path —
    // shallow inserts the placeholder row, content replaces it with
    // the hydrated record. We deliberately don't drive scanState from
    // these events anymore; pass-1 events would otherwise fight the
    // pass-2 progress bar (their index/total numbers are running
    // discovery counts, not extraction percentages).
    void listen<DatasetScannedPayload>(EVENT_NAMES.DATASET_SCANNED, (evt) => {
      if (evt.payload.folder !== folderPath) return;
      const d = evt.payload.dataset;
      setDatasetMap((prev) => {
        const next = new Map(prev);
        next.set(d.relative_path, d);
        return next;
      });
    }).then((off) => unlisteners.push(off));

    // Pass-2 (content extraction) progress. First event flips the UI
    // from "Listing files…" to "Indexing content N of T".
    void listen<ScanProgress>(EVENT_NAMES.SCAN_PROGRESS, (evt) => {
      if (evt.payload.folder !== folderPath) return;
      setScanState({
        kind: 'extracting',
        current: evt.payload.current,
        total: evt.payload.total,
      });
    }).then((off) => unlisteners.push(off));

    void listen<ScanComplete>(EVENT_NAMES.SCAN_COMPLETE, (evt) => {
      if (evt.payload.folder !== folderPath) return;
      // Debounce: rapid-fire scan_complete events (e.g. two concurrent scans)
      // collapse into a single cache refresh after 200 ms.
      if (scanCompleteTimerRef.current) clearTimeout(scanCompleteTimerRef.current);
      scanCompleteTimerRef.current = setTimeout(async () => {
        scanCompleteTimerRef.current = null;
        try {
          const fresh = await invoke<DatasetMetadata[]>(
            'get_cached_folder_metadata',
            { folderPath },
          );
          const map = new Map<string, DatasetMetadata>();
          for (const d of fresh) map.set(d.relative_path, d);
          setDatasetMap(map);
        } catch {
          /* keep what we have */
        }
        setScanState({ kind: 'idle' });
      }, 200);
    }).then((off) => unlisteners.push(off));

    return () => {
      for (const off of unlisteners) off();
      if (scanCompleteTimerRef.current) clearTimeout(scanCompleteTimerRef.current);
    };
  }, [folderPath]);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    // Recency cutoff is computed once per render rather than per
    // dataset — relative-time math is cheap but we still avoid the
    // per-row work when `any` is selected.
    const cutoffMs = (() => {
      switch (recencyFilter) {
        case '24h':
          return Date.now() - 24 * 60 * 60 * 1000;
        case '7d':
          return Date.now() - 7 * 24 * 60 * 60 * 1000;
        case '30d':
          return Date.now() - 30 * 24 * 60 * 60 * 1000;
        default:
          return null;
      }
    })();
    const matched = datasets.filter((d) => {
      if (q && !d.relative_path.toLowerCase().includes(q)) return false;
      if (formatFilter !== 'all' && !matchesFormat(formatFilter, d.file_format)) {
        return false;
      }
      if (cutoffMs !== null) {
        const t = Date.parse(d.last_modified);
        if (Number.isNaN(t) || t < cutoffMs) return false;
      }
      return true;
    });
    // Sort applied after filter so the user only pays the comparison
    // cost on the visible subset. .slice() avoids mutating the
    // datasets array (which datasetMap referrers expect to keep its
    // identity for memoization further down the tree).
    const sorted = matched.slice();
    switch (sort) {
      case 'modified-desc':
        sorted.sort((a, b) => Date.parse(b.last_modified) - Date.parse(a.last_modified));
        break;
      case 'size-desc':
        sorted.sort((a, b) => b.size_bytes - a.size_bytes);
        break;
      case 'name':
      default:
        sorted.sort((a, b) => a.relative_path.localeCompare(b.relative_path));
        break;
    }
    return sorted;
  }, [datasets, search, formatFilter, recencyFilter, sort]);

  const totals = useMemo(() => {
    if (datasets.length === 0) return null;
    const tabular = datasets.filter((d) => !isDocumentFormat(d.file_format));
    const docs = datasets.filter((d) => isDocumentFormat(d.file_format));
    const bytes = datasets.reduce((s, d) => s + d.size_bytes, 0);
    return { total: datasets.length, tabular: tabular.length, docs: docs.length, bytes };
  }, [datasets]);

  const rescan = async () => {
    try {
      await startRescan();
      toast.success('Rescanning in the background…');
    } catch (err) {
      toast.error(`Rescan failed: ${err}`);
    }
  };

  const revealFolder = async () => {
    try {
      await invoke('reveal_in_finder', { path: folderPath });
    } catch (err) {
      toast.error(`Couldn't open: ${err}`);
    }
  };

  if (!folder) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="flex-1 overflow-y-auto p-6">
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">
              This source isn't connected.
            </p>
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              {folderPath || '(no path)'}
            </p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <SourceIcon kind={classifySource(folderPath)} size="sm" />
              <span className="truncate">
                {classifySource(folderPath) === 'gdrive'
                  ? 'My Drive'
                  : isRemoteUrl(folderPath)
                    ? filenameFromUrl(folderPath)
                    : folderBasename(folderPath)}
              </span>
            </h1>
            <p
              className="mt-1 truncate text-sm text-slate-500 dark:text-slate-400"
              title={folderPath}
            >
              <span className="font-medium text-slate-600 dark:text-slate-300">
                {sourceKindLabel(classifySource(folderPath))}
              </span>
              {classifySource(folderPath) === 'gdrive' ? '' : ` · ${folderPath}`}
            </p>
            {totals && (
              <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
                {totals.total} {totals.total === 1 ? 'file' : 'files'}
                {' · '}
                {totals.tabular} tabular
                {totals.docs > 0 && ` · ${totals.docs} document${totals.docs === 1 ? '' : 's'}`}
                {' · '}
                {formatBytes(totals.bytes)}
              </p>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              onClick={rescan}
              disabled={scanState.kind !== 'idle'}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              <RefreshCw
                className={`h-3.5 w-3.5 ${scanState.kind !== 'idle' ? 'animate-spin' : ''}`}
              />
              {scanState.kind !== 'idle' ? 'Scanning…' : 'Rescan'}
            </button>
            {classifySource(folderPath) === 'local' && (
              <button
                onClick={revealFolder}
                className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                <SquareArrowOutUpRight className="h-3.5 w-3.5" />
                {openFolderLabel()}
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Search + filter chips + status stays as a non-scrolling
          header so the virtualized list below gets a stable scroll
          container. */}
      <div className="border-b border-slate-200 bg-white px-6 py-3 dark:border-slate-800 dark:bg-slate-900">
        <div className="relative">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input
            type="text"
            autoCorrect="off"
            autoCapitalize="off"
            spellCheck={false}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter files by name…"
            className="w-full rounded-lg border border-slate-200 bg-white py-2 pl-9 pr-3 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-100 dark:placeholder-slate-500"
          />
        </div>

        {/* Format + recency chip rows. Chips beat dropdowns here
            because the user can see all options at once and one
            click flips between them — common case is "show me CSVs
            from this week" which is two clicks total. */}
        <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
          <span className="text-[11px] uppercase tracking-wide text-slate-400 dark:text-slate-500">
            Format
          </span>
          {(
            [
              ['all', 'All'],
              ['csv', 'CSV'],
              ['parquet', 'Parquet'],
              ['excel', 'Excel'],
              ['documents', 'Docs'],
              ['other', 'Other'],
            ] as Array<[FolderFormatFilter, string]>
          ).map(([value, label]) => (
            <FilterChip
              key={value}
              active={formatFilter === value}
              onClick={() => setFormatFilter(value)}
            >
              {label}
            </FilterChip>
          ))}
          <span className="ml-3 text-[11px] uppercase tracking-wide text-slate-400 dark:text-slate-500">
            Modified
          </span>
          {(
            [
              ['any', 'Any time'],
              ['24h', 'Last 24h'],
              ['7d', 'Last 7d'],
              ['30d', 'Last 30d'],
            ] as Array<[FolderRecencyFilter, string]>
          ).map(([value, label]) => (
            <FilterChip
              key={value}
              active={recencyFilter === value}
              onClick={() => setRecencyFilter(value)}
            >
              {label}
            </FilterChip>
          ))}
        </div>

        {(search || formatFilter !== 'all' || recencyFilter !== 'any') &&
          datasets.length > 0 && (
            <div className="mt-1.5 text-xs text-slate-500 dark:text-slate-400">
              {filtered.length} of {datasets.length} match
            </div>
          )}

        {scanState.kind !== 'idle' && (
          // Subtle inline indicator. The user already sees the file
          // list streaming in row-by-row via the dataset_scanned
          // events, so we don't need a big banner — a one-line
          // status with running counts is enough to signal that
          // background work is happening.
          //
          // Also reads less alarming, which matters because the
          // file watcher routinely fires this on cache-empty visits
          // even though there's nothing actually wrong.
          <div className="mt-2 inline-flex items-center gap-1.5 text-xs text-slate-500 dark:text-slate-400">
            <Loader2 className="h-3 w-3 animate-spin" />
            {scanState.kind === 'walking' ? (
              <span>
                Indexing in background
                {scanState.discovered > 0 && ` · ${scanState.discovered} files found so far`}
              </span>
            ) : (
              <span>
                {scanState.total > 0
                  ? `Indexing in background · ${scanState.current} of ${scanState.total}`
                  : 'Indexing in background'}
              </span>
            )}
          </div>
        )}

        {error && (
          <div className="mt-3 rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            Couldn't scan folder: {error}
          </div>
        )}
      </div>

      <VirtualizedDatasetList
        filtered={filtered}
        scanRunning={scanState.kind !== 'idle'}
        cacheLoading={cacheLoading}
        search={search}
        folderPath={folderPath}
        sort={sort}
        setSort={setSort}
      />
    </div>
  );
}

// ─── Datagrid ─────────────────────────────────────────────────────────────

// Columns: Name (flex) · Format · Rows · Cols · Size · Modified
const GRID_COLS = 'grid-cols-[1fr_4.5rem_5.5rem_3.5rem_5rem_6.5rem]';

function VirtualizedDatasetList({
  filtered,
  scanRunning,
  cacheLoading,
  search,
  folderPath,
  sort,
  setSort,
}: {
  filtered: DatasetMetadata[];
  scanRunning: boolean;
  cacheLoading: boolean;
  search: string;
  folderPath: string;
  sort: FolderSort;
  setSort: (v: FolderSort) => void;
}) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();

  const virtualizer = useVirtualizer({
    count: filtered.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 36,
    overscan: 12,
    getItemKey: (index) => filtered[index]?.relative_path ?? index,
  });

  const items = virtualizer.getVirtualItems();

  const openFile = (relativePath: string) => {
    navigate(
      `/folders/${encodeURIComponent(folderPath)}/files/${encodeURIComponent(relativePath)}`,
    );
  };

  const SortIcon = ({ col }: { col: FolderSort }) =>
    sort === col ? (
      <ArrowDown className="h-3 w-3" />
    ) : col === 'name' && sort !== 'name' ? (
      <ArrowUpDown className="h-3 w-3 opacity-30" />
    ) : (
      <ArrowUpDown className="h-3 w-3 opacity-30" />
    );

  // Column headers with sort controls
  const headerCell = (label: string, col: FolderSort | null, align: 'left' | 'right' = 'left') => {
    const base = `flex items-center gap-1 text-[11px] font-semibold uppercase tracking-wide select-none ${align === 'right' ? 'justify-end' : ''}`;
    if (!col) return <span className={`${base} text-slate-400 dark:text-slate-500`}>{label}</span>;
    return (
      <button
        onClick={() => setSort(col)}
        className={`${base} ${sort === col ? 'text-purple-600 dark:text-purple-400' : 'text-slate-400 hover:text-slate-600 dark:text-slate-500 dark:hover:text-slate-300'}`}
      >
        {align === 'right' && <SortIcon col={col} />}
        {label}
        {align === 'left' && <SortIcon col={col} />}
      </button>
    );
  };

  if (cacheLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <Loader2 className="h-5 w-5 animate-spin text-slate-400" />
      </div>
    );
  }

  if (!scanRunning && filtered.length === 0 && search === '') {
    return (
      <div className="flex-1 overflow-y-auto px-6 py-4">
        <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
          <p className="text-sm text-slate-600 dark:text-slate-400">No indexable files found in this folder.</p>
          <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
            Sery indexes parquet, csv, xlsx, xls, docx, pptx, pdf, html, and ipynb.
          </p>
        </div>
      </div>
    );
  }

  if (filtered.length === 0 && search !== '') {
    return (
      <div className="flex-1 overflow-y-auto px-6 py-4">
        <div className="rounded-lg border-2 border-dashed border-slate-300 p-6 text-center text-sm text-slate-500 dark:border-slate-700 dark:text-slate-400">
          No files match <span className="font-mono">{search}</span>.
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Sticky column headers */}
      <div className={`grid ${GRID_COLS} border-b border-slate-200 bg-white px-6 py-2 dark:border-slate-800 dark:bg-slate-900`}>
        {headerCell('Name', 'name')}
        {headerCell('Format', null)}
        {headerCell('Rows', null, 'right')}
        {headerCell('Cols', null, 'right')}
        {headerCell('Size', 'size-desc', 'right')}
        {headerCell('Modified', 'modified-desc', 'right')}
      </div>

      {/* Virtualized rows */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto">
        <div style={{ height: `${virtualizer.getTotalSize()}px`, position: 'relative', width: '100%' }}>
          {items.map((virtualRow) => {
            const d = filtered[virtualRow.index];
            if (!d) return null;
            return (
              <div
                key={virtualRow.key}
                data-index={virtualRow.index}
                ref={virtualizer.measureElement}
                style={{ position: 'absolute', top: 0, left: 0, width: '100%', transform: `translateY(${virtualRow.start}px)` }}
              >
                <DatasetRow dataset={d} onOpen={() => openFile(d.relative_path)} />
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ─── Grid row ─────────────────────────────────────────────────────────────

function DatasetRow({ dataset, onOpen }: { dataset: DatasetMetadata; onOpen: () => void }) {
  const isDoc = isDocumentFormat(dataset.file_format);
  const isDbTable = isDbTableFormat(dataset.file_format);
  return (
    <button
      onClick={onOpen}
      className={`group grid w-full ${GRID_COLS} items-center border-b border-slate-100 px-6 py-2 text-left transition-colors hover:bg-slate-50 dark:border-slate-800 dark:hover:bg-slate-800/50`}
    >
      <div className="flex min-w-0 items-center gap-2">
        {isDoc
          ? <FileText className="h-3.5 w-3.5 shrink-0 text-slate-400 dark:text-slate-500" />
          : isDbTable
            ? <Table2 className="h-3.5 w-3.5 shrink-0 text-blue-500 dark:text-blue-400" />
            : <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-emerald-500 dark:text-emerald-400" />
        }
        <span className="truncate text-sm text-slate-800 dark:text-slate-100">
          {dataset.relative_path}
        </span>
      </div>
      <span className="truncate text-right font-mono text-xs uppercase text-slate-500 dark:text-slate-400">
        {dataset.file_format}
      </span>
      <span className="text-right text-xs tabular-nums text-slate-500 dark:text-slate-400">
        {dataset.row_count_estimate !== null ? dataset.row_count_estimate.toLocaleString() : '—'}
      </span>
      <span className="text-right text-xs tabular-nums text-slate-500 dark:text-slate-400">
        {!isDoc && dataset.schema.length > 0 ? dataset.schema.length : '—'}
      </span>
      <span className="text-right text-xs tabular-nums text-slate-500 dark:text-slate-400">
        {formatBytes(dataset.size_bytes)}
      </span>
      <span className="text-right text-xs text-slate-400 dark:text-slate-500" title={dataset.last_modified}>
        {formatRelativeTime(dataset.last_modified)}
      </span>
    </button>
  );
}

/** Toggle pill used in the FolderDetail filter row. Active state is
 *  filled purple; inactive is a subtle outline. Same shape repeats
 *  across format + recency rows so the eye groups them as one
 *  control surface. */
function FilterChip({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`rounded-full border px-2.5 py-0.5 text-[11px] font-medium transition-colors ${
        active
          ? 'border-purple-500 bg-purple-100 text-purple-700 dark:border-purple-400 dark:bg-purple-900/40 dark:text-purple-200'
          : 'border-slate-200 bg-white text-slate-600 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800'
      }`}
    >
      {children}
    </button>
  );
}

// ─── Helpers ──────────────────────────────────────────────────────────────

const DOCUMENT_FORMATS = new Set(['docx', 'pptx', 'html', 'htm', 'ipynb', 'pdf']);
const DB_TABLE_FORMATS = new Set([
  'mysql', 'postgresql', 'mongodb', 'redis', 'sqlite', 'snowflake', 'clickhouse',
  'oracle', 'oracle-legacy', 'oracle-10g', 'db2', 'informix', 'saphana', 'teradata',
  'vertica', 'databricks', 'trino', 'hive', 'bigquery', 'cassandra', 'neo4j',
  'firebird', 'exasol', 'h2', 'kylin', 'access', 'dameng', 'kingbase', 'highgo',
  'vastbase', 'goldendb', 'oceanbase-oracle', 'gbase', 'sundb', 'yashandb',
  'tdengine', 'xugu', 'mongodb-jdbc', 'agent_db',
]);
const KNOWN_TABULAR = new Set(['csv', 'tsv', 'parquet', 'xlsx', 'xls']);

/** Test whether a dataset's `file_format` belongs to the chip's
 *  bucket. The grouping is intentionally chunky — users think in
 *  "spreadsheets" not "xlsx vs xls"; in "documents" not "pdf vs
 *  docx vs html". */
function matchesFormat(filter: FolderFormatFilter, fmt: string): boolean {
  const f = fmt.toLowerCase();
  switch (filter) {
    case 'all':
      return true;
    case 'csv':
      return f === 'csv' || f === 'tsv';
    case 'parquet':
      return f === 'parquet';
    case 'excel':
      return f === 'xlsx' || f === 'xls';
    case 'documents':
      return DOCUMENT_FORMATS.has(f);
    case 'other':
      return !KNOWN_TABULAR.has(f) && !DOCUMENT_FORMATS.has(f);
  }
}

function isDocumentFormat(fmt: string): boolean {
  return DOCUMENT_FORMATS.has(fmt.toLowerCase());
}

function isDbTableFormat(fmt: string): boolean {
  return DB_TABLE_FORMATS.has(fmt.toLowerCase());
}

function folderBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Platform-aware label for the "reveal in file manager" button.
 * macOS has Finder, Windows has Explorer, Linux has whatever the
 * user's environment runs; safest portable label there is "Open
 * folder."
 */
function openFolderLabel(): string {
  if (typeof navigator === 'undefined') return 'Open folder';
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'Open in Finder';
  if (ua.includes('win')) return 'Open in Explorer';
  return 'Open folder';
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return 'recently';
  const diff = Date.now() - then;
  if (diff < 0) return 'just now';
  const sec = Math.floor(diff / 1000);
  if (sec < 60) return `${sec}s ago`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d ago`;
  return new Date(iso).toLocaleDateString();
}
