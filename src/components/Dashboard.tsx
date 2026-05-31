import { useNavigate } from 'react-router-dom';
import { Database, FolderOpen, HardDrive, Cloud } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';

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
  'mysql',
  'postgresql',
  'snowflake',
  'clickhouse',
  'mongodb',
  'redis',
  'sqlite',
]);

export function Dashboard() {
  const { config, stats, history } = useAgentStore();
  const navigate = useNavigate();
  const sources = config?.sources ?? [];

  const local = sources.filter((s) => s.kind.kind === 'local');
  const cloud = sources.filter((s) => CLOUD_KINDS.has(s.kind.kind));
  const databases = sources.filter((s) => DB_KINDS.has(s.kind.kind));
  const totalDatasets = sources.reduce(
    (n, s) => n + (s.last_scan_stats?.datasets ?? 0),
    0,
  );

  return (
    <div className="p-6">
      <h1 className="text-xl font-semibold text-slate-800 dark:text-slate-100 mb-6">
        Overview
      </h1>

      {/* Stats row */}
      <div className="grid grid-cols-2 gap-3 mb-8 lg:grid-cols-4">
        <StatCard
          icon={<FolderOpen className="h-5 w-5 text-blue-600" />}
          label="Local folders"
          value={local.length}
          bg="bg-blue-50 dark:bg-blue-900/20"
        />
        <StatCard
          icon={<Cloud className="h-5 w-5 text-purple-600" />}
          label="Cloud sources"
          value={cloud.length}
          bg="bg-purple-50 dark:bg-purple-900/20"
        />
        <StatCard
          icon={<Database className="h-5 w-5 text-emerald-600" />}
          label="Databases"
          value={databases.length}
          bg="bg-emerald-50 dark:bg-emerald-900/20"
        />
        <StatCard
          icon={<HardDrive className="h-5 w-5 text-slate-600" />}
          label="Total datasets"
          value={totalDatasets.toLocaleString()}
          bg="bg-slate-50 dark:bg-slate-800/50"
        />
      </div>

      {/* Queries today pill */}
      {stats && stats.queries_today > 0 && (
        <p className="mb-6 text-sm text-slate-500 dark:text-slate-400">
          <span className="font-semibold text-slate-700 dark:text-slate-200">
            {stats.queries_today}
          </span>{' '}
          {stats.queries_today === 1 ? 'query' : 'queries'} run today
        </p>
      )}

      {/* Recent queries */}
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
                <div className="flex items-center justify-between gap-2">
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
                <p className="text-[11px] text-slate-400 mt-0.5">
                  {new Date(entry.timestamp).toLocaleString()}
                </p>
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

function StatCard({
  icon,
  label,
  value,
  bg,
}: {
  icon: React.ReactNode;
  label: string;
  value: number | string;
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
    </div>
  );
}
