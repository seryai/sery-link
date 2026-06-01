import { useNavigate } from 'react-router-dom';
import { AlertTriangle, Database, FolderOpen, HardDrive, Cloud } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { SourceIcon } from './SourceIcon';
import { legacyKindStringOf, scanKeyOf } from '../utils/sources';

const CLOUD_KINDS = new Set([
  's3', 'google_drive', 'sftp', 'web_dav', 'dropbox',
  'azure_blob', 'one_drive', 'https',
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

function dayKey(iso: string): string {
  return iso.slice(0, 10); // YYYY-MM-DD
}

function last7Days(): string[] {
  const days: string[] = [];
  for (let i = 6; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    days.push(d.toISOString().slice(0, 10));
  }
  return days;
}

function shortDay(isoDate: string): string {
  const d = new Date(isoDate + 'T00:00:00');
  return d.toLocaleDateString(undefined, { weekday: 'short' }).slice(0, 3);
}

// ── component ──────────────────────────────────────────────────────────────

export function Dashboard() {
  const { config, stats, history, scansInFlight } = useAgentStore();
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

  // ── needs attention ───────────────────────────────────────────────────────
  const attention = sources.filter((s) => {
    const scanning = s.id in scansInFlight || (scanKeyOf(s) ?? '') in scansInFlight;
    if (scanning) return false;
    if ((s.last_scan_stats?.errors ?? 0) > 0) return true;
    if (!s.last_scan_at) return true;
    const age = Date.now() - new Date(s.last_scan_at).getTime();
    return age > 24 * 60 * 60 * 1000;
  });

  // ── activity chart ────────────────────────────────────────────────────────
  const days = last7Days();
  const countByDay = new Map<string, number>();
  for (const e of history) {
    const k = dayKey(e.timestamp);
    countByDay.set(k, (countByDay.get(k) ?? 0) + 1);
  }
  const dayCounts = days.map((d) => ({ day: d, count: countByDay.get(d) ?? 0 }));
  const maxCount  = Math.max(...dayCounts.map((d) => d.count), 1);
  const totalActivity = dayCounts.reduce((n, d) => n + d.count, 0);

  // ── disk usage ────────────────────────────────────────────────────────────
  const sourcesWithBytes = [...sources]
    .filter((s) => (s.last_scan_stats?.total_bytes ?? 0) > 0)
    .sort((a, b) => (b.last_scan_stats?.total_bytes ?? 0) - (a.last_scan_stats?.total_bytes ?? 0));

  // ── query stats ───────────────────────────────────────────────────────────
  const successRate =
    stats && stats.total_queries > 0
      ? Math.round((stats.successful_queries / stats.total_queries) * 100)
      : null;

  const successfulHistory = history.filter((e) => e.status === 'success' && e.duration_ms > 0);
  const avgDuration =
    successfulHistory.length > 0
      ? Math.round(successfulHistory.reduce((n, e) => n + e.duration_ms, 0) / successfulHistory.length)
      : null;

  function openSource(s: typeof sources[0]) {
    if (DB_KINDS.has(s.kind.kind)) {
      navigate(`/db/${encodeURIComponent(s.id)}`);
    } else {
      const key = scanKeyOf(s);
      if (key) navigate(`/folders/${encodeURIComponent(key)}`);
    }
  }

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

      {/* ── Needs attention ── */}
      {attention.length > 0 && (
        <div>
          <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300 mb-3 flex items-center gap-1.5">
            <AlertTriangle className="h-3.5 w-3.5 text-amber-500" />
            Needs attention
          </h2>
          <div className="rounded-xl border border-amber-200 dark:border-amber-900/50 overflow-hidden divide-y divide-amber-100 dark:divide-amber-900/30">
            {attention.map((s) => {
              const errors  = s.last_scan_stats?.errors ?? 0;
              const isError = errors > 0;
              const label   = !s.last_scan_at
                ? 'Never scanned'
                : isError
                  ? `${errors} error${errors !== 1 ? 's' : ''}`
                  : 'Stale — not scanned in 24 h';
              return (
                <button
                  key={s.id}
                  onClick={() => openSource(s)}
                  className="w-full flex items-center gap-3 px-4 py-2.5 bg-amber-50/60 hover:bg-amber-50 dark:bg-amber-950/20 dark:hover:bg-amber-950/30 text-left transition-colors"
                >
                  <SourceIcon kind={legacyKindStringOf(s)} size="sm" />
                  <span className="flex-1 min-w-0 text-sm font-medium text-slate-800 dark:text-slate-100 truncate">
                    {s.name}
                  </span>
                  <span className={`text-xs flex-shrink-0 ${isError ? 'text-rose-500' : 'text-amber-600 dark:text-amber-400'}`}>
                    {label}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* ── Activity chart (7-day query bar chart) ── */}
      {totalActivity > 0 && (
        <div>
          <div className="flex items-end justify-between mb-3">
            <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300">
              Query activity
            </h2>
            <span className="text-xs text-slate-400 dark:text-slate-500">
              {totalActivity} {totalActivity === 1 ? 'query' : 'queries'} this week
              {successRate !== null && ` · ${successRate}% success`}
              {avgDuration !== null && ` · avg ${fmtDuration(avgDuration)}`}
            </span>
          </div>
          <div className="rounded-xl border border-slate-200 dark:border-slate-700 bg-white dark:bg-slate-800/40 px-4 pt-4 pb-3">
            <div className="flex items-end gap-1.5 h-24">
              {dayCounts.map(({ day, count }) => {
                const heightPct = count === 0 ? 0 : Math.max((count / maxCount) * 100, 8);
                const isToday = day === new Date().toISOString().slice(0, 10);
                return (
                  <div key={day} className="flex-1 flex flex-col items-center gap-1 h-full justify-end">
                    <span className="text-[10px] text-slate-400 dark:text-slate-500 tabular-nums">
                      {count > 0 ? count : ''}
                    </span>
                    <div className="w-full rounded-t-sm transition-all" style={{ height: `${heightPct}%` }}>
                      <div
                        className={`w-full h-full rounded-t-sm ${
                          count === 0
                            ? 'bg-slate-100 dark:bg-slate-700/40'
                            : isToday
                              ? 'bg-purple-500 dark:bg-purple-400'
                              : 'bg-purple-300 dark:bg-purple-600/60'
                        }`}
                      />
                    </div>
                  </div>
                );
              })}
            </div>
            <div className="flex gap-1.5 mt-1.5">
              {dayCounts.map(({ day }) => {
                const isToday = day === new Date().toISOString().slice(0, 10);
                return (
                  <div key={day} className="flex-1 text-center">
                    <span className={`text-[10px] ${isToday ? 'font-semibold text-purple-600 dark:text-purple-400' : 'text-slate-400 dark:text-slate-500'}`}>
                      {isToday ? 'Today' : shortDay(day)}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}

      {/* ── Disk usage ── */}
      {sourcesWithBytes.length > 0 && (
        <div>
          <h2 className="text-sm font-semibold text-slate-700 dark:text-slate-300 mb-3">
            Disk usage
          </h2>
          <div className="space-y-2.5">
            {sourcesWithBytes.map((source) => {
              const bytes = source.last_scan_stats?.total_bytes ?? 0;
              const pct   = totalBytes > 0 ? (bytes / totalBytes) * 100 : 0;
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
                        className="h-1.5 rounded-full bg-purple-400 dark:bg-purple-500"
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
          <p className="mt-2 text-right text-xs text-slate-400 dark:text-slate-500">
            {fmtBytes(totalBytes)} total
          </p>
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
  icon, label, value, sub, bg,
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
        <span className="text-xs font-medium text-slate-600 dark:text-slate-300">{label}</span>
      </div>
      <p className="text-2xl font-bold text-slate-800 dark:text-slate-100">{value}</p>
      {sub && (
        <p className="text-xs text-slate-500 dark:text-slate-400 mt-0.5 truncate">{sub}</p>
      )}
    </div>
  );
}
