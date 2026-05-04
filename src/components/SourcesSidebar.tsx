// F42 Days 5-6 — Sources sidebar.
//
// Mutation-capable list of every registered DataSource. Renders
// each as a row with kind icon + name + dataset count + status
// pill. Right-click opens a context menu with rename / rescan /
// move-to-group / remove. No drag-reorder yet (Day 7) and no Add
// Source modal here yet (Day 8 — kind-specific credential forms).
//
// This coexists with FolderList for v0.7.0 — Sources sidebar is
// the new authoritative surface; FolderList stays for one release
// while users get used to the new UX. v0.7.1 removes FolderList.
//
// Spec ref: SPEC_F42_SOURCES_SIDEBAR.md §3.1

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  GripVertical,
  Loader2,
  MoreVertical,
  Plus,
  RefreshCw,
} from 'lucide-react';
import {
  DndContext,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import {
  SortableContext,
  arrayMove,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { AddSourceModal } from './AddSourceModal';
import { EditCredentialsDialog } from './EditCredentialsDialog';
import { EditS3CredentialsDialog } from './EditS3CredentialsDialog';
import { SourceIcon, sourceIconBgClass } from './SourceIcon';
import {
  groupSources,
  legacyKindStringOf,
  removeSource,
  renameSource,
  reorderSources,
  scanKeyOf,
  setSourceGroup,
  sourceKindLabel,
} from '../utils/sources';
import type { AgentConfig, DataSource } from '../types/events';

type SourceStatus = 'scanning' | 'online' | 'pending';

interface RowMenuState {
  sourceId: string;
  x: number;
  y: number;
}

/** Compute the visible status of a source from its scan history +
 *  any in-flight scan. "scanning" wins over the static state because
 *  it's the freshest signal. */
function statusOf(
  source: DataSource,
  scansInFlight: Record<string, unknown>,
): SourceStatus {
  const key = scanKeyOf(source);
  if (key && key in scansInFlight) {
    return 'scanning';
  }
  if (source.last_scan_at) {
    return 'online';
  }
  return 'pending';
}

export function SourcesSidebar() {
  const { config, setConfig, scansInFlight } = useAgentStore();
  const toast = useToast();
  const [busy, setBusy] = useState(false);
  const [menu, setMenu] = useState<RowMenuState | null>(null);
  const [editingSourceId, setEditingSourceId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [groupPickerSourceId, setGroupPickerSourceId] = useState<string | null>(
    null,
  );
  const [editCredsSourceId, setEditCredsSourceId] = useState<string | null>(
    null,
  );

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

  // Inline rename: clicking "Rename…" puts the row into edit mode.
  // The actual API call fires when the user submits the input
  // (Enter / blur). Esc cancels without saving.
  const onRename = (source: DataSource) => {
    setMenu(null);
    setEditingSourceId(source.id);
  };

  const commitRename = async (source: DataSource, nextName: string) => {
    setEditingSourceId(null);
    const trimmed = nextName.trim();
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

  const onRescan = async (source: DataSource) => {
    setMenu(null);
    // F43 / F44 / F48: SFTP + WebDAV + Dropbox rescans are
    // kind-aware — run the download-then-scan flow via their
    // respective rescan_*_source command, which pulls files to a
    // per-source cache dir and then runs the local scanner.
    if (
      source.kind.kind === 'sftp' ||
      source.kind.kind === 'web_dav' ||
      source.kind.kind === 'dropbox' ||
      source.kind.kind === 'azure_blob'
    ) {
      const rescanCommand =
        source.kind.kind === 'sftp'
          ? 'rescan_sftp_source'
          : source.kind.kind === 'web_dav'
            ? 'rescan_webdav_source'
            : source.kind.kind === 'dropbox'
              ? 'rescan_dropbox_source'
              : 'rescan_azure_blob_source';
      setBusy(true);
      try {
        toast.success(`Rescanning "${source.name}" (downloading…)`);
        await invoke(rescanCommand, { sourceId: source.id });
        await reloadConfig();
        toast.success(`Rescan complete for "${source.name}"`);
      } catch (err) {
        toast.error(`Rescan failed: ${err}`);
      } finally {
        setBusy(false);
      }
      return;
    }
    const key = scanKeyOf(source);
    if (!key) {
      // Drive sources scan via gdrive_walker, not the path-keyed
      // scanner. Until the Drive adapter rewires through DataSource,
      // we surface a clean message instead of silently failing.
      toast.error('Rescan for Google Drive sources is not yet supported here.');
      return;
    }
    setBusy(true);
    try {
      await invoke('rescan_folder', { folderPath: key });
      toast.success(`Rescanning "${source.name}"…`);
      // Don't await reload — the scan_progress event listener will
      // tick scansInFlight and the StatusPill will animate.
    } catch (err) {
      toast.error(`Couldn't start rescan: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const onRevealInFinder = async (source: DataSource) => {
    setMenu(null);
    if (source.kind.kind !== 'local') {
      // Spec §3.1 lists "Show in Finder" as Local-only; the menu
      // hides it for other kinds, but defensively bail here too.
      return;
    }
    try {
      await invoke('reveal_in_finder', { path: source.kind.path });
    } catch (err) {
      toast.error(`Couldn't reveal: ${err}`);
    }
  };

  const onEditCredentials = (source: DataSource) => {
    setMenu(null);
    // S3 + the four cache-and-scan kinds all support credential
    // rotation. Local / HTTPS / Drive don't (Local has no creds;
    // HTTPS public; Drive uses OAuth tokens managed by gdrive_oauth
    // separately).
    if (
      source.kind.kind === 's3' ||
      source.kind.kind === 'sftp' ||
      source.kind.kind === 'web_dav' ||
      source.kind.kind === 'dropbox' ||
      source.kind.kind === 'azure_blob'
    ) {
      setEditCredsSourceId(source.id);
    }
  };

  /** Kick a rescan for every scannable source. Local / HTTPS / S3
   *  go through rescan_folder (path-keyed); SFTP goes through
   *  rescan_sftp_source (download-then-scan, kind-aware). Drive
   *  is skipped — it scans via gdrive_walker, not the path-keyed
   *  scanner, until the Drive adapter rewires through DataSource. */
  const onScanAll = async () => {
    const sftpTargets = sources.filter((s) => s.kind.kind === 'sftp');
    const webdavTargets = sources.filter((s) => s.kind.kind === 'web_dav');
    const dropboxTargets = sources.filter((s) => s.kind.kind === 'dropbox');
    const azureTargets = sources.filter((s) => s.kind.kind === 'azure_blob');
    const pathTargets = sources
      .filter(
        (s) =>
          s.kind.kind !== 'sftp' &&
          s.kind.kind !== 'web_dav' &&
          s.kind.kind !== 'dropbox' &&
          s.kind.kind !== 'azure_blob' &&
          s.kind.kind !== 'google_drive',
      )
      .map((s) => ({ source: s, key: scanKeyOf(s) }))
      .filter((t): t is { source: DataSource; key: string } => t.key !== null);
    const total =
      sftpTargets.length +
      webdavTargets.length +
      dropboxTargets.length +
      azureTargets.length +
      pathTargets.length;
    if (total === 0) {
      toast.error('No scannable sources');
      return;
    }
    toast.success(`Rescanning ${total} source(s)…`);
    // Fire-and-forget: each invoke returns when the scanner ACK's;
    // scan_progress events drive the StatusPill updates for path-
    // keyed scans. Cache-and-scan kinds don't emit live events yet
    // (slice 3 polish) — the toast confirmation is the user feedback.
    for (const { key } of pathTargets) {
      invoke('rescan_folder', { folderPath: key }).catch((err) => {
        console.error(`Scan-all failed for ${key}:`, err);
      });
    }
    for (const source of sftpTargets) {
      invoke('rescan_sftp_source', { sourceId: source.id }).catch((err) => {
        console.error(`Scan-all SFTP failed for ${source.id}:`, err);
      });
    }
    for (const source of webdavTargets) {
      invoke('rescan_webdav_source', { sourceId: source.id }).catch((err) => {
        console.error(`Scan-all WebDAV failed for ${source.id}:`, err);
      });
    }
    for (const source of dropboxTargets) {
      invoke('rescan_dropbox_source', { sourceId: source.id }).catch((err) => {
        console.error(`Scan-all Dropbox failed for ${source.id}:`, err);
      });
    }
    for (const source of azureTargets) {
      invoke('rescan_azure_blob_source', { sourceId: source.id }).catch(
        (err) => {
          console.error(`Scan-all Azure failed for ${source.id}:`, err);
        },
      );
    }
  };

  const onMoveToGroup = (source: DataSource) => {
    setMenu(null);
    setGroupPickerSourceId(source.id);
  };

  const commitGroupChange = async (
    source: DataSource,
    newGroup: string | null,
  ) => {
    setGroupPickerSourceId(null);
    if ((source.group ?? null) === newGroup) return;
    setBusy(true);
    try {
      await setSourceGroup(source.id, newGroup);
      await reloadConfig();
      toast.success(
        newGroup === null ? 'Moved to top level' : `Moved to "${newGroup}"`,
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

  const renderRow = (source: DataSource, sortable: boolean) => {
    const props: SourceRowProps = {
      source,
      status: statusOf(source, scansInFlight),
      editing: editingSourceId === source.id,
      onCommitRename: (name) => commitRename(source, name),
      onCancelRename: () => setEditingSourceId(null),
      onContextMenu: (e) => {
        e.preventDefault();
        setMenu({ sourceId: source.id, x: e.clientX, y: e.clientY });
      },
    };
    return sortable ? (
      <SortableSourceRow key={source.id} {...props} />
    ) : (
      <SourceRow key={source.id} {...props} />
    );
  };

  // ─── Drag-reorder (within each bucket) ─────────────────────────
  // Each bucket — the ungrouped section and each named group —
  // is its own DndContext, so dragging reorders within that bucket
  // only. Cross-bucket drag (drop onto a foreign group) is out of
  // scope; users move sources between groups via the
  // "Move to group…" action which has clearer semantics.
  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 4 } }),
    useSensor(KeyboardSensor, {
      coordinateGetter: sortableKeyboardCoordinates,
    }),
  );

  const reorderWithinBucket = async (
    bucket: DataSource[],
    oldIndex: number,
    newIndex: number,
  ) => {
    const newBucketOrder = arrayMove(bucket, oldIndex, newIndex);
    // Detect which visual bucket was reordered (the ungrouped section
    // or one specific named group) by reference identity against the
    // already-grouped state.
    const isUngrouped =
      bucket.length === ungrouped.length &&
      bucket.every((s, i) => s.id === ungrouped[i].id);

    // Rebuild the full sources list in the same visual order:
    // ungrouped first, then each named group alphabetical. The
    // bucket whose drag fired gets its new in-order list; the other
    // buckets are passed through untouched.
    const newUngrouped = isUngrouped ? newBucketOrder : ungrouped;
    const newNamed = namedGroups.map(([_gname, members]) => {
      const isThisGroup =
        !isUngrouped &&
        bucket.length === members.length &&
        bucket.every((s, i) => s.id === members[i].id);
      return isThisGroup ? newBucketOrder : members;
    });
    const finalSources = [...newUngrouped, ...newNamed.flatMap((m) => m)];

    // Optimistic local re-stamp so the UI doesn't snap during the
    // round-trip. The Rust side is authoritative; a failed commit
    // triggers reloadConfig which snaps to truth.
    if (config) {
      const stamped = finalSources.map((s, i) => ({
        ...s,
        sort_order: i,
      }));
      setConfig({ ...config, sources: stamped });
    }

    // Persist the full ID order. The Rust impl rewrites sort_order
    // based on this list; matching shape means the post-reload is a
    // no-op when the call succeeds.
    try {
      await reorderSources(finalSources.map((s) => s.id));
    } catch (err) {
      toast.error(`Couldn't save new order: ${err}`);
      await reloadConfig();
    }
  };

  const makeBucketDragHandler =
    (bucket: DataSource[]) => async (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const oldIndex = bucket.findIndex((s) => s.id === active.id);
      const newIndex = bucket.findIndex((s) => s.id === over.id);
      if (oldIndex < 0 || newIndex < 0) return;
      await reorderWithinBucket(bucket, oldIndex, newIndex);
    };

  if (sources.length === 0) {
    return (
      <>
        <div className="flex h-full flex-col items-center justify-center p-8 text-center">
          <div className="mb-4 rounded-full bg-purple-100 p-4 dark:bg-purple-900/40">
            <Plus className="h-8 w-8 text-purple-600 dark:text-purple-300" />
          </div>
          <h2 className="mb-2 text-lg font-semibold text-slate-800 dark:text-slate-100">
            No sources yet
          </h2>
          <p className="mb-4 max-w-sm text-sm text-slate-600 dark:text-slate-400">
            Bookmark a folder, S3 bucket, or remote URL — Sery indexes the
            schema locally and never copies the data unless you ask.
          </p>
          <button
            onClick={() => setAddOpen(true)}
            className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-medium text-white shadow-sm transition-colors hover:bg-purple-700"
          >
            <Plus className="h-4 w-4" />
            Add a source
          </button>
        </div>
        <AddSourceModal
          open={addOpen}
          onClose={() => setAddOpen(false)}
          onAdded={reloadConfig}
        />
      </>
    );
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3 dark:border-slate-800">
        <div className="flex items-center gap-2">
          <h1 className="text-lg font-semibold text-slate-800 dark:text-slate-100">
            Sources
          </h1>
          {busy && (
            <Loader2 className="h-4 w-4 animate-spin text-slate-400" />
          )}
        </div>
        <div className="flex items-center gap-1.5">
          <button
            onClick={onScanAll}
            title="Rescan every source"
            className="inline-flex items-center gap-1 rounded-md border border-slate-200 bg-white px-2 py-1 text-xs font-medium text-slate-700 shadow-sm transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200 dark:hover:bg-slate-700"
          >
            <RefreshCw className="h-3.5 w-3.5" />
            Scan all
          </button>
          <button
            onClick={() => setAddOpen(true)}
            className="inline-flex items-center gap-1 rounded-md bg-purple-600 px-2.5 py-1 text-xs font-medium text-white shadow-sm transition-colors hover:bg-purple-700"
          >
            <Plus className="h-3.5 w-3.5" />
            Add source
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto p-3">
        {/* Ungrouped section — its own DndContext so drag reorders
            within the ungrouped bucket only. */}
        {ungrouped.length > 0 && (
          <DndContext
            sensors={dndSensors}
            collisionDetection={closestCenter}
            onDragEnd={makeBucketDragHandler(ungrouped)}
          >
            <SortableContext
              items={ungrouped.map((s) => s.id)}
              strategy={verticalListSortingStrategy}
            >
              <div className="space-y-1">
                {ungrouped.map((s) => renderRow(s, true))}
              </div>
            </SortableContext>
          </DndContext>
        )}
        {/* Named groups — each gets its OWN DndContext so within-
            group drag works but cross-bucket drag doesn't (cross-
            bucket moves use the "Move to group…" action which has
            clearer semantics for the group-membership change). */}
        {namedGroups.map(([groupName, members]) => (
          <SourceGroupSection
            key={groupName}
            groupName={groupName}
            sources={members}
            sensors={dndSensors}
            onDragEnd={makeBucketDragHandler(members)}
            renderRow={(s) => renderRow(s, true)}
          />
        ))}
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          source={sources.find((s) => s.id === menu.sourceId)!}
          onRename={onRename}
          onRescan={onRescan}
          onRevealInFinder={onRevealInFinder}
          onEditCredentials={onEditCredentials}
          onMoveToGroup={onMoveToGroup}
          onRemove={onRemove}
        />
      )}
      <AddSourceModal
        open={addOpen}
        onClose={() => setAddOpen(false)}
        onAdded={reloadConfig}
      />
      {groupPickerSourceId && (
        <GroupPickerDialog
          source={sources.find((s) => s.id === groupPickerSourceId)!}
          existingGroups={namedGroups.map(([name]) => name)}
          onCommit={(newGroup) =>
            commitGroupChange(
              sources.find((s) => s.id === groupPickerSourceId)!,
              newGroup,
            )
          }
          onCancel={() => setGroupPickerSourceId(null)}
        />
      )}
      {editCredsSourceId &&
        (() => {
          const target = sources.find((s) => s.id === editCredsSourceId);
          if (!target) return null;
          // S3 has its own dialog (different shape — works on URL,
          // and predates the cache-and-scan generalization).
          if (target.kind.kind === 's3') {
            return (
              <EditS3CredentialsDialog
                source={target}
                onClose={() => setEditCredsSourceId(null)}
                onSaved={reloadConfig}
              />
            );
          }
          // SFTP / WebDAV / Dropbox / Azure share the unified dialog.
          return (
            <EditCredentialsDialog
              source={target}
              onClose={() => setEditCredsSourceId(null)}
              onSaved={reloadConfig}
            />
          );
        })()}
    </div>
  );
}

// ─── Helper components ─────────────────────────────────────────────

interface SourceRowProps {
  source: DataSource;
  status: SourceStatus;
  editing: boolean;
  onCommitRename: (newName: string) => void;
  onCancelRename: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
  /** Optional drag-handle slot. Rendered to the left of the icon when
   *  the row is sortable; otherwise omitted so non-draggable rows
   *  (those inside a group section) keep their existing layout. */
  dragHandle?: React.ReactNode;
}

function SourceRow({
  source,
  status,
  editing,
  onCommitRename,
  onCancelRename,
  onContextMenu,
  dragHandle,
}: SourceRowProps) {
  const legacyKind = legacyKindStringOf(source);
  const datasetCount = source.last_scan_stats?.datasets ?? null;
  return (
    <div
      onContextMenu={onContextMenu}
      className="group flex cursor-pointer items-center gap-2 rounded-lg px-2 py-2 text-sm transition-colors hover:bg-slate-100 dark:hover:bg-slate-800"
    >
      {dragHandle}
      <div
        className={`flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md ${sourceIconBgClass(
          legacyKind,
        )}`}
      >
        <SourceIcon kind={legacyKind} size="sm" />
      </div>
      <div className="min-w-0 flex-1">
        {editing ? (
          <RenameInput
            initial={source.name}
            onCommit={onCommitRename}
            onCancel={onCancelRename}
          />
        ) : (
          <div className="truncate font-medium text-slate-800 dark:text-slate-100">
            {source.name}
          </div>
        )}
        <div className="flex items-center gap-1.5 truncate text-xs text-slate-500 dark:text-slate-400">
          <StatusPill status={status} />
          <span className="truncate">
            {sourceKindLabel(source)}
            {datasetCount !== null && ` · ${datasetCount.toLocaleString()} files`}
          </span>
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
  sensors: ReturnType<typeof useSensors>;
  onDragEnd: (event: DragEndEvent) => void;
  renderRow: (source: DataSource) => React.ReactNode;
}

function SourceGroupSection({
  groupName,
  sources,
  sensors,
  onDragEnd,
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
      {!collapsed && (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          onDragEnd={onDragEnd}
        >
          <SortableContext
            items={sources.map((s) => s.id)}
            strategy={verticalListSortingStrategy}
          >
            <div className="space-y-1">{sources.map(renderRow)}</div>
          </SortableContext>
        </DndContext>
      )}
    </div>
  );
}

interface ContextMenuProps {
  x: number;
  y: number;
  source: DataSource;
  onRename: (source: DataSource) => void;
  onRescan: (source: DataSource) => void;
  onRevealInFinder: (source: DataSource) => void;
  onEditCredentials: (source: DataSource) => void;
  onMoveToGroup: (source: DataSource) => void;
  onRemove: (source: DataSource) => void;
}

function ContextMenu({
  x,
  y,
  source,
  onRename,
  onRescan,
  onRevealInFinder,
  onEditCredentials,
  onMoveToGroup,
  onRemove,
}: ContextMenuProps) {
  // Stop propagation so clicks INSIDE the menu don't trigger the
  // outside-click close handler attached at window level.
  const isLocal = source.kind.kind === 'local';
  const hasEditableCreds =
    source.kind.kind === 's3' ||
    source.kind.kind === 'sftp' ||
    source.kind.kind === 'web_dav' ||
    source.kind.kind === 'dropbox' ||
    source.kind.kind === 'azure_blob';
  return (
    <div
      onClick={(e) => e.stopPropagation()}
      style={{ left: x, top: y }}
      className="fixed z-50 min-w-[12rem] rounded-md border border-slate-200 bg-white py-1 text-sm shadow-lg dark:border-slate-700 dark:bg-slate-900"
    >
      <MenuItem onClick={() => onRescan(source)}>Rescan now</MenuItem>
      <MenuItem onClick={() => onRename(source)}>Rename…</MenuItem>
      {hasEditableCreds && (
        <MenuItem onClick={() => onEditCredentials(source)}>
          Edit credentials…
        </MenuItem>
      )}
      <MenuItem onClick={() => onMoveToGroup(source)}>Move to group…</MenuItem>
      {isLocal && (
        <MenuItem onClick={() => onRevealInFinder(source)}>
          Show in Finder
        </MenuItem>
      )}
      <div className="my-1 h-px bg-slate-200 dark:bg-slate-700" />
      <MenuItem onClick={() => onRemove(source)} variant="danger">
        Remove source
      </MenuItem>
    </div>
  );
}

/** Sortable wrapper around <SourceRow>. Used only inside the
 *  ungrouped (top-level) section's <SortableContext>; rows inside
 *  named groups stay non-sortable for v0.7.0. */
function SortableSourceRow(props: SourceRowProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } =
    useSortable({ id: props.source.id });
  const style: React.CSSProperties = {
    transform: CSS.Transform.toString(transform),
    transition,
    opacity: isDragging ? 0.5 : 1,
  };
  const handle = (
    <button
      {...attributes}
      {...listeners}
      onClick={(e) => e.stopPropagation()}
      className="flex h-6 w-4 cursor-grab items-center justify-center text-slate-400 opacity-0 transition-opacity hover:text-slate-600 group-hover:opacity-100 active:cursor-grabbing dark:hover:text-slate-200"
      aria-label="Reorder source"
    >
      <GripVertical className="h-4 w-4" />
    </button>
  );
  return (
    <div ref={setNodeRef} style={style}>
      <SourceRow {...props} dragHandle={handle} />
    </div>
  );
}

/** Inline rename editor — replaces the row's name text with a focused
 *  input. Enter submits; Escape cancels; blur commits (matching
 *  Finder's rename UX). The parent owns the trim + no-op-if-unchanged
 *  policy, so this component is purely about input handling. */
function RenameInput({
  initial,
  onCommit,
  onCancel,
}: {
  initial: string;
  onCommit: (next: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initial);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // Focus + select-all on mount so the user can type-to-replace.
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  return (
    <input
      ref={inputRef}
      value={value}
      onChange={(e) => setValue(e.target.value)}
      onClick={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        if (e.key === 'Enter') {
          e.preventDefault();
          onCommit(value);
        } else if (e.key === 'Escape') {
          e.preventDefault();
          onCancel();
        }
      }}
      onBlur={() => onCommit(value)}
      className="w-full rounded border border-purple-400 bg-white px-1 py-0.5 text-sm font-medium text-slate-800 outline-none focus:border-purple-500 focus:ring-1 focus:ring-purple-500 dark:bg-slate-800 dark:text-slate-100"
    />
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

/** Modal-ish dialog for picking a group. Shows the existing groups
 *  as chip-style options + a "+ New group" inline input, plus a
 *  "Top level" choice that clears the group. Prefer this over
 *  window.prompt because (a) typing the same group name twice with
 *  inconsistent casing creates two near-identical groups, and (b)
 *  prompt() interrupts the rendered UI in a way that's especially
 *  ugly inside an Electron/Tauri shell. */
function GroupPickerDialog({
  source,
  existingGroups,
  onCommit,
  onCancel,
}: {
  source: DataSource;
  existingGroups: string[];
  onCommit: (group: string | null) => void;
  onCancel: () => void;
}) {
  const [newGroup, setNewGroup] = useState('');

  // Esc to cancel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onCancel();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onCancel]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onClick={onCancel}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-sm rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900"
      >
        <div className="border-b border-slate-200 px-4 py-3 dark:border-slate-800">
          <h3 className="text-sm font-semibold text-slate-800 dark:text-slate-100">
            Move "{source.name}" to…
          </h3>
        </div>
        <div className="space-y-2 p-4">
          {/* Top-level option — clears the group field. */}
          <button
            onClick={() => onCommit(null)}
            className={`block w-full rounded px-3 py-2 text-left text-sm transition-colors ${
              source.group === null
                ? 'bg-purple-50 font-medium text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                : 'text-slate-700 hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-800'
            }`}
          >
            Top level (no group)
          </button>
          {existingGroups.map((g) => (
            <button
              key={g}
              onClick={() => onCommit(g)}
              className={`block w-full rounded px-3 py-2 text-left text-sm transition-colors ${
                source.group === g
                  ? 'bg-purple-50 font-medium text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                  : 'text-slate-700 hover:bg-slate-100 dark:text-slate-200 dark:hover:bg-slate-800'
              }`}
            >
              {g}
            </button>
          ))}
          <div className="mt-3 border-t border-slate-200 pt-3 dark:border-slate-700">
            <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
              Or create new
            </label>
            <div className="flex gap-2">
              <input
                autoFocus
                type="text"
                value={newGroup}
                onChange={(e) => setNewGroup(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && newGroup.trim().length > 0) {
                    onCommit(newGroup.trim());
                  }
                }}
                placeholder="Group name"
                className="flex-1 rounded border border-slate-200 bg-white px-2 py-1.5 text-sm text-slate-800 placeholder-slate-400 focus:border-purple-500 focus:outline-none dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
              />
              <button
                onClick={() =>
                  newGroup.trim().length > 0 && onCommit(newGroup.trim())
                }
                disabled={newGroup.trim().length === 0}
                className="rounded bg-purple-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-50"
              >
                Add
              </button>
            </div>
          </div>
        </div>
        <div className="flex justify-end border-t border-slate-200 px-4 py-3 dark:border-slate-800">
          <button
            onClick={onCancel}
            className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}

/** Tiny colored dot that hints whether the source is healthy /
 *  scanning / waiting for its first scan. Plays the same role as
 *  the per-folder spinner on the Folders cards but in a denser
 *  one-line shape that fits the sidebar row. */
function StatusPill({ status }: { status: SourceStatus }) {
  switch (status) {
    case 'scanning':
      return (
        <span
          className="inline-flex h-2 w-2 flex-shrink-0 animate-pulse rounded-full bg-blue-500"
          aria-label="Scanning"
          title="Scanning"
        />
      );
    case 'online':
      return (
        <span
          className="inline-flex h-2 w-2 flex-shrink-0 rounded-full bg-emerald-500"
          aria-label="Online"
          title="Last scan succeeded"
        />
      );
    case 'pending':
      return (
        <span
          className="inline-flex h-2 w-2 flex-shrink-0 rounded-full bg-slate-400"
          aria-label="Pending"
          title="No scan yet"
        />
      );
  }
}
