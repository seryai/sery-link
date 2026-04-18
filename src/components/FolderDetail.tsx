// Folder detail page — /folders/:folderId
//
// Reached by clicking a folder card on the Folders tab. Shows every
// dataset Sery indexed inside this folder, with filename search,
// per-dataset schema preview, sample-row preview, and a "locate in
// Finder" shortcut.
//
// Deliberately useful WITHOUT a cloud connection — we scan via the
// local scanner on mount and render everything client-side. The only
// Tauri commands this page uses are `scan_folder` (fast, respects the
// mtime cache) and `reveal_in_finder` (OS-level reveal). No
// workspace_id required, no cloud round-trips.

import { useEffect, useMemo, useState } from 'react';
import { useParams, useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import {
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  Database,
  FileText,
  Folder as FolderIcon,
  Loader2,
  RefreshCw,
  Search,
  Sparkles,
  SquareArrowOutUpRight,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';

interface ColumnSchema {
  name: string;
  type: string;
  nullable: boolean;
}

interface DatasetMetadata {
  relative_path: string;
  file_format: string;
  size_bytes: number;
  row_count_estimate: number | null;
  schema: ColumnSchema[];
  last_modified: string;
  document_markdown?: string;
  sample_rows?: Record<string, unknown>[];
  samples_redacted: boolean;
}

export function FolderDetail() {
  const { folderId } = useParams<{ folderId: string }>();
  const navigate = useNavigate();
  const toast = useToast();
  const { config } = useAgentStore();

  const folderPath = folderId ? decodeURIComponent(folderId) : '';
  const folder = config?.watched_folders.find((f) => f.path === folderPath);

  const [datasets, setDatasets] = useState<DatasetMetadata[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState('');
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const loadDatasets = async () => {
    setLoading(true);
    setError(null);
    try {
      const result = await invoke<DatasetMetadata[]>('scan_folder', {
        folderPath,
      });
      setDatasets(result);
    } catch (err) {
      setError(String(err));
      setDatasets([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (!folderPath) return;
    void loadDatasets();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [folderPath]);

  const filtered = useMemo(() => {
    if (!datasets) return [];
    const q = search.trim().toLowerCase();
    if (!q) return datasets;
    return datasets.filter((d) => d.relative_path.toLowerCase().includes(q));
  }, [datasets, search]);

  const totals = useMemo(() => {
    if (!datasets) return null;
    const tabular = datasets.filter((d) => !isDocumentFormat(d.file_format));
    const docs = datasets.filter((d) => isDocumentFormat(d.file_format));
    const bytes = datasets.reduce((s, d) => s + d.size_bytes, 0);
    return { total: datasets.length, tabular: tabular.length, docs: docs.length, bytes };
  }, [datasets]);

  const rescan = async () => {
    try {
      await invoke('rescan_folder', { folderPath });
      toast.success('Rescanning in the background…');
      await loadDatasets();
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

  const revealDataset = async (relativePath: string) => {
    try {
      const full = `${folderPath.replace(/\/$/, '')}/${relativePath}`;
      await invoke('reveal_in_finder', { path: full });
    } catch (err) {
      toast.error(`Couldn't open: ${err}`);
    }
  };

  const openInAnalytics = () => {
    navigate(`/analytics/${encodeURIComponent(folderPath)}`);
  };

  if (!folder) {
    return (
      <div className="flex h-full flex-col overflow-hidden">
        <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
          <button
            onClick={() => navigate('/folders')}
            className="inline-flex items-center gap-2 text-sm text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
          >
            <ArrowLeft className="h-4 w-4" /> Back to folders
          </button>
        </div>
        <div className="flex-1 overflow-y-auto p-6">
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">
              This folder isn't being watched.
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
        <button
          onClick={() => navigate('/folders')}
          className="mb-3 inline-flex items-center gap-2 text-xs text-slate-500 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100"
        >
          <ArrowLeft className="h-3.5 w-3.5" /> All folders
        </button>
        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <FolderIcon className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              <span className="truncate">{folderBasename(folderPath)}</span>
            </h1>
            <p
              className="mt-1 truncate text-sm text-slate-500 dark:text-slate-400"
              title={folderPath}
            >
              {folderPath}
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
              disabled={loading}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} />
              Rescan
            </button>
            <button
              onClick={revealFolder}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              <SquareArrowOutUpRight className="h-3.5 w-3.5" />
              Open in Finder
            </button>
            <button
              onClick={openInAnalytics}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
            >
              <Sparkles className="h-3.5 w-3.5" />
              Analyze
            </button>
          </div>
        </div>
      </div>

      {/* Search + content */}
      <div className="flex-1 overflow-y-auto p-6">
        <div className="relative mb-4">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter files by name…"
            className="w-full rounded-lg border border-slate-200 bg-white py-2 pl-9 pr-3 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-100 dark:placeholder-slate-500"
          />
          {search && datasets && (
            <div className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              {filtered.length} of {datasets.length} match
            </div>
          )}
        </div>

        {loading && !datasets && (
          <div className="flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white py-12 text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900">
            <Loader2 className="h-4 w-4 animate-spin" />
            Scanning folder…
          </div>
        )}

        {error && (
          <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            Couldn't scan folder: {error}
          </div>
        )}

        {!loading && datasets && datasets.length === 0 && (
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <p className="text-sm text-slate-600 dark:text-slate-400">
              No indexable files found in this folder.
            </p>
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              Sery indexes parquet, csv, xlsx, xls, docx, pptx, html, and ipynb.
            </p>
          </div>
        )}

        {!loading && filtered.length === 0 && datasets && datasets.length > 0 && (
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-6 text-center text-sm text-slate-500 dark:border-slate-700 dark:text-slate-400">
            No files match <span className="font-mono">{search}</span>.
          </div>
        )}

        {!loading && filtered.length > 0 && (
          <ul className="space-y-2">
            {filtered.map((d) => {
              const id = d.relative_path;
              const isOpen = expandedId === id;
              return (
                <DatasetRow
                  key={id}
                  dataset={d}
                  isOpen={isOpen}
                  onToggle={() => setExpandedId(isOpen ? null : id)}
                  onLocate={() => revealDataset(d.relative_path)}
                />
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

// ─── Row ──────────────────────────────────────────────────────────────────

function DatasetRow({
  dataset,
  isOpen,
  onToggle,
  onLocate,
}: {
  dataset: DatasetMetadata;
  isOpen: boolean;
  onToggle: () => void;
  onLocate: () => void;
}) {
  const isDoc = isDocumentFormat(dataset.file_format);
  const icon = isDoc ? (
    <FileText className="h-4 w-4 text-slate-500 dark:text-slate-400" />
  ) : (
    <Database className="h-4 w-4 text-purple-600 dark:text-purple-400" />
  );

  return (
    <li className="rounded-lg border border-slate-200 bg-white transition-colors dark:border-slate-800 dark:bg-slate-900">
      <button
        onClick={onToggle}
        className="block w-full text-left"
        aria-expanded={isOpen}
      >
        <div className="flex items-center gap-3 px-4 py-3">
          {isOpen ? (
            <ChevronDown className="h-4 w-4 flex-shrink-0 text-slate-400" />
          ) : (
            <ChevronRight className="h-4 w-4 flex-shrink-0 text-slate-400" />
          )}
          {icon}
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
              {dataset.relative_path}
            </div>
            <div className="mt-0.5 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
              <span className="uppercase">{dataset.file_format}</span>
              {dataset.row_count_estimate !== null && (
                <>
                  <span>·</span>
                  <span>{dataset.row_count_estimate.toLocaleString()} rows</span>
                </>
              )}
              {dataset.schema.length > 0 && (
                <>
                  <span>·</span>
                  <span>
                    {dataset.schema.length} {dataset.schema.length === 1 ? 'col' : 'cols'}
                  </span>
                </>
              )}
              <span>·</span>
              <span>{formatBytes(dataset.size_bytes)}</span>
              <span>·</span>
              <span title={dataset.last_modified}>{formatRelativeTime(dataset.last_modified)}</span>
            </div>
          </div>
        </div>
      </button>

      {isOpen && (
        <div className="border-t border-slate-200 px-4 py-3 dark:border-slate-800">
          {/* Action row */}
          <div className="mb-3 flex items-center gap-2">
            <button
              onClick={(e) => {
                e.stopPropagation();
                onLocate();
              }}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-2 py-1 text-xs font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
            >
              <SquareArrowOutUpRight className="h-3 w-3" />
              Show in Finder
            </button>
          </div>

          {/* Schema */}
          {dataset.schema.length > 0 && (
            <div className="mb-3">
              <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                Schema
              </div>
              <div className="overflow-hidden rounded-md border border-slate-200 dark:border-slate-800">
                <table className="w-full text-xs">
                  <thead className="bg-slate-50 text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                    <tr>
                      <th className="px-3 py-1.5 text-left font-medium">Column</th>
                      <th className="px-3 py-1.5 text-left font-medium">Type</th>
                      <th className="px-3 py-1.5 text-left font-medium">Nullable</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-slate-200 dark:divide-slate-800">
                    {dataset.schema.map((c, i) => (
                      <tr key={i}>
                        <td className="px-3 py-1.5 font-mono text-slate-900 dark:text-slate-100">
                          {c.name}
                        </td>
                        <td className="px-3 py-1.5 text-slate-600 dark:text-slate-400">
                          {c.type}
                        </td>
                        <td className="px-3 py-1.5 text-slate-500 dark:text-slate-500">
                          {c.nullable ? 'yes' : 'no'}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </div>
          )}

          {/* Sample rows */}
          {dataset.sample_rows && dataset.sample_rows.length > 0 && (
            <div className="mb-3">
              <div className="mb-1 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                Sample rows
                {dataset.samples_redacted && (
                  <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-normal normal-case text-amber-700 dark:bg-amber-900/40 dark:text-amber-200">
                    PII redacted
                  </span>
                )}
              </div>
              <pre className="overflow-x-auto rounded-md border border-slate-200 bg-slate-50 p-2 font-mono text-[11px] text-slate-700 dark:border-slate-800 dark:bg-slate-950/50 dark:text-slate-300">
                {JSON.stringify(dataset.sample_rows, null, 2)}
              </pre>
            </div>
          )}

          {/* Document markdown */}
          {dataset.document_markdown && (
            <div>
              <div className="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                Extracted text (first 2000 chars)
              </div>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap rounded-md border border-slate-200 bg-slate-50 p-2 text-xs text-slate-700 dark:border-slate-800 dark:bg-slate-950/50 dark:text-slate-300">
                {dataset.document_markdown.slice(0, 2000)}
                {dataset.document_markdown.length > 2000 && '\n…'}
              </pre>
            </div>
          )}

          {dataset.schema.length === 0 &&
            !dataset.sample_rows &&
            !dataset.document_markdown && (
              <p className="text-xs text-slate-500 dark:text-slate-400">
                No schema or sample data extracted.
              </p>
            )}
        </div>
      )}
    </li>
  );
}

// ─── Helpers ──────────────────────────────────────────────────────────────

const DOCUMENT_FORMATS = new Set(['docx', 'pptx', 'html', 'htm', 'ipynb']);

function isDocumentFormat(fmt: string): boolean {
  return DOCUMENT_FORMATS.has(fmt.toLowerCase());
}

function folderBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
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
