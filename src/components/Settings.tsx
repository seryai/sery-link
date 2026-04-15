// Settings panel — tabbed layout with General, Sync, App, and About sections.
// All changes are staged in local state and only persisted when the user
// clicks Save. Certain toggles (auto-sync, launch-at-login) also trigger a
// backend side-effect on save.

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  Bell,
  Download,
  Info,
  LogOut,
  Monitor,
  Moon,
  Power,
  RefreshCw,
  Save,
  Settings as SettingsIcon,
  Sun,
  Upload,
  Zap,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { PluginsPanel } from './PluginsPanel';
import type { AgentConfig } from '../types/events';

type Tab = 'general' | 'sync' | 'app' | 'plugins' | 'about';

export function Settings() {
  const { config, setConfig, agentInfo, setAuthenticated, setAgentInfo } =
    useAgentStore();
  const toast = useToast();
  const [tab, setTab] = useState<Tab>('general');
  const [draft, setDraft] = useState<AgentConfig | null>(config);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setDraft(config);
  }, [config]);

  const dirty = useMemo(() => {
    if (!config || !draft) return false;
    return JSON.stringify(config) !== JSON.stringify(draft);
  }, [config, draft]);

  const save = async () => {
    if (!draft) return;
    setSaving(true);
    try {
      await invoke('save_config', { config: draft });
      setConfig(draft);

      // Side effects that depend on the saved config
      try {
        await invoke('restart_file_watcher');
      } catch (err) {
        console.error('Failed to restart file watcher:', err);
      }
      try {
        await invoke('set_launch_at_login', {
          enabled: draft.app.launch_at_login,
        });
      } catch (err) {
        console.error('Failed to toggle launch-at-login:', err);
      }

      toast.success('Settings saved');
    } catch (err) {
      toast.error(`Couldn't save settings: ${err}`);
    } finally {
      setSaving(false);
    }
  };

  const logout = async () => {
    if (!window.confirm('Sign out of Sery on this device?')) return;
    try {
      await invoke('logout');
      setAuthenticated(false);
      setAgentInfo(null);
      toast.info('Signed out');
    } catch (err) {
      toast.error(`Couldn't sign out: ${err}`);
    }
  };

  const exportConfig = async () => {
    if (!agentInfo?.workspace_id) {
      toast.error('Workspace ID not available');
      return;
    }
    try {
      const json = await invoke<string>('export_configuration', {
        workspaceId: agentInfo.workspace_id,
      });
      const blob = new Blob([json], { type: 'application/json' });
      const url = URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = `sery-config-${new Date().toISOString().split('T')[0]}.json`;
      a.click();
      URL.revokeObjectURL(url);
      toast.success('Configuration exported');
    } catch (err) {
      toast.error(`Export failed: ${err}`);
    }
  };

  const importConfig = async () => {
    if (!agentInfo?.workspace_id) {
      toast.error('Workspace ID not available');
      return;
    }
    try {
      const file = await openDialog({
        multiple: false,
        filters: [{ name: 'JSON', extensions: ['json'] }],
      });
      if (!file) return;

      const contents = await invoke<string>('read_file', { path: file });

      // Validate first
      await invoke('validate_import_file', { json: contents });

      // Ask user for import strategy
      const strategy = window.confirm(
        'Merge with existing folders (OK) or replace all (Cancel)?'
      )
        ? 'merge'
        : 'overwrite';

      const result = await invoke<{
        folders_added: number;
        folders_skipped: number;
        folders_replaced: number;
        datasets_imported: number;
        queries_imported: number;
        warnings: string[];
      }>('import_configuration', {
        json: contents,
        workspaceId: agentInfo.workspace_id,
        strategy,
      });

      // Reload config to reflect changes
      const newConfig = await invoke<AgentConfig>('get_config');
      setConfig(newConfig);

      const summary = [
        result.folders_added && `${result.folders_added} folders added`,
        result.folders_replaced && `${result.folders_replaced} folders replaced`,
        result.folders_skipped && `${result.folders_skipped} folders skipped`,
        result.datasets_imported && `${result.datasets_imported} datasets imported`,
      ]
        .filter(Boolean)
        .join(', ');

      toast.success(`Import complete: ${summary}`);

      if (result.warnings.length > 0) {
        result.warnings.forEach((w) => toast.info(w));
      }
    } catch (err) {
      toast.error(`Import failed: ${err}`);
    }
  };

  if (!draft) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-slate-500 dark:text-slate-400">
        Loading settings…
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <div className="mb-6 flex items-center justify-between">
        <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
          <SettingsIcon className="h-6 w-6 text-slate-700 dark:text-slate-300" />
          Settings
        </h1>
        {dirty && (
          <button
            onClick={save}
            disabled={saving}
            className="flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
          >
            {saving ? (
              <RefreshCw className="h-4 w-4 animate-spin" />
            ) : (
              <Save className="h-4 w-4" />
            )}
            {saving ? 'Saving…' : 'Save changes'}
          </button>
        )}
      </div>

      {/* Tab bar */}
      <div className="mb-6 flex gap-1 border-b border-slate-200 dark:border-slate-800">
        <TabButton active={tab === 'general'} onClick={() => setTab('general')}>
          General
        </TabButton>
        <TabButton active={tab === 'sync'} onClick={() => setTab('sync')}>
          Sync
        </TabButton>
        <TabButton active={tab === 'app'} onClick={() => setTab('app')}>
          App
        </TabButton>
        <TabButton active={tab === 'plugins'} onClick={() => setTab('plugins')}>
          Plugins
        </TabButton>
        <TabButton active={tab === 'about'} onClick={() => setTab('about')}>
          About
        </TabButton>
      </div>

      {/* Panels */}
      {tab === 'general' && (
        <GeneralPanel draft={draft} setDraft={setDraft} />
      )}
      {tab === 'sync' && <SyncPanel draft={draft} setDraft={setDraft} />}
      {tab === 'app' && <AppPanel draft={draft} setDraft={setDraft} />}
      {tab === 'plugins' && <PluginsPanel />}
      {tab === 'about' && (
        <AboutPanel
          draft={draft}
          agentId={agentInfo?.agent_id}
          workspaceId={agentInfo?.workspace_id}
          onLogout={logout}
          onExport={exportConfig}
          onImport={importConfig}
        />
      )}
    </div>
  );
}

