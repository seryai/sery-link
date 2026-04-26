// Privacy / Activity tab — transparent record of every sync that has gone
// to the cloud, plus a disclosure of what's sent vs kept local. Backed by
// `get_sync_audit` (reads ~/.seryai/sync_audit.jsonl).

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertCircle,
  CheckCircle2,
  Cloud,
  Eye,
  FileDown,
  Folder,
  Lock,
  RefreshCw,
  Shield,
  Sparkles,
  Trash2,
  XCircle,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import type { AuditEntry } from '../types/events';

export function Privacy() {
  const { audit, setAudit, authenticated } = useAgentStore();
  const toast = useToast();
  const [loading, setLoading] = useState(false);
  const [auditPath, setAuditPath] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
    try {
      const entries = await invoke<AuditEntry[]>('get_sync_audit');
      setAudit(entries);
    } catch (err) {
      toast.error(`Couldn't load audit log: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  // Reveals the audit JSONL in the OS file manager AND returns the
  // absolute path so the UI can show users exactly where the file
  // lives — the load-bearing "verify it yourself" affordance for the
  // privacy story. Failures fall back to a toast; we still update the
  // path display if the call succeeds.
  const revealAuditFile = async () => {
    try {
      const path = await invoke<string>('reveal_audit_file_in_finder');
      setAuditPath(path);
    } catch (err) {
      toast.error(`Couldn't reveal audit file: ${err}`);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const clearAudit = async () => {
    if (!window.confirm('Clear the sync audit log? This cannot be undone.'))
      return;
    try {
      await invoke('clear_sync_audit');
      setAudit([]);
      toast.success('Audit log cleared');
    } catch (err) {
      toast.error(`Couldn't clear audit: ${err}`);
    }
  };

  const clearCloud = async () => {
    if (
      !window.confirm(
        'Delete all metadata this device has uploaded to Sery? ' +
          'You can re-sync at any time.',
      )
    )
      return;
    try {
      await invoke('clear_cloud_metadata');
      toast.success('Cloud metadata cleared');
    } catch (err) {
      toast.error(`Couldn't clear cloud metadata: ${err}`);
    }
  };

  const exportBundle = async () => {
    try {
      const path = await invoke<string>('export_diagnostic_bundle');
      toast.success(`Saved to ${path}`);
    } catch (err) {
      toast.error(`Export failed: ${err}`);
    }
  };

  const totals = useMemo(() => {
    let datasets = 0;
    let columns = 0;
    let bytes = 0;
    let errors = 0;
    let syncs = 0;
    let byokCalls = 0;
    for (const e of audit) {
      const kind = e.kind ?? 'sync';
      if (kind === 'byok_call') {
        byokCalls++;
      } else {
        syncs++;
        datasets += e.dataset_count;
        columns += e.column_count;
        bytes += e.total_bytes;
      }
      if (e.status === 'error') errors++;
    }
    return { datasets, columns, bytes, errors, syncs, byokCalls };
  }, [audit]);

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <Shield className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Privacy &amp; Activity
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Full transparency into what this device has shared with Sery.
            </p>
          </div>
          <button
            onClick={refresh}
            disabled={loading}
            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <RefreshCw className={`h-4 w-4 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">

      {/* Local-only state banner — make sure users in local mode
          know the "goes to cloud" card is hypothetical. */}
      {!authenticated && (
        <div className="mb-4 flex items-start gap-2 rounded-lg border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-900 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-200">
          <Lock className="mt-0.5 h-4 w-4 flex-shrink-0" />
          <div>
            You're running Sery Link locally. Nothing below has been
            sent anywhere — these cards show what <em>would</em> cross
            the network if you connect to Sery.ai.
          </div>
        </div>
      )}

      {/* Disclosure cards */}
      <div className="mb-6 grid gap-3 md:grid-cols-2">
        <DisclosureCard
          tone="sent"
          icon={<Cloud className="h-5 w-5" />}
          title={authenticated ? 'What goes to the cloud' : 'What would cross the network'}
          items={[
            'File paths (relative to watched folders)',
            'Schemas — column names and types',
            'Row counts and file sizes',
            'Results of queries you run',
          ]}
        />
        <DisclosureCard
          tone="kept"
          icon={<Lock className="h-5 w-5" />}
          title="What stays on this device"
          items={[
            'Raw file contents',
            'Files outside watched folders',
            'Your OS credentials and environment',
            'Files matching exclude patterns',
          ]}
        />
      </div>

      {/* Totals */}
      <div className="mb-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <TotalCard label="Syncs" value={totals.syncs.toLocaleString()} />
        <TotalCard
          label="BYOK calls"
          value={totals.byokCalls.toLocaleString()}
        />
        <TotalCard
          label="Columns shared"
          value={totals.columns.toLocaleString()}
        />
        <TotalCard
          label="Errors"
          value={totals.errors.toLocaleString()}
          tone={totals.errors > 0 ? 'warn' : undefined}
        />
      </div>

      {/* Actions */}
      <div className="mb-6 flex flex-wrap gap-2">
        <button
          onClick={revealAuditFile}
          className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          title="Open the folder containing sync_audit.jsonl in your file manager so you can verify the contents directly"
        >
          <Folder className="h-4 w-4" />
          Reveal audit file
        </button>
        <button
          onClick={exportBundle}
          className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
        >
          <FileDown className="h-4 w-4" />
          Export diagnostic bundle
        </button>
        <button
          onClick={clearAudit}
          disabled={audit.length === 0}
          className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
        >
          <Trash2 className="h-4 w-4" />
          Clear local audit log
        </button>
        <button
          onClick={clearCloud}
          className="flex items-center gap-2 rounded-lg border border-rose-300 bg-white px-3 py-2 text-sm font-medium text-rose-700 transition-colors hover:bg-rose-50 dark:border-rose-900 dark:bg-slate-900 dark:text-rose-300 dark:hover:bg-rose-950/40"
        >
          <XCircle className="h-4 w-4" />
          Delete cloud metadata
        </button>
      </div>

      {/* Audit file path — only shown after the user clicks Reveal,
          so the path doesn't get leaked into screenshots by default
          but is one click away when wanted. */}
      {auditPath && (
        <div className="mb-6 rounded-lg border border-slate-200 bg-slate-50 px-3 py-2 text-xs text-slate-600 dark:border-slate-800 dark:bg-slate-900 dark:text-slate-400">
          <div className="font-medium text-slate-700 dark:text-slate-300 mb-0.5">
            Audit file lives at:
          </div>
          <code className="font-mono break-all">{auditPath}</code>
        </div>
      )}

      {/* Audit list */}
      <div className="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-900 dark:text-slate-100">
        <Eye className="h-4 w-4 text-slate-500" />
        Outbound activity
      </div>

      {audit.length === 0 ? (
        <div className="rounded-xl border border-dashed border-slate-300 bg-slate-50 py-12 text-center dark:border-slate-700 dark:bg-slate-900">
          <Shield className="mx-auto mb-3 h-10 w-10 text-slate-300 dark:text-slate-600" />
          <p className="text-sm font-medium text-slate-700 dark:text-slate-300">
            No syncs yet
          </p>
          <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
            Every metadata upload will be logged here.
          </p>
        </div>
      ) : (
        <div className="overflow-hidden rounded-xl border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
          {audit.map((entry, idx) => (
            <AuditRow
              key={`${entry.timestamp}-${idx}`}
              entry={entry}
              isLast={idx === audit.length - 1}
            />
          ))}
        </div>
      )}
      </div>
    </div>
  );
}

// ─── Pieces ─────────────────────────────────────────────────────────────────

function DisclosureCard({
  tone,
  icon,
  title,
  items,
}: {
  tone: 'sent' | 'kept';
  icon: React.ReactNode;
  title: string;
  items: string[];
}) {
  const classes = {
    sent: {
      border: 'border-emerald-200 dark:border-emerald-900',
      bg: 'bg-emerald-50/50 dark:bg-emerald-950/20',
      icon: 'text-emerald-600 dark:text-emerald-400',
      title: 'text-emerald-900 dark:text-emerald-100',
    },
    kept: {
      border: 'border-purple-200 dark:border-purple-900',
      bg: 'bg-purple-50/50 dark:bg-purple-950/20',
      icon: 'text-purple-600 dark:text-purple-400',
      title: 'text-purple-900 dark:text-purple-100',
    },
  }[tone];

  return (
    <div className={`rounded-xl border ${classes.border} ${classes.bg} p-4`}>
      <div className="mb-2 flex items-center gap-2">
        <span className={classes.icon}>{icon}</span>
        <span className={`text-sm font-semibold ${classes.title}`}>{title}</span>
      </div>
      <ul className="space-y-1">
        {items.map((item) => (
          <li
            key={item}
            className="flex items-start gap-2 text-xs text-slate-700 dark:text-slate-300"
          >
            <span className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-slate-400 dark:bg-slate-500" />
            {item}
          </li>
        ))}
      </ul>
    </div>
  );
}

function TotalCard({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: 'warn';
}) {
  const valueClass =
    tone === 'warn'
      ? 'text-amber-600 dark:text-amber-400'
      : 'text-slate-900 dark:text-slate-100';
  return (
    <div className="rounded-xl border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <div className="text-[10px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {label}
      </div>
      <div className={`mt-1 text-2xl font-bold ${valueClass}`}>{value}</div>
    </div>
  );
}

function AuditRow({
  entry,
  isLast,
}: {
  entry: AuditEntry;
  isLast: boolean;
}) {
  const ok = entry.status === 'success';
  const kind = entry.kind ?? 'sync';
  const isByok = kind === 'byok_call';

  return (
    <div
      className={`flex items-start gap-3 px-4 py-3 ${
        isLast ? '' : 'border-b border-slate-100 dark:border-slate-800'
      }`}
    >
      <div className="mt-0.5 shrink-0">
        {!ok ? (
          <AlertCircle className="h-5 w-5 text-rose-500" />
        ) : isByok ? (
          // Lock icon for BYOK — the structural privacy proof. If a
          // user wonders "did this call really skip Sery?" the row's
          // host badge below answers it: api.anthropic.com, not sery.ai.
          <Lock className="h-5 w-5 text-emerald-500" />
        ) : (
          <CheckCircle2 className="h-5 w-5 text-emerald-500" />
        )}
      </div>
      <div className="min-w-0 flex-1">
        {isByok ? (
          <ByokRowBody entry={entry} ok={ok} />
        ) : (
          <SyncRowBody entry={entry} ok={ok} />
        )}
      </div>
    </div>
  );
}

function SyncRowBody({ entry, ok }: { entry: AuditEntry; ok: boolean }) {
  return (
    <>
      <div
        className="truncate text-sm font-medium text-slate-900 dark:text-slate-100"
        title={entry.folder}
      >
        {folderBasename(entry.folder)}
      </div>
      <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 dark:text-slate-400">
        <span className="inline-flex items-center gap-1 rounded bg-slate-100 px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide text-slate-600 dark:bg-slate-800 dark:text-slate-300">
          <Cloud className="h-2.5 w-2.5" />
          sync
        </span>
        <span>{new Date(entry.timestamp).toLocaleString()}</span>
        {ok ? (
          <>
            <span>·</span>
            <span>{entry.dataset_count.toLocaleString()} datasets</span>
            <span>·</span>
            <span>{entry.column_count.toLocaleString()} columns</span>
            <span>·</span>
            <span>{formatBytes(entry.total_bytes)}</span>
          </>
        ) : (
          <span className="text-rose-600 dark:text-rose-400">
            {entry.error ?? 'Unknown error'}
          </span>
        )}
      </div>
    </>
  );
}

function ByokRowBody({ entry, ok }: { entry: AuditEntry; ok: boolean }) {
  const provider = entry.provider ?? 'unknown';
  const host = entry.host ?? 'unknown';
  const promptChars = entry.prompt_chars ?? 0;
  const responseChars = entry.response_chars ?? 0;
  const durationMs = entry.duration_ms ?? 0;
  return (
    <>
      <div className="flex items-center gap-2 text-sm font-medium text-slate-900 dark:text-slate-100">
        <span>BYOK call to</span>
        {/* The host pill is the load-bearing privacy artifact. It's
            emerald-toned to signal "this stayed off our servers,"
            and the value is the literal host the request targeted —
            if it ever reads `*.sery.ai` for a byok_call the privacy
            guarantee is broken (and the unit test in
            byok::anthropic would have failed first). */}
        <span className="inline-flex items-center gap-1 rounded bg-emerald-100 px-1.5 py-0.5 font-mono text-xs text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300">
          <Lock className="h-3 w-3" />
          {host}
        </span>
        <span className="text-xs text-slate-500 dark:text-slate-400">
          ({provider})
        </span>
      </div>
      <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-slate-500 dark:text-slate-400">
        <span className="inline-flex items-center gap-1 rounded bg-purple-100 px-1 py-0.5 text-[10px] font-medium uppercase tracking-wide text-purple-700 dark:bg-purple-900/40 dark:text-purple-300">
          <Sparkles className="h-2.5 w-2.5" />
          byok
        </span>
        <span>{new Date(entry.timestamp).toLocaleString()}</span>
        {ok ? (
          <>
            <span>·</span>
            <span>{promptChars.toLocaleString()} chars in</span>
            <span>·</span>
            <span>{responseChars.toLocaleString()} chars out</span>
            <span>·</span>
            <span>{durationMs}ms</span>
          </>
        ) : (
          <span className="text-rose-600 dark:text-rose-400">
            {entry.error ?? 'Unknown error'}
          </span>
        )}
      </div>
    </>
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
