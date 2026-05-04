// F42 Day 5 — Sources sidebar (minimal slice).
//
// Read-only-mutation list of every registered DataSource. Renders
// each as a row with kind icon + name + dataset count + status
// pill. Right-click opens a context menu with rename / move-to-
// group / remove. No drag-reorder yet (Day 7) and no Add Source
// modal here yet (heavier slice with kind-specific credential forms).
//
// This coexists with FolderList for v0.7.0 — Sources sidebar is
// the new authoritative surface; FolderList stays for one release
// while users get used to the new UX. v0.7.1 removes FolderList.
//
// Spec ref: SPEC_F42_SOURCES_SIDEBAR.md §3.1

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Loader2, MoreVertical, Plus } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { SourceIcon, sourceIconBgClass } from './SourceIcon';
import {
  groupSources,
  legacyKindStringOf,
  removeSource,
  renameSource,
  setSourceGroup,
  sourceKindLabel,
} from '../utils/sources';
import type { AgentConfig, DataSource } from '../types/events';

interface RowMenuState {
  sourceId: string;
  x: number;
  y: number;
}

export function SourcesSidebar() {
  const { config, setConfig } = useAgentStore();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<RowMenuState | null>(null);

  // Close any open context menu on outside click / escape.
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    const escape = (e: KeyboardEvent) => e.key === 'Escape' && setMenu(null);
    window.addEventListener('click', close);
    window.addEventListener('keydown', escape);
    return () => {
      window.removeEventListener('click', close);
      window.removeEventListener('keydown', escape);
    };
  }, [menu]);

  const reloadConfig = async () => {
    try {
      const next = await invoke<AgentConfig>('get_config');
      setConfig(next);
    } catch (err) {
      console.error('Failed to reload config:', err);
    }
  };

  const sources = config?.sources ?? [];
  const groups = groupSources(sources);
  // Render top-level (ungrouped) first, then named groups in
  // alphabetical order — deterministic without forcing the user to
  // think about group ordering.
  const ungrouped = groups.get('') ?? [];
  const namedGroups = [...groups.entries()]
    .filter(([key]) => key !== '')
    .sort(([a], [b]) => a.localeCompare(b));

  const onRename = async (source: DataSource) => {
    setMenu(null);
    const next = window.prompt('Rename source', source.name);
    if (next === null) return; // user cancelled
    const trimmed = next.trim();
    if (trimmed.length === 0 || trimmed === source.name) return;
    setBusy(true);
    try {
      await renameSource(source.id, trimmed);
      await reloadConfig();
      toast.success(`Renamed to "${trimmed}"`);
    } catch (err) {
      toast.error(`Couldn't rename: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const onMoveToGroup = async (source: DataSource) => {
    setMenu(null);
    const groupNames = namedGroups.map(([key]) => key);
    const hint =
      groupNames.length > 0
        ? `Existing groups: ${groupNames.join(', ')}\n(empty input clears group)`
        : '(empty input clears group)';
    const next = window.prompt(`Move to group\n${hint}`, source.group ?? '');
    if (next === null) return;
    const trimmed = next.trim();
    setBusy(true);
    try {
      await setSourceGroup(source.id, trimmed.length === 0 ? null : trimmed);
      await reloadConfig();
      toast.success(
        trimmed.length === 0 ? 'Moved to top level' : `Moved to "${trimmed}"`,
      );
    } catch (err) {
      toast.error(`Couldn't move: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const onRemove = async (source: DataSource) => {
    setMenu(null);
    const ok = window.confirm(
      `Remove "${source.name}"?\n\nThis only removes the bookmark — your data isn't touched.`,
    );
    if (!ok) return;
    setBusy(true);
    try {
      await removeSource(source.id);
      await reloadConfig();
      toast.success(`Removed "${source.name}"`);
    } catch (err) {
      toast.error(`Couldn't remove: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const renderRow = (source: DataSource) => (
    <SourceRow
      key={source.id}
      source={source}
      onContextMenu={(e) => {
        e.preventDefault();
        setMenu({ sourceId: source.id, x: e.clientX, y: e.clientY });
      }}
    />
  );

  if (sources.length === 0) {
    return (
      <div className="flex h-full flex-col items-center justify-center p-8 text-center">
        <div className="mb-4 rounded-full bg-purple-100 p-4 dark:bg-purple-900/40">
          <Plus className="h-8 w-8 text-purple-600 dark:text-purple-300" />
        </div>
        <h2 className="mb-2 text-lg font-semibold text-slate-800 dark:text-slate-100">
          No sources yet
        </h2>
        <p className="max-w-sm text-sm text-slate-600 dark:text-slate-400">
          Add a folder, S3 bucket, or remote URL from the{' '}
          <span className="font-medium">Folders</span> tab — it'll show up
          here automatically. (A unified Add Source modal lands later in v0.7.)
        </p>
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3 dark:border-slate-800">
        <h1 className="text-lg font-semibold text-slate-800 dark:text-slate-100">
          Sources
        </h1>
        {busy && (
          <Loader2 className="h-4 w-4 animate-spin text-slate-400" />
        )}
      </div>
      <div className="flex-1 overflow-y-auto p-3">
        {/* Ungrouped section — no header, just rows */}
        {ungrouped.length > 0 && (
          <div className="space-y-1">{ungrouped.map(renderRow)}</div>
        )}
        {/* Named groups — collapsible header + rows */}
        {namedGroups.map(([groupName, members]) => (
          <SourceGroupSection
            key={groupName}
            groupName={groupName}
            sources={members}
            renderRow={renderRow}
          />
        ))}
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          source={sources.find((s) => s.id === menu.sourceId)!}
          onRename={onRename}
          onMoveToGroup={onMoveToGroup}
          onRemove={onRemove}
        />
      )}
    </div>
  );
}

// ─── Helper components ─────────────────────────────────────────────

interface SourceRowProps {
  source: DataSource;
  onContextMenu: (e: React.MouseEvent) => void;
}

function SourceRow({ source, onContextMenu }: SourceRowProps) {
  const legacyKind = legacyKindStringOf(source);
  const datasetCount = source.last_scan_stats?.datasets ?? null;
  return (
    <div
      onContextMenu={onContextMenu}
      className="group flex cursor-pointer items-center gap-3 rounded-lg px-2 py-2 text-sm transition-colors hover:bg-slate-100 dark:hover:bg-slate-800"
    >
      <div
        className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md ${sourceIconBgClass(
          legacyKind,
        )}`}
      >
        <SourceIcon kind={legacyKind} size="sm" />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate font-medium text-slate-800 dark:text-slate-100">
          {source.name}
        </div>
        <div className="truncate text-xs text-slate-500 dark:text-slate-400">
          {sourceKindLabel(source)}
          {datasetCount !== null && ` · ${datasetCount.toLocaleString()} files`}
        </div>
      </div>
      <button
        onClick={(e) => {
          e.stopPropagation();
          // Treat the kebab click as a context-menu request at the
          // button's position so the same menu opens.
          const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
          onContextMenu({
            ...e,
            clientX: rect.left,
            clientY: rect.bottom,
            preventDefault: () => {},
          } as unknown as React.MouseEvent);
        }}
        className="flex h-6 w-6 items-center justify-center rounded opacity-0 transition-opacity hover:bg-slate-200 group-hover:opacity-100 dark:hover:bg-slate-700"
        aria-label="Source actions"
      >
        <MoreVertical className="h-4 w-4 text-slate-500" />
      </button>
    </div>
  );
}

interface SourceGroupSectionProps {
  groupName: string;
  sources: DataSource[];
  renderRow: (source: DataSource) => React.ReactNode;
}

function SourceGroupSection({
  groupName,
  sources,
  renderRow,
}: SourceGroupSectionProps) {
  const [collapsed, setCollapsed] = useState(false);
  return (
    <div className="mt-4">
      <button
        onClick={() => setCollapsed((c) => !c)}
        className="mb-1 flex w-full items-center justify-between rounded px-2 py-1 text-xs font-semibold uppercase tracking-wide text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <span>{groupName}</span>
        <span className="text-slate-400">
          {collapsed ? '+' : '−'} {sources.length}
        </span>
      </button>
      {!collapsed && <div className="space-y-1">{sources.map(renderRow)}</div>}
    </div>
  );
}

interface ContextMenuProps {
  x: number;
  y: number;
  source: DataSource;
  onRename: (source: DataSource) => void;
  onMoveToGroup: (source: DataSource) => void;
  onRemove: (source: DataSource) => void;
}

function ContextMenu({
  x,
  y,
  source,
  onRename,
  onMoveToGroup,
  onRemove,
}: ContextMenuProps) {
  // Stop propagation so clicks INSIDE the menu don't trigger the
  // outside-click close handler attached at window level.
  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{ left: x, top: y }}
      className="fixed z-50 min-w-[12rem] rounded-md border border-slate-200 bg-white py-1 text-sm shadow-lg dark:border-slate-700 dark:bg-slate-900"
    >
      <MenuItem onClick={() => onRename(source)}>Rename…</MenuItem>
      <MenuItem onClick={() => onMoveToGroup(source)}>Move to group…</MenuItem>
      <div className="my-1 h-px bg-slate-200 dark:bg-slate-700" />
      <MenuItem onClick={() => onRemove(source)} variant="danger">
        Remove source
      </MenuItem>
    </div>
  );
}

function MenuItem({
  children,
  onClick,
  variant,
}: {
  children: React.ReactNode;
  onClick: () => void;
  variant?: 'danger';
}) {
  const cls =
    variant === 'danger'
      ? 'text-rose-600 hover:bg-rose-50 dark:text-rose-400 dark:hover:bg-rose-900/30'
      : 'text-slate-700 hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-800';
  return (
    <button
      onClick={onClick}
      className={`block w-full px-3 py-1.5 text-left transition-colors ${cls}`}
    >
      {children}
    </button>
  );
}