// ─── Tab button ─────────────────────────────────────────────────────────────

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      onClick={onClick}
      className={`relative px-4 py-2 text-sm font-medium transition-colors ${
        active
          ? 'text-purple-700 dark:text-purple-300'
          : 'text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100'
      }`}
    >
      {children}
      {active && (
        <span className="absolute inset-x-0 -bottom-px h-0.5 bg-purple-600 dark:bg-purple-400" />
      )}
    </button>
  );
}

// ─── General ────────────────────────────────────────────────────────────────

function GeneralPanel({
  draft,
  setDraft,
}: {
  draft: AgentConfig;
  setDraft: (c: AgentConfig) => void;
}) {
  return (
    <Panel>
      <Field label="Agent name" hint="Shown in the cloud and on this device.">
        <input
          type="text"
          value={draft.agent.name}
          onChange={(e) =>
            setDraft({
              ...draft,
              agent: { ...draft.agent, name: e.target.value },
            })
          }
          className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
        />
      </Field>

      <Field label="Theme">
        <div className="inline-flex rounded-lg border border-slate-200 bg-white p-0.5 dark:border-slate-800 dark:bg-slate-900">
          <ThemeButton
            active={draft.app.theme === 'light'}
            onClick={() =>
              setDraft({ ...draft, app: { ...draft.app, theme: 'light' } })
            }
            icon={<Sun className="h-4 w-4" />}
            label="Light"
          />
          <ThemeButton
            active={draft.app.theme === 'dark'}
            onClick={() =>
              setDraft({ ...draft, app: { ...draft.app, theme: 'dark' } })
            }
            icon={<Moon className="h-4 w-4" />}
            label="Dark"
          />
          <ThemeButton
            active={draft.app.theme === 'system'}
            onClick={() =>
              setDraft({ ...draft, app: { ...draft.app, theme: 'system' } })
            }
            icon={<Monitor className="h-4 w-4" />}
            label="System"
          />
        </div>
      </Field>
    </Panel>
  );
}

