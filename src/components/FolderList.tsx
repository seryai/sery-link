// Folder dashboard — rich cards showing each watched folder with live
// scan state, last-sync stats, and per-folder actions.
//
// Live updates come from `scansInFlight` in the agent store (populated by
// the scan_progress event listener). Last-sync stats come from the folder
// config (persisted by the watcher/scanner after every sync).

import { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import {
  AlertCircle,
  Database,
  FolderOpen,
  Folder as FolderIcon,
  Globe,
  Loader2,
  MoreVertical,
  Plus,
  RefreshCw,
  Trash2,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { AddRemoteSourceModal } from './AddRemoteSourceModal';
import { SourceIcon, sourceIconBgClass } from './SourceIcon';
import {
  classifySource,
  filenameFromUrl,
  sourceKindLabel,
} from '../utils/url';
import type { AgentConfig, WatchedFolder } from '../types/events';

export function FolderList() {
  const { config, setConfig, scansInFlight } = useAgentStore();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [remoteOpen, setRemoteOpen] = useState(false);

  const reloadConfig = async () => {
    try {
      const next = await invoke<AgentConfig>('get_config');
      setConfig(next);
    } catch (err) {
      console.error('Failed to reload config:', err);
    }
  };

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

      setBusy(true);
      await invoke('add_watched_folder', { path: selected, recursive: true });
      await reloadConfig();
      toast.success('Folder added');

      // Kick off an initial scan in the background
      invoke('rescan_folder', { folderPath: selected }).catch((err) => {
        console.error('Initial scan failed:', err);
      });
    } catch (err) {
      toast.error(`Couldn't add folder: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  if (!config) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-500 dark:text-slate-400">
        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
        Loading configuration…
      </div>
    );
  }

  const folders = config.watched_folders;
  const totalDatasets = folders.reduce(
    (sum, f) => sum + (f.last_scan_stats?.datasets ?? 0),
    0,
  );
  const totalBytes = folders.reduce(
    (sum, f) => sum + (f.last_scan_stats?.total_bytes ?? 0),
    0,
  );

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <FolderIcon className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Watched folders
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              {folders.length === 0
                ? 'Add a folder to start analyzing your local data.'
                : `${folders.length} folder${folders.length === 1 ? '' : 's'} · ${totalDatasets} dataset${totalDatasets === 1 ? '' : 's'} · ${formatBytes(totalBytes)}`}
            </p>
            {/* MCP discoverability nudge — closes I4 from
                UI_AUDIT_2026_05.md. Only shown once a folder exists,
                so brand-new installs don't see noise; visible to users
                who have the substrate MCP needs. */}
            {folders.length > 0 && (
              <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
                💡 Expose any folder to Claude Desktop / Cursor / Continue
                via Settings → MCP.
              </p>
            )}
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={() => setRemoteOpen(true)}
              className="flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-4 py-2 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
              title="Connect an S3 bucket, an HTTPS URL, or other remote data — credentials stay in your OS keychain"
            >
              <Globe className="h-4 w-4" />
              Add remote source
            </button>
            <button
              onClick={addFolder}
              disabled={busy}
              className="flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
              title="Read-only access — your files are never modified"
            >
              <Plus className="h-4 w-4" />
              Watch Folder
            </button>
          </div>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
        {folders.length === 0 ? (
          <EmptyState
            onAdd={addFolder}
            onAddRemote={() => setRemoteOpen(true)}
            busy={busy}
          />
        ) : (
          <div className="grid gap-4 sm:grid-cols-1 lg:grid-cols-2 xl:grid-cols-3">
            {folders.map((folder) => (
              <FolderCard
                key={folder.path}
                folder={folder}
                scan={scansInFlight[folder.path]}
                onChanged={reloadConfig}
              />
            ))}
          </div>
        )}
      </div>

      <AddRemoteSourceModal
        open={remoteOpen}
        onClose={() => setRemoteOpen(false)}
        onAdded={async (url) => {
          await reloadConfig();
          // Kick off the initial scan so the user sees the schema
          // immediately — same pattern as the filesystem add flow.
          invoke('rescan_folder', { folderPath: url }).catch((err) => {
            console.error('Initial remote scan failed:', err);
          });
        }}
      />
    </div>
  );
}

// ─── Empty state ────────────────────────────────────────────────────────────

function EmptyState({
  onAdd,
  onAddRemote,
  busy,
}: {
  onAdd: () => void;
  onAddRemote: () => void;
  busy: boolean;
}) {
  return (
    <div className="rounded-2xl border-2 border-dashed border-slate-300 bg-slate-50 p-12 text-center dark:border-slate-700 dark:bg-slate-900">
      <div className="mx-auto mb-4 flex h-16 w-16 items-center justify-center rounded-2xl bg-purple-100 dark:bg-purple-900/40">
        <FolderOpen className="h-8 w-8 text-purple-600 dark:text-purple-300" />
      </div>
      <h2 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-100">
        No sources yet
      </h2>
      <p className="mx-auto mb-6 max-w-md text-sm text-slate-600 dark:text-slate-400">
        Pick a local folder of Parquet/CSV/Excel/documents, or connect
        an S3 bucket. Either way, credentials and raw files stay on
        your machine — only schemas + redacted samples sync to your
        Sery network.
      </p>
      <div className="flex items-center justify-center gap-2">
        <button
          onClick={onAdd}
          disabled={busy}
          className="inline-flex items-center gap-2 rounded-lg bg-purple-600 px-5 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
          title="Read-only access — your files are never modified"
        >
          <Plus className="h-4 w-4" />
          Watch a folder
        </button>
        <button
          onClick={onAddRemote}
          className="inline-flex items-center gap-2 rounded-lg border border-slate-200 bg-white px-5 py-2.5 text-sm font-semibold text-slate-700 shadow-sm transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          title="Connect an S3 bucket or HTTPS URL"
        >
          <Globe className="h-4 w-4" />
          Add remote source
        </button>
      </div>
    </div>
  );
}

// ─── Folder card ────────────────────────────────────────────────────────────

interface FolderCardProps {
  folder: WatchedFolder;
  scan?: { current: number; total: number; currentFile: string };
  onChanged: () => Promise<void>;
}

function FolderCard({ folder, scan, onChanged }: FolderCardProps) {
  const navigate = useNavigate();
  const toast = useToast();
  const [menuOpen, setMenuOpen] = useState(false);
  const [rescanning, setRescanning] = useState(false);

  const scanning = !!scan;
  const progress = scan && scan.total > 0 ? scan.current / scan.total : 0;

  // Source-type metadata. Drives the icon, the source label, and
  // the friendly title — the cache path for Drive folders is too
  // long and cryptic to use as the user-facing name. Centralised
  // here so the rest of the card stays readable.
  const kind = classifySource(folder.path);
  const { displayName, subtitle } = (() => {
    switch (kind) {
      case 'gdrive':
        return { displayName: 'My Drive', subtitle: 'All files indexed' };
      case 's3':
        return {
          displayName: filenameFromUrl(folder.path) || 'S3 source',
          subtitle: folder.path,
        };
      case 'http':
        return {
          displayName: filenameFromUrl(folder.path) || 'Web URL',
          subtitle: folder.path,
        };
      case 'local':
      // FolderList's classifySource only returns the 4 original
      // kinds — the F43-F49 variants come through the new
      // SourcesSidebar surface, not this legacy card. The default
      // branch keeps TypeScript happy for the wider SourceKind
      // union without changing real behavior.
      // eslint-disable-next-line no-fallthrough
      default:
        return { displayName: folderBasename(folder.path), subtitle: folder.path };
    }
  })();

  const openDetail = () => {
    navigate(`/folders/${encodeURIComponent(folder.path)}`);
  };

  const rescan = async () => {
    setRescanning(true);
    try {
      await invoke('rescan_folder', { folderPath: folder.path });
      // Toast is fired by the sync_completed event listener; no need to
      // duplicate here.
    } catch (err) {
      toast.error(`Scan failed: ${err}`);
    } finally {
      setRescanning(false);
    }
  };

  const reveal = async () => {
    try {
      await invoke('reveal_in_finder', { path: folder.path });
    } catch (err) {
      toast.error(`Couldn't open folder: ${err}`);
    }
    setMenuOpen(false);
  };

  const remove = async () => {
    setMenuOpen(false);
    try {
      if (kind === 'gdrive') {
        // For Drive sources we delegate to disconnect_gdrive instead
        // of remove_watched_folder because Drive has out-of-band
        // state (OAuth tokens in keychain, the gdrive_watched_folders
        // config list, cached file bytes) that remove_watched_folder
        // doesn't know to clean up. Leaving them behind would yield
        // a confusing zombie state where tokens still exist but no
        // cache dir is watched.
        await invoke('disconnect_gdrive');
        await onChanged();
        toast.success('Google Drive disconnected');
      } else {
        await invoke('remove_watched_folder', { path: folder.path });
        await onChanged();
        toast.success(kind === 'local' ? 'Folder removed' : 'Source removed');
      }
    } catch (err) {
      toast.error(`Couldn't remove: ${err}`);
    }
  };

  return (
    <div
      onClick={openDetail}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          openDetail();
        }
      }}
      className="group relative cursor-pointer overflow-hidden rounded-xl border border-slate-200 bg-white p-5 shadow-sm transition-shadow hover:border-purple-300 hover:shadow-md dark:border-slate-800 dark:bg-slate-900 dark:hover:border-purple-700"
    >
      {/* Header row */}
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <div
            className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg ${sourceIconBgClass(
              kind,
            )}`}
          >
            <SourceIcon kind={kind} />
          </div>
          <div className="min-w-0 flex-1">
            <div
              className="truncate text-sm font-semibold text-slate-900 dark:text-slate-100"
              title={folder.path}
            >
              {displayName}
            </div>
            <div
              className="truncate text-xs text-slate-500 dark:text-slate-400"
              title={folder.path}
            >
              <span className="font-medium text-slate-600 dark:text-slate-300">
                {sourceKindLabel(kind)}
              </span>
              {' · '}
              <span>{subtitle}</span>
            </div>
          </div>
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <button
            onClick={(e) => {
              e.stopPropagation();
              void rescan();
            }}
            disabled={scanning || rescanning}
            title="Rescan now"
            className="rounded-md p-1.5 text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700 disabled:cursor-not-allowed disabled:opacity-50 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          >
            <RefreshCw
              className={`h-4 w-4 ${scanning || rescanning ? 'animate-spin' : ''}`}
            />
          </button>
          <div className="relative">
            <button
              onClick={(e) => {
                e.stopPropagation();
                setMenuOpen((v) => !v);
              }}
              title="More actions"
              className="rounded-md p-1.5 text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-200"
            >
              <MoreVertical className="h-4 w-4" />
            </button>
            {menuOpen && (
              <>
                <div
                  className="fixed inset-0 z-10"
                  onClick={(e) => {
                    e.stopPropagation();
                    setMenuOpen(false);
                  }}
                />
                <div className="absolute right-0 top-full z-20 mt-1 w-44 overflow-hidden rounded-lg border border-slate-200 bg-white shadow-lg dark:border-slate-700 dark:bg-slate-800">
                  {kind === 'local' && (
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        void reveal();
                      }}
                      className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm text-slate-700 hover:bg-slate-50 dark:text-slate-200 dark:hover:bg-slate-700"
                    >
                      <FolderOpen className="h-4 w-4" />
                      Reveal in Finder
                    </button>
                  )}
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      void remove();
                    }}
                    className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm text-rose-600 hover:bg-rose-50 dark:text-rose-400 dark:hover:bg-rose-950/40"
                  >
                    <Trash2 className="h-4 w-4" />
                    {kind === 'gdrive'
                      ? 'Disconnect Google Drive'
                      : kind === 'local'
                        ? 'Remove folder'
                        : 'Remove source'}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Active scan progress */}
      {scanning && scan && (
        <div className="mb-3">
          <div className="mb-1.5 flex items-center justify-between text-xs text-slate-600 dark:text-slate-400">
            <span className="flex items-center gap-1.5">
              <Loader2 className="h-3 w-3 animate-spin" />
              Scanning {scan.current} / {scan.total}
            </span>
            <span>{Math.round(progress * 100)}%</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-slate-200 dark:bg-slate-800">
            <div
              className="h-full rounded-full bg-purple-600 transition-all duration-300"
              style={{ width: `${Math.max(2, progress * 100)}%` }}
            />
          </div>
          {scan.currentFile && (
            <div className="mt-1 truncate text-[10px] text-slate-500 dark:text-slate-500">
              {scan.currentFile}
            </div>
          )}
        </div>
      )}

      {/* Stats */}
      {folder.last_scan_stats ? (
        <div className="grid grid-cols-3 gap-3 rounded-lg bg-slate-50 p-3 dark:bg-slate-800/50">
          <Stat
            label="Datasets"
            value={folder.last_scan_stats.datasets.toLocaleString()}
            icon={<Database className="h-3 w-3" />}
          />
          <Stat
            label="Columns"
            value={folder.last_scan_stats.columns.toLocaleString()}
          />
          <Stat
            label="Size"
            value={formatBytes(folder.last_scan_stats.total_bytes)}
          />
        </div>
      ) : (
        <div className="rounded-lg bg-slate-50 p-3 text-xs text-slate-500 dark:bg-slate-800/50 dark:text-slate-400">
          Not scanned yet — click the refresh icon to index this folder.
        </div>
      )}

      {/* Footer row */}
      <div className="mt-3 flex items-center justify-between text-[11px] text-slate-500 dark:text-slate-400">
        <span>
          {folder.last_scan_at
            ? `Last synced ${formatRelativeTime(folder.last_scan_at)}`
            : 'Never synced'}
        </span>
        {folder.last_scan_stats && folder.last_scan_stats.errors > 0 && (
          <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
            <AlertCircle className="h-3 w-3" />
            {folder.last_scan_stats.errors} file
            {folder.last_scan_stats.errors === 1 ? '' : 's'} skipped
          </span>
        )}
      </div>
    </div>
  );
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function Stat({
  label,
  value,
  icon,
}: {
  label: string;
  value: string;
  icon?: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-0.5 flex items-center gap-1 text-[10px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {icon}
        {label}
      </div>
      <div className="text-sm font-semibold text-slate-900 dark:text-slate-100">
        {value}
      </div>
    </div>
  );
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
