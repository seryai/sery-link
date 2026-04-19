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

import { useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import {
  ArrowLeft,
  Database,
  FileText,
  Folder as FolderIcon,
  Loader2,
  SquareArrowOutUpRight,
  Sparkles,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import type { DatasetMetadataPayload as DatasetMetadata } from '../types/events';

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

  const openInAnalytics = () => {
    navigate(`/analytics/${encodeURIComponent(folderPath)}`);
  };

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
            <button
              onClick={reveal}
              className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              <SquareArrowOutUpRight className="h-3.5 w-3.5" />
              Show in Finder
            </button>
            {dataset && !isDocumentFormat(dataset.file_format) && (
              <button
                onClick={openInAnalytics}
                className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
              >
                <Sparkles className="h-3.5 w-3.5" />
                Analyze
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
            {/* Schema — tabular only */}
            {dataset.schema.length > 0 && (
              <section className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
                <header className="border-b border-slate-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:border-slate-800 dark:text-slate-400">
                  Schema
                </header>
                <div className="overflow-hidden">
                  <table className="w-full text-sm">
                    <thead className="bg-slate-50 text-slate-600 dark:bg-slate-800/50 dark:text-slate-300">
                      <tr>
                        <th className="px-4 py-2 text-left font-medium">
                          Column
                        </th>
                        <th className="px-4 py-2 text-left font-medium">
                          Type
                        </th>
                        <th className="px-4 py-2 text-left font-medium">
                          Nullable
                        </th>
                      </tr>
                    </thead>
                    <tbody className="divide-y divide-slate-200 dark:divide-slate-800">
                      {dataset.schema.map((c, i) => (
                        <tr
                          key={i}
                          className="hover:bg-slate-50 dark:hover:bg-slate-800/50"
                        >
                          <td className="px-4 py-2 font-mono text-slate-900 dark:text-slate-100">
                            {c.name}
                          </td>
                          <td className="px-4 py-2 text-slate-600 dark:text-slate-400">
                            {c.type}
                          </td>
                          <td className="px-4 py-2 text-slate-500 dark:text-slate-500">
                            {c.nullable ? 'yes' : 'no'}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </section>
            )}

            {/* Sample rows */}
            {dataset.sample_rows && dataset.sample_rows.length > 0 && (
              <section className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
                <header className="flex items-center justify-between border-b border-slate-200 px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:border-slate-800 dark:text-slate-400">
                  <span>Sample rows</span>
                  {dataset.samples_redacted && (
                    <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[10px] font-normal normal-case text-amber-700 dark:bg-amber-900/40 dark:text-amber-200">
                      PII redacted
                    </span>
                  )}
                </header>
                <pre className="overflow-x-auto p-3 font-mono text-xs text-slate-700 dark:text-slate-300">
                  {JSON.stringify(dataset.sample_rows, null, 2)}
                </pre>
              </section>
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
              !dataset.document_markdown &&
              !dataset.sample_rows && (
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