function ThemeButton({
  active,
  onClick,
  icon,
  label,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
}) {
  return (
    <button
      onClick={onClick}
      className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
        active
          ? 'bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-300'
          : 'text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-100'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}

// ─── Sync ───────────────────────────────────────────────────────────────────

function SyncPanel({
  draft,
  setDraft,
}: {
  draft: AgentConfig;
  setDraft: (c: AgentConfig) => void;
}) {
  return (
    <Panel>
      <Toggle
        icon={<Zap className="h-5 w-5" />}
        label="Auto-sync on file changes"
        hint="Index schemas automatically when files are added, modified, or removed."
        checked={draft.sync.auto_sync_on_change}
        onChange={(v) =>
          setDraft({
            ...draft,
            sync: { ...draft.sync, auto_sync_on_change: v },
          })
        }
      />

      <Field
        label="Fallback scan interval"
        hint="How often to rescan watched folders as a safety net. Minimum 60s."
      >
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={60}
            step={60}
            value={draft.sync.fallback_scan_interval_seconds}
            onChange={(e) =>
              setDraft({
                ...draft,
                sync: {
                  ...draft.sync,
                  fallback_scan_interval_seconds: Math.max(
                    60,
                    parseInt(e.target.value) || 300,
                  ),
                },
              })
            }
            className="w-32 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          />
          <span className="text-sm text-slate-500 dark:text-slate-400">
            seconds
          </span>
        </div>
      </Field>

      <Field
        label="Sync cadence"
        hint="How often the agent sends heartbeats to the cloud."
      >
        <div className="flex items-center gap-2">
          <input
            type="number"
            min={10}
            value={draft.sync.interval_seconds}
            onChange={(e) =>
              setDraft({
                ...draft,
                sync: {
                  ...draft.sync,
                  interval_seconds: Math.max(10, parseInt(e.target.value) || 300),
                },
              })
            }
            className="w-32 rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          />
          <span className="text-sm text-slate-500 dark:text-slate-400">
            seconds
          </span>
        </div>
      </Field>
    </Panel>
  );
}

// ─── App ────────────────────────────────────────────────────────────────────

function AppPanel({
  draft,
  setDraft,
}: {
  draft: AgentConfig;
  setDraft: (c: AgentConfig) => void;
}) {
  return (
    <Panel>
      <Toggle
        icon={<Power className="h-5 w-5" />}
        label="Launch at login"
        hint="Start the agent automatically when you sign in."
        checked={draft.app.launch_at_login}
        onChange={(v) =>
          setDraft({ ...draft, app: { ...draft.app, launch_at_login: v } })
        }
      />

      <Toggle
        icon={<Bell className="h-5 w-5" />}
        label="Notifications"
        hint="Show system notifications when syncs complete or fail."
        checked={draft.app.notifications_enabled}
        onChange={(v) =>
          setDraft({
            ...draft,
            app: { ...draft.app, notifications_enabled: v },
          })
        }
      />
    </Panel>
  );
}

// ─── About ──────────────────────────────────────────────────────────────────

function AboutPanel({
  draft,
  agentId,
  workspaceId,
  onLogout,
  onExport,
  onImport,
}: {
  draft: AgentConfig;
  agentId: string | undefined;
  workspaceId: string | undefined;
  onLogout: () => void;
  onExport: () => void;
  onImport: () => void;
}) {
  return (
    <Panel>
      <div className="grid gap-3 rounded-lg border border-slate-200 bg-slate-50 p-4 text-sm dark:border-slate-800 dark:bg-slate-900">
        <Row label="Agent ID" value={agentId ?? '—'} mono />
        <Row label="Workspace" value={workspaceId ?? '—'} mono />
        <Row label="Platform" value={draft.agent.platform} />
        <Row label="Hostname" value={draft.agent.hostname} />
        <Row label="API endpoint" value={draft.cloud.api_url} mono />
      </div>

      <div className="flex items-start gap-3 rounded-lg border border-sky-200 bg-sky-50 p-4 text-xs text-sky-900 dark:border-sky-900 dark:bg-sky-950/30 dark:text-sky-200">
        <Info className="mt-0.5 h-4 w-4 shrink-0" />
        <span>
          Signing out removes the stored token from this device. Your data
          stays put, and you can sign back in at any time.
        </span>
      </div>

      <div className="flex gap-3">
        <button
          onClick={onExport}
          className="flex flex-1 items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
        >
          <Download className="h-4 w-4" />
          Export Configuration
        </button>
        <button
          onClick={onImport}
          className="flex flex-1 items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-4 py-2 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
        >
          <Upload className="h-4 w-4" />
          Import Configuration
        </button>
      </div>

      <button
        onClick={onLogout}
        className="flex items-center gap-2 rounded-lg border border-rose-300 bg-white px-4 py-2 text-sm font-medium text-rose-700 transition-colors hover:bg-rose-50 dark:border-rose-900 dark:bg-slate-900 dark:text-rose-300 dark:hover:bg-rose-950/40"
      >
        <LogOut className="h-4 w-4" />
        Sign out
      </button>
    </Panel>
  );
}

function Row({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <span className="text-slate-600 dark:text-slate-400">{label}</span>
      <span
        className={`truncate text-slate-900 dark:text-slate-100 ${mono ? 'font-mono text-xs' : ''}`}
        title={value}
      >
        {value}
      </span>
    </div>
  );
}

// ─── Layout pieces ──────────────────────────────────────────────────────────

function Panel({ children }: { children: React.ReactNode }) {
  return <div className="space-y-6">{children}</div>;
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <label className="mb-1 block text-sm font-medium text-slate-900 dark:text-slate-100">
        {label}
      </label>
      {hint && (
        <p className="mb-2 text-xs text-slate-500 dark:text-slate-400">{hint}</p>
      )}
      {children}
    </div>
  );
}

function Toggle({
  icon,
  label,
  hint,
  checked,
  onChange,
}: {
  icon: React.ReactNode;
  label: string;
  hint: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start gap-4 rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <div className="mt-0.5 text-purple-600 dark:text-purple-400">{icon}</div>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-slate-900 dark:text-slate-100">
          {label}
        </div>
        <div className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
          {hint}
        </div>
      </div>
      <button
        onClick={() => onChange(!checked)}
        role="switch"
        aria-checked={checked}
        className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors ${
          checked
            ? 'bg-purple-600'
            : 'bg-slate-300 dark:bg-slate-700'
        }`}
      >
        <span
          className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
            checked ? 'translate-x-6' : 'translate-x-1'
          }`}
        />
      </button>
    </div>
  );
}
