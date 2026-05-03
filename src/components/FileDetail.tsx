// File detail — the third navigation level (Folders → Folder → File).
//
// Reached by clicking a row in FolderDetail OR a result in SearchPage.
// Shows everything the old expanded-row view showed, plus more vertical
// room for schema tables, sample rows, and document-markdown previews.
//
// Reads cached DatasetMetadata via `get_cached_folder_metadata` (same
// source of truth FolderDetail uses), then finds the matching row by
// relative_path. If the cache doesn't have the file — e.g. the user
// deep-linked to a never-scanned path — we fall back to an empty state
// with a Rescan suggestion rather than erroring.

import { useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  ArrowLeft,
  Database,
  FileText,
  Folder as FolderIcon,
  Loader2,
  Search,
  SquareArrowOutUpRight,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { isRemoteUrl } from '../utils/url';
import type {
  ColumnProfile,
  DatasetMetadataPayload as DatasetMetadata,
} from '../types/events';

const DOCUMENT_FORMATS = new Set(['docx', 'pptx', 'html', 'htm', 'ipynb']);
function isDocumentFormat(fmt: string) {
  return DOCUMENT_FORMATS.has(fmt.toLowerCase());
}

export function FileDetail() {
  const { folderId, filePath } = useParams<{
    folderId: string;
    filePath: string;
  }>();
  const navigate = useNavigate();
  const toast = useToast();
  const { config } = useAgentStore();

  const folderPath = folderId ? decodeURIComponent(folderId) : '';
  const relativePath = filePath ? decodeURIComponent(filePath) : '';
  const folder = config?.watched_folders.find((f) => f.path === folderPath);

  const [dataset, setDataset] = useState<DatasetMetadata | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Column profile — lazy-loaded on user action ("Profile this file")
  // because SUMMARIZE touches every row and can take several seconds on
  // large files. We don't want to pay that cost just because the user
  // navigated here from search.
  const [profile, setProfile] = useState<ColumnProfile[] | null>(null);
  const [profileLoading, setProfileLoading] = useState(false);
  const [profileError, setProfileError] = useState<string | null>(null);

  useEffect(() => {
    if (!folderPath || !relativePath) return;
    let cancelled = false;
    (async () => {
      setLoading(true);
      try {
        const rows = await invoke<DatasetMetadata[]>(
          'get_cached_folder_metadata',
          { folderPath },
        );
        if (cancelled) return;
        const match = rows.find((r) => r.relative_path === relativePath) ?? null;
        setDataset(match);
        setError(null);
      } catch (err) {
        if (cancelled) return;
        setError(String(err));
        setDataset(null);
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [folderPath, relativePath]);

  const absolutePath = useMemo(() => {
    if (!folderPath || !relativePath) return '';
    return `${folderPath.replace(/\/$/, '')}/${relativePath}`;
  }, [folderPath, relativePath]);

  const reveal = async () => {
    try {
      await invoke('reveal_in_finder', { path: absolutePath });
    } catch (err) {
      toast.error(`Couldn't open: ${err}`);
    }
  };

  // Auto-load column stats on mount for tabular files. No click needed —
  // when the user lands here they want to know about the file, so stats
  // appear inline next to the schema. Documents (docx/pptx/html) skip
  // this because there are no columns to profile.
  //
  // SUMMARIZE is fast on parquet and small CSVs (sub-second). For very
  // large files it can take a few seconds — the schema still renders
  // immediately and stats fill in when ready, so the page isn't blank
  // while we wait.
  useEffect(() => {
    // Reset whenever the file changes so old stats don't linger.
    setProfile(null);
    setProfileError(null);
    setProfileLoading(false);

    if (!folderPath || !relativePath) return;
    if (!dataset || dataset.schema.length === 0) return;
    if (isDocumentFormat(dataset.file_format)) return;

    let cancelled = false;
    setProfileLoading(true);
    (async () => {
      try {
        const result = await invoke<ColumnProfile[]>('profile_dataset', {
          folderPath,
          relativePath,
        });
        if (cancelled) return;
        setProfile(result);
      } catch (err) {
        if (cancelled) return;
        setProfileError(String(err));
      } finally {
        if (!cancelled) setProfileLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [folderPath, relativePath, dataset]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header with breadcrumb + actions */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="mb-3 flex flex-wrap items-center gap-1 text-xs text-slate-500 dark:text-slate-400">
          <button
            onClick={() => navigate('/folders')}
            className="hover:text-slate-900 dark:hover:text-slate-100"
          >
            Folders
          </button>
          <span className="text-slate-300 dark:text-slate-600">/</span>
          <button
            onClick={() =>
              navigate(`/folders/${encodeURIComponent(folderPath)}`)
            }
            className="hover:text-slate-900 dark:hover:text-slate-100"
          >
            {folderBasename(folderPath) || folderPath || '(folder)'}
          </button>
          <span className="text-slate-300 dark:text-slate-600">/</span>
          <span className="text-slate-900 dark:text-slate-100">
            {fileBasename(relativePath) || relativePath || '(file)'}
          </span>
        </div>

        <div className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              {dataset && isDocumentFormat(dataset.file_format) ? (
                <FileText className="h-6 w-6 text-slate-500 dark:text-slate-400" />
              ) : (
                <Database className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              )}
              <span className="truncate">{fileBasename(relativePath)}</span>
            </h1>
            <p
              className="mt-1 truncate text-xs text-slate-500 dark:text-slate-400"
              title={absolutePath}
            >
              {absolutePath}
            </p>
          </div>
          <div className="flex shrink-0 items-center gap-2">
            <button
              onClick={() =>
                navigate(`/folders/${encodeURIComponent(folderPath)}`)
              }
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              <ArrowLeft className="h-3.5 w-3.5" />
              Folder
            </button>
            {/* Convert to Parquet — only for local CSV / TSV / Excel.
                Hidden for remote URLs (no source file to read) and
                for files already in Parquet. */}
            {!isRemoteUrl(folderPath) &&
              dataset &&
              isConvertibleFormat(dataset.file_format) && (
                <ConvertToParquetButton
                  folderPath={folderPath}
                  relativePath={relativePath}
                  onConverted={(result) => {
                    // Auto-navigate to the new file so the user
                    // sees their conversion succeed in the most
                    // direct way: the parquet's stats + row preview
                    // open in front of them.
                    navigate(
                      `/folders/${encodeURIComponent(folderPath)}/files/${encodeURIComponent(result.relative_path)}`,
                    );
                  }}
                />
              )}
            {!isRemoteUrl(folderPath) && (
              <button
                onClick={reveal}
                className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
              >
                <SquareArrowOutUpRight className="h-3.5 w-3.5" />
                Show in Finder
              </button>
            )}
          </div>
        </div>

        {/* Stats strip */}
        {dataset && (
          <div className="mt-3 flex items-center gap-2 text-xs text-slate-500 dark:text-slate-400">
            <span className="uppercase tracking-wide">
              {dataset.file_format}
            </span>
            <span className="text-slate-300 dark:text-slate-600">·</span>
            <span>{formatBytes(dataset.size_bytes)}</span>
            {dataset.row_count_estimate !== null && (
              <>
                <span className="text-slate-300 dark:text-slate-600">·</span>
                <span>
                  {dataset.row_count_estimate.toLocaleString()} rows
                </span>
              </>
            )}
            {dataset.schema.length > 0 && (
              <>
                <span className="text-slate-300 dark:text-slate-600">·</span>
                <span>
                  {dataset.schema.length} column
                  {dataset.schema.length === 1 ? '' : 's'}
                </span>
              </>
            )}
            <span className="text-slate-300 dark:text-slate-600">·</span>
            <span title={dataset.last_modified}>
              Modified {formatRelativeTime(dataset.last_modified)}
            </span>
          </div>
        )}
      </div>

      {/* Body */}
      <div className="flex-1 overflow-y-auto p-6">
        {loading && !dataset && (
          <div className="flex items-center justify-center gap-2 rounded-lg border border-slate-200 bg-white py-12 text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading file details…
          </div>
        )}

        {error && (
          <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
            Couldn't load file: {error}
          </div>
        )}

        {!loading && !dataset && !error && (
          <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
            <FolderIcon
              className="mx-auto mb-3 h-10 w-10 text-slate-300 dark:text-slate-600"
              strokeWidth={1.5}
            />
            <p className="text-sm text-slate-600 dark:text-slate-400">
              This file isn't in the cache yet.
            </p>
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              Open the parent folder and run Rescan to index it.
            </p>
            {folder && (
              <button
                onClick={() =>
                  navigate(`/folders/${encodeURIComponent(folderPath)}`)
                }
                className="mt-4 inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
              >
                <FolderIcon className="h-3.5 w-3.5" />
                Open parent folder
              </button>
            )}
          </div>
        )}

        {dataset && (
          <div className="space-y-4">
            {/* One unified "Columns" section. Shows schema immediately
                (from the scan cache) and fills in computed stats as
                they arrive. No separate "profile" / "schema" split —
                the user sees one table that answers "what's in each
                column?" */}
            {dataset.schema.length > 0 && (
              <ColumnsSection
                schema={dataset.schema}
                profile={profile}
                profileLoading={profileLoading}
                profileError={profileError}
              />
            )}

            {/* Inline data preview — replaces the old JSON sample
                dump for tabular files. Calls read_dataset_rows on
                mount and renders up to MAX_PREVIEW_ROWS in a
                virtualized table with a client-side filter. */}
            {!isDocumentFormat(dataset.file_format) && (
              <DataPreview
                folderPath={folderPath}
                relativePath={relativePath}
              />
            )}

            {/* Document markdown */}
            {dataset.document_markdown && (
              <section className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
                <header className="border-b border-slate-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:border-slate-800 dark:text-slate-400">
                  Extracted text
                </header>
                <pre className="max-h-[70vh] overflow-auto whitespace-pre-wrap p-4 text-sm text-slate-700 dark:text-slate-300">
                  {dataset.document_markdown}
                </pre>
              </section>
            )}

            {/* Nothing extracted (Shallow tier) */}
            {dataset.schema.length === 0 &&
              !dataset.document_markdown && (
                <section className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
                  <p className="text-sm text-slate-600 dark:text-slate-400">
                    No schema or extracted text for this file.
                  </p>
                  <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
                    Sery indexed the filename and size; content extraction
                    was skipped for this format.
                  </p>
                </section>
              )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Columns section ──────────────────────────────────────────────────

// Unified "columns" table. Shows schema (name + type) immediately from
// cache, then adds Empty / Unique values / Min / Max / Avg inline as
// stats arrive. One table, one mental model — instead of the previous
// split where "Schema" and "Profile" were separate sections that
// duplicated the column list.
//
// User-facing labels are deliberately non-jargon: "Empty" instead of
// "Null %", "Unique values" instead of "approx_unique". DuckDB terms
// leak into column_type (VARCHAR/BIGINT etc) — that's a separate
// follow-up worth doing if real users find it confusing.
function ColumnsSection({
  schema,
  profile,
  profileLoading,
  profileError,
}: {
  schema: { name: string; type: string; nullable: boolean }[];
  profile: ColumnProfile[] | null;
  profileLoading: boolean;
  profileError: string | null;
}) {
  // Index the profile by column name so we can join schema rows with
  // their stats in O(1) per row. Stats may arrive after the schema is
  // already on screen, so an empty profile just means "stats still
  // loading" — not "no stats at all".
  const profileByName = new Map<string, ColumnProfile>();
  for (const p of profile ?? []) {
    profileByName.set(p.column_name, p);
  }

  return (
    <section className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
      <header className="flex items-center justify-between border-b border-slate-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:border-slate-800 dark:text-slate-400">
        <span>
          Columns · {schema.length}
        </span>
        {profileLoading && (
          <span className="flex items-center gap-1 text-[11px] font-normal normal-case text-slate-400">
            <Loader2 className="h-3 w-3 animate-spin" />
            Computing stats…
          </span>
        )}
      </header>
      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead className="bg-slate-50 text-slate-600 dark:bg-slate-800/50 dark:text-slate-300">
            <tr>
              <th className="px-4 py-2 text-left font-medium">Name</th>
              <th className="px-4 py-2 text-left font-medium">Type</th>
              <th
                className="px-4 py-2 text-right font-medium"
                title="Percentage of rows where this column is empty / blank"
              >
                Empty
              </th>
              <th
                className="px-4 py-2 text-right font-medium"
                title="Approximate number of distinct values in this column"
              >
                Unique values
              </th>
              <th className="px-4 py-2 text-right font-medium">Min</th>
              <th className="px-4 py-2 text-right font-medium">Max</th>
              <th className="px-4 py-2 text-right font-medium">Average</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-200 dark:divide-slate-800">
            {schema.map((col, i) => {
              const stats = profileByName.get(col.name);
              return (
                <tr
                  key={i}
                  className="hover:bg-slate-50 dark:hover:bg-slate-800/50"
                >
                  <td className="px-4 py-2 font-mono text-slate-900 dark:text-slate-100">
                    {col.name}
                  </td>
                  <td className="px-4 py-2 text-slate-600 dark:text-slate-400">
                    {col.type}
                  </td>
                  <td
                    className={`px-4 py-2 text-right tabular-nums ${nullPctColor(stats?.null_percentage ?? null)}`}
                  >
                    {formatStat(
                      stats ? formatPercent(stats.null_percentage) : null,
                      profileLoading,
                    )}
                  </td>
                  <td className="px-4 py-2 text-right tabular-nums text-slate-700 dark:text-slate-300">
                    {formatStat(
                      stats?.approx_unique !== undefined &&
                        stats?.approx_unique !== null
                        ? stats.approx_unique.toLocaleString()
                        : null,
                      profileLoading,
                    )}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-xs text-slate-600 dark:text-slate-400">
                    {formatStat(truncate(stats?.min ?? null, 24), profileLoading)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-xs text-slate-600 dark:text-slate-400">
                    {formatStat(truncate(stats?.max ?? null, 24), profileLoading)}
                  </td>
                  <td className="px-4 py-2 text-right font-mono text-xs text-slate-600 dark:text-slate-400">
                    {formatStat(truncate(stats?.avg ?? null, 12), profileLoading)}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {profileError && (
        <p className="border-t border-rose-200 bg-rose-50 px-4 py-2 text-[11px] text-rose-700 dark:border-rose-900 dark:bg-rose-950/30 dark:text-rose-300">
          Couldn't compute stats: {profileError}
        </p>
      )}
    </section>
  );
}

/// Placeholder for a stats cell while the profile is still loading.
/// Once it arrives we render the real value; if the profile failed or
/// doesn't cover this column we show an em-dash so the layout is stable.
function formatStat(value: string | null, loading: boolean): string {
  if (value !== null) return value;
  return loading ? '…' : '—';
}

function formatPercent(v: number | null): string {
  if (v === null || v === undefined) return '—';
  return `${v.toFixed(v < 1 ? 2 : 1)}%`;
}

function nullPctColor(v: number | null): string {
  if (v === null || v === undefined || v === 0)
    return 'text-slate-700 dark:text-slate-300';
  if (v > 50) return 'text-rose-600 dark:text-rose-400';
  if (v > 20) return 'text-amber-600 dark:text-amber-400';
  return 'text-slate-700 dark:text-slate-300';
}

function truncate(s: string | null, max: number): string {
  if (s === null || s === undefined) return '—';
  if (s.length <= max) return s;
  return s.slice(0, max - 1) + '…';
}

// ─── Format conversion ─────────────────────────────────────────────────
//
// CSV / TSV / XLSX / XLS → Parquet via a backend Tauri command.
// Parquet's footprint is typically 5-50x smaller than CSV and queries
// 10-100x faster, which matters when the same file gets opened
// repeatedly. The conversion writes next to the source so the file
// watcher picks it up on its next sync — no extra "where did my file
// go" UX.

interface ConvertResult {
  output_path: string;
  relative_path: string;
  size_bytes: number;
  source_size_bytes: number;
}

const CONVERTIBLE_FORMATS = new Set(['csv', 'tsv', 'xlsx', 'xls']);

function isConvertibleFormat(fmt: string): boolean {
  return CONVERTIBLE_FORMATS.has(fmt.toLowerCase());
}

function ConvertToParquetButton({
  folderPath,
  relativePath,
  onConverted,
}: {
  folderPath: string;
  relativePath: string;
  onConverted: (result: ConvertResult) => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);

  const handle = async () => {
    if (busy) return;
    setBusy(true);
    try {
      const result = await invoke<ConvertResult>('convert_to_parquet', {
        folderPath,
        relativePath,
      });
      const ratio =
        result.source_size_bytes > 0
          ? result.source_size_bytes / Math.max(1, result.size_bytes)
          : 0;
      const ratioLabel = ratio >= 1.5 ? ` (${ratio.toFixed(1)}× smaller)` : '';
      toast.success(
        `Converted to ${result.relative_path}${ratioLabel}`,
      );
      onConverted(result);
    } catch (err) {
      toast.error(`Convert failed: ${typeof err === 'string' ? err : String(err)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      onClick={handle}
      disabled={busy}
      title="Save a Parquet copy next to this file. Faster + smaller for repeat queries."
      className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
    >
      {busy ? (
        <Loader2 className="h-3.5 w-3.5 animate-spin" />
      ) : (
        <Database className="h-3.5 w-3.5" />
      )}
      {busy ? 'Converting…' : 'Convert to Parquet'}
    </button>
  );
}

// ─── Data preview ──────────────────────────────────────────────────────
//
// Inline browser for tabular files. Calls read_dataset_rows on mount,
// renders the result in a virtualized table with a client-side filter
// (matches any cell substring across all loaded rows).
//
// Replaces the "Sample rows" JSON dump that used to live here. Doc
// formats keep their existing extracted-text panel since they have
// no row structure.

interface DatasetRowsPayload {
  columns: string[];
  rows: string[][];
  total_rows: number;
  truncated: boolean;
}

function DataPreview({
  folderPath,
  relativePath,
}: {
  folderPath: string;
  relativePath: string;
}) {
  const [data, setData] = useState<DatasetRowsPayload | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState('');

  useEffect(() => {
    if (!folderPath || !relativePath) return;
    let cancelled = false;
    setLoading(true);
    setError(null);
    invoke<DatasetRowsPayload>('read_dataset_rows', { folderPath, relativePath })
      .then((result) => {
        if (cancelled) return;
        setData(result);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(typeof err === 'string' ? err : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [folderPath, relativePath]);

  // Client-side filter — case-insensitive substring across every cell.
  // Cheap because rows are bounded by MAX_PREVIEW_ROWS (5000); no need
  // to round-trip a query for re-filter on every keystroke.
  const filteredRows = useMemo(() => {
    if (!data) return [];
    const q = filter.trim().toLowerCase();
    if (!q) return data.rows;
    return data.rows.filter((row) => row.some((cell) => cell.toLowerCase().includes(q)));
  }, [data, filter]);

  return (
    <section className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
      <header className="flex items-center justify-between gap-3 border-b border-slate-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:border-slate-800 dark:text-slate-400">
        <span>Data preview</span>
        {data && (
          <span className="font-normal normal-case text-slate-400 dark:text-slate-500">
            {filter ? `${filteredRows.length} of ` : ''}
            {data.rows.length.toLocaleString()}
            {data.truncated && data.total_rows > 0
              ? ` of ${data.total_rows.toLocaleString()} rows (showing first ${data.rows.length.toLocaleString()})`
              : data.truncated
                ? ` rows (more available)`
                : ` rows`}
          </span>
        )}
      </header>

      {loading && (
        <div className="flex items-center justify-center gap-2 p-8 text-sm text-slate-500 dark:text-slate-400">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading rows…
        </div>
      )}

      {error && !loading && (
        <div className="m-3 rounded-md border border-rose-300 bg-rose-50 p-3 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
          {error}
        </div>
      )}

      {!loading && !error && data && data.rows.length === 0 && (
        <div className="p-8 text-center text-sm text-slate-500 dark:text-slate-400">
          File has no rows.
        </div>
      )}

      {!loading && !error && data && data.rows.length > 0 && (
        <>
          <div className="border-b border-slate-200 p-2 dark:border-slate-800">
            <div className="relative">
              <Search className="pointer-events-none absolute left-2 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-slate-400" />
              <input
                type="text"
                value={filter}
                onChange={(e) => setFilter(e.target.value)}
                placeholder={`Filter rows (across ${data.columns.length} column${data.columns.length === 1 ? '' : 's'})…`}
                className="w-full rounded-md border border-slate-200 bg-white py-1.5 pl-7 pr-2 text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-1 focus:ring-purple-500/30 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
              />
            </div>
          </div>
          <PreviewTable columns={data.columns} rows={filteredRows} />
          {data.truncated && (
            <div className="border-t border-slate-200 bg-amber-50 px-3 py-2 text-[11px] text-amber-800 dark:border-slate-800 dark:bg-amber-950/30 dark:text-amber-200">
              File is larger than the preview limit. To analyse the
              whole thing, open this workspace in the dashboard and
              ask in chat, or run a recipe over this folder.
            </div>
          )}
        </>
      )}
    </section>
  );
}

const ROW_HEIGHT = 28;

function PreviewTable({
  columns,
  rows,
}: {
  columns: string[];
  rows: string[][];
}) {
  // Virtualize the row list so a 5000-row preview stays smooth.
  // Tanstack-virtual handles overscan + position math; we just
  // give it the row count + a stable size estimator.
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 12,
  });
  const items = virtualizer.getVirtualItems();
  const totalSize = virtualizer.getTotalSize();

  return (
    <div ref={scrollRef} className="max-h-[520px] overflow-auto">
      <table className="min-w-full text-xs">
        <thead className="sticky top-0 z-10 bg-slate-50 dark:bg-slate-800/80">
          <tr>
            {columns.map((c) => (
              <th
                key={c}
                className="border-b border-slate-200 px-2 py-1.5 text-left font-semibold text-slate-700 dark:border-slate-700 dark:text-slate-200"
              >
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody style={{ position: 'relative', height: `${totalSize}px` }}>
          {items.map((vRow) => {
            const row = rows[vRow.index];
            if (!row) return null;
            return (
              <tr
                key={vRow.key}
                style={{
                  position: 'absolute',
                  top: 0,
                  left: 0,
                  width: '100%',
                  height: `${vRow.size}px`,
                  transform: `translateY(${vRow.start}px)`,
                }}
                className="even:bg-slate-50/40 dark:even:bg-slate-900/40"
              >
                {row.map((cell, j) => (
                  <td
                    key={j}
                    title={cell}
                    className="max-w-xs truncate border-b border-slate-100 px-2 py-1 font-mono text-slate-600 dark:border-slate-800 dark:text-slate-300"
                  >
                    {cell}
                  </td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ─── Helpers ───────────────────────────────────────────────────────────

function fileBasename(relativePath: string): string {
  const parts = relativePath.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || relativePath;
}

function folderBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

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
