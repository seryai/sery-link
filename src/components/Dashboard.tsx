import { useNavigate } from 'react-router-dom';
import { Database, FolderOpen, HardDrive, Cloud } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { SourceIcon } from './SourceIcon';
import { legacyKindStringOf } from '../utils/sources';

const CLOUD_KINDS = new Set([
  's3',
  'google_drive',
  'sftp',
  'web_dav',
  'dropbox',
  'azure_blob',
  'one_drive',
  'https',
]);
const DB_KINDS = new Set([
  'mysql', 'postgresql', 'snowflake', 'clickhouse', 'mongodb', 'redis', 'sqlite',
  'agent_db',
]);

// ── helpers ────────────────────────────────────────────────────────────────

function fmtBytes(n: number): string {
  if (n === 0) return '0 B';
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(1)} GB`;
}


function fmtDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(1)}s`;
}

function fileBasename(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

// ── component ──────────────────────────────────────────────────────────────

export function Dashboard() {
  const { config, stats, history } = useAgentStore();
  const navigate = useNavigate();
  const sources = config?.sources ?? [];

  const local     = sources.filter((s) => s.kind.kind === 'local');
  const cloud     = sources.filter((s) => CLOUD_KINDS.has(s.kind.kind));
  const databases = sources.filter((s) => DB_KINDS.has(s.kind.kind));

  const totalDatasets = sources.reduce((n, s) => n + (s.last_scan_stats?.datasets ?? 0), 0);
  const totalColumns  = sources.reduce((n, s) => n + (s.last_scan_stats?.columns  ?? 0), 0);
  const totalBytes    = sources.reduce((n, s) => n + (s.last_scan_stats?.total_bytes ?? 0), 0);
  const localBytes    = local.reduce((n, s) => n + (s.last_scan_stats?.total_bytes ?? 0), 0);
  const cloudDatasets = cloud.reduce((n, s) => n + (s.last_scan_stats?.datasets ?? 0), 0);
  const dbTables      = databases.reduce((n, s) => n + (s.last_scan_stats?.datasets ?? 0), 0);

  const successRate =
    stats && stats.total_queries > 0
      ? Math.round((stats.successful_queries / stats.total_queries) * 100)
      : null;

  const successfulHistory = history.filter(
    (e) => e.status === 'success' && e.duration_ms > 0,
  );
  const avgDuration =
    successfulHistory.length > 0
      ? Math.round(
          successfulHistory.reduce((n, e) => n + e.duration_ms, 0) /
            successfulHistory.length,
        )
      : null;

  return (
    <div className="p-6 space-y-8">
      <h1 className="text-xl font-semibold text-slate-800 dark:text-slate-100">
        Overview
      </h1>

      {/* ── Stats cards ── */}
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <StatCard
          icon={<FolderOpen className="h-5 w-5 text-blue-600" />}
          label="Local folders"
          value={local.length}
          sub={localBytes > 0 ? fmtBytes(localBytes) : undefined}
          bg="bg-blue-50 dark:bg-blue-900/20"
        />
        <StatCard
          icon={<Cloud className="h-5 w-5 text-purple-600" />}
          label="Cloud sources"
          value={cloud.length}
          sub={cloudDatasets > 0 ? `${cloudDatasets.toLocaleString()} datasets` : undefined}
          bg="bg-purple-50 dark:bg-purple-900/20"
        />
        <StatCard
          icon={<Database className="h-5 w-5 text-emerald-600" />}
          label="Databases"
          value={databases.length}
          sub={dbTables > 0 ? `${dbTables.toLocaleString()} tables` : undefined}
          bg="bg-emerald-50 dark:bg-emerald-900/20"
        />
        <StatCard
          icon={<HardDrive className="h-5 w-5 text-slate-600" />}
          label="Total datasets"
          value={totalDatasets.toLocaleString()}
          sub={
            totalColumns > 0
              ? `${totalColumns.toLocaleString()} cols · ${fmtBytes(totalBytes)}`
              : undefined
          }
          bg="bg-slate-50 dark:bg-slate-800/50"
        />
      </div>

      {/* ── Query health ── */}
      {stats && stats.total_queries > 0 && (
        <div>
          <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300 mb-3">
            Query health
          </h2>
          <div className="flex flex-wrap gap-2">
            <MetricPill label="Today" value={String(stats.queries_today)} />
            <MetricPill label="All time" value={stats.total_queries.toLocaleString()} />
            {successRate !== null && (
              <MetricPill
                label="Success rate"
                value={`${successRate}%`}
                color={
                  successRate >= 90
                    ? 'text-emerald-700 dark:text-emerald-300 bg-emerald-50 dark:bg-emerald-900/20'
                    : successRate >= 70
                      ? 'text-amber-700 dark:text-amber-300 bg-amber-50 dark:bg-amber-900/20'
                      : 'text-rose-700 dark:text-rose-300 bg-rose-50 dark:bg-rose-900/20'
                }
              />
            )}
            {avgDuration !== null && (
              <MetricPill label="Avg duration" value={fmtDuration(avgDuration)} />
            )}
            {stats.failed_queries > 0 && (
              <MetricPill
                label="Errors"
                value={String(stats.failed_queries)}
                color="text-rose-700 dark:text-rose-300 bg-rose-50 dark:bg-rose-900/20"
              />
            )}
          </div>
        </div>
      )}

      {/* ── Disk usage ── */}
      {totalBytes > 0 && (
        <div>
          <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300 mb-3">
            Disk usage
          </h2>
          <div className="space-y-2">
            {[...sources]
              .filter((s) => (s.last_scan_stats?.total_bytes ?? 0) > 0)
              .sort((a, b) => (b.last_scan_stats?.total_bytes ?? 0) - (a.last_scan_stats?.total_bytes ?? 0))
              .map((source) => {
                const bytes = source.last_scan_stats?.total_bytes ?? 0;
                const pct = totalBytes > 0 ? (bytes / totalBytes) * 100 : 0;
                return (
                  <div key={source.id} className="flex items-center gap-3">
                    <SourceIcon kind={legacyKindStringOf(source)} size="sm" />
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-xs font-medium text-slate-700 dark:text-slate-300 truncate">
                          {source.name}
                        </span>
                        <span className="text-xs text-slate-400 dark:text-slate-500 ml-2 flex-shrink-0">
                          {fmtBytes(bytes)}
                        </span>
                      </div>
                      <div className="h-1.5 w-full rounded-full bg-slate-100 dark:bg-slate-800">
                        <div
                          className="h-1.5 rounded-full bg-purple-500 dark:bg-purple-400"
                          style={{ width: `${Math.max(pct, 0.5)}%` }}
                        />
                      </div>
                    </div>
                    <span className="text-[11px] text-slate-400 dark:text-slate-500 w-8 text-right flex-shrink-0">
                      {pct < 1 ? '<1' : Math.round(pct)}%
                    </span>
                  </div>
                );
              })}
          </div>
          {totalBytes > 0 && (
            <p className="mt-2 text-right text-xs text-slate-400 dark:text-slate-500">
              {fmtBytes(totalBytes)} total
            </p>
          )}
        </div>
      )}

      {/* ── Recent queries ── */}
      {history.length > 0 && (
        <div>
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300">
              Recent queries
            </h2>
            <button
              onClick={() => navigate('/history')}
              className="text-xs text-purple-600 hover:text-purple-700 dark:text-purple-400"
            >
              View all →
            </button>
          </div>
          <div className="space-y-2">
            {history.slice(0, 5).map((entry) => (
              <div
                key={entry.query_id ?? entry.timestamp}
                className="rounded-lg border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800/50 px-3 py-2.5"
              >
                <div className="flex items-center justify-between gap-2 mb-1">
                  <p className="text-xs text-slate-700 dark:text-slate-300 truncate font-mono">
                    {entry.sql || '—'}
                  </p>
                  <span
                    className={`flex-shrink-0 text-[10px] font-medium px-1.5 py-0.5 rounded-full ${
                      entry.status === 'success'
                        ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300'
                        : 'bg-rose-100 text-rose-700 dark:bg-rose-900/30 dark:text-rose-300'
                    }`}
                  >
                    {entry.status}
                  </span>
                </div>
                <div className="flex items-center gap-3 text-[11px] text-slate-400 dark:text-slate-500">
                  {entry.file_path && (
                    <span className="truncate max-w-[180px]" title={entry.file_path}>
                      {fileBasename(entry.file_path)}
                    </span>
                  )}
                  {entry.row_count != null && (
                    <span>{entry.row_count.toLocaleString()} rows</span>
                  )}
                  {entry.duration_ms > 0 && (
                    <span>{fmtDuration(entry.duration_ms)}</span>
                  )}
                  <span className="ml-auto">{new Date(entry.timestamp).toLocaleString()}</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      )}

      {sources.length === 0 && (
        <div className="text-center py-16 text-slate-400 dark:text-slate-500">
          <p className="text-sm">
            No sources yet. Click <strong>+</strong> in the toolbar to add one.
          </p>
        </div>
      )}
    </div>
  );
}

// ── sub-components ─────────────────────────────────────────────────────────

function StatCard({
  icon,
  label,
  value,
  sub,
  bg,
}: {
  icon: React.ReactNode;
  label: string;
  value: number | string;
  sub?: string;
  bg: string;
}) {
  return (
    <div className={`rounded-xl p-4 ${bg}`}>
      <div className="flex items-center gap-2 mb-2">
        {icon}
        <span className="text-xs font-medium text-slate-600 dark:text-slate-300">
          {label}
        </span>
      </div>
      <p className="text-2xl font-bold text-slate-800 dark:text-slate-100">
        {value}
      </p>
      {sub && (
        <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5 truncate">
          {sub}
        </p>
      )}
    </div>
  );
}

function MetricPill({
  label,
  value,
  color = 'text-slate-700 dark:text-slate-200 bg-slate-100 dark:bg-slate-700/50',
}: {
  label: string;
  value: string;
  color?: string;
}) {
  return (
    <div className={`flex items-center gap-1.5 rounded-lg px-3 py-1.5 ${color}`}>
      <span className="text-xs text-slate-500 dark:text-slate-400">{label}</span>
      <span className="text-sm font-semibold">{value}</span>
    </div>
  );
}
