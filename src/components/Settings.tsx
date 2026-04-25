// Settings panel — tabbed layout with General, Sync, App, and About sections.
// All changes are staged in local state and only persisted when the user
// clicks Save. Certain toggles (auto-sync, launch-at-login) also trigger a
// backend side-effect on save.

import { useEffect, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { getVersion } from '@tauri-apps/api/app';
import { check, type Update } from '@tauri-apps/plugin-updater';
import {
  Bell,
  CheckCircle2,
  Download,
  Info,
  LogOut,
  Monitor,
  Moon,
  Power,
  RefreshCw,
  Save,
  Settings as SettingsIcon,
  Sparkles,
  Sun,
  Upload,
  Wifi,
  WifiOff,
  Zap,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import { useToast } from './Toast';
import { PluginsPanel } from './PluginsPanel';
import { MarketplacePanel } from './MarketplacePanel';
import type { AgentConfig } from '../types/events';

type Tab = 'general' | 'sync' | 'app' | 'plugins' | 'marketplace' | 'about';

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
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <SettingsIcon className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Settings
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              Manage your Sery Link preferences, sync behavior, and plugins.
            </p>
          </div>
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
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto p-6">
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
          <TabButton active={tab === 'marketplace'} onClick={() => setTab('marketplace')}>
            Marketplace
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
        {tab === 'marketplace' && <MarketplacePanel />}
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
      <Field label="Machine name" hint="Shown in the Machines view and on this device.">
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
      <NetworkModeToggle />

      <AIProviderPanel />

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
        label="Connection frequency"
        hint="How often Sery Link checks in with Sery.ai. Lower values feel more live; higher values reduce network traffic."
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

// ─── Network mode (Local-Only toggle, ROADMAP F6) ───────────────────────────
//
// One toggle that pins the app to LocalOnly auth mode. When on, the
// WebSocket tunnel disconnects and every cloud-dependent feature gate
// returns false. Local features (column search, profiles, recipes, the
// file watcher) keep working untouched.
//
// The keyring token is left intact, so toggling back is one click rather
// than a re-pair. State is read fresh on mount and on every save round-
// trip — we don't trust component-local state to mirror what's pinned in
// config.

function NetworkModeToggle() {
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [busy, setBusy] = useState(false);
  const toast = useToast();
  const { setAuthenticated } = useAgentStore();

  useEffect(() => {
    let cancelled = false;
    invoke<boolean>('is_local_only_mode_enabled')
      .then((value) => {
        if (!cancelled) setEnabled(value);
      })
      .catch((err) => {
        console.error('Failed to read local-only mode:', err);
        if (!cancelled) setEnabled(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const onToggle = async (next: boolean) => {
    if (busy) return;
    setBusy(true);
    try {
      await invoke('set_local_only_mode', { enabled: next });
      setEnabled(next);
      // The WebSocket either dropped (next=true) or came back online
      // (next=false). Keep the global auth flag in sync so the StatusBar
      // and other surfaces reflect reality immediately.
      setAuthenticated(!next);
      toast.success(
        next
          ? 'Disconnected from network. Local features still work.'
          : 'Reconnected to network.',
      );
    } catch (err) {
      toast.error(`Couldn't change network mode: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  // Loading shimmer — show the row immediately so layout doesn't jump,
  // but with the toggle disabled until we know the real state.
  if (enabled === null) {
    return (
      <div className="flex items-start gap-4 rounded-lg border border-slate-200 bg-white p-4 opacity-60 dark:border-slate-800 dark:bg-slate-900">
        <div className="mt-0.5 text-purple-600 dark:text-purple-400">
          <Wifi className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="text-sm font-medium text-slate-900 dark:text-slate-100">
            Network mode
          </div>
          <div className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
            Loading…
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex items-start gap-4 rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <div className="mt-0.5 text-purple-600 dark:text-purple-400">
        {enabled ? <WifiOff className="h-5 w-5" /> : <Wifi className="h-5 w-5" />}
      </div>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-slate-900 dark:text-slate-100">
          Local-only mode
        </div>
        <div className="mt-0.5 text-xs text-slate-500 dark:text-slate-400">
          {enabled
            ? 'Disconnected from the Sery network. Cloud AI, cross-machine search, and sharing are paused. Local search, schemas, profiles, and recipes still work.'
            : 'Connected to the Sery network. Turn this on to disconnect — your files and local features keep working; cloud features pause until you turn it back off.'}
        </div>
      </div>
      <button
        onClick={() => onToggle(!enabled)}
        role="switch"
        aria-checked={enabled}
        disabled={busy}
        className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors disabled:opacity-50 ${
          enabled
            ? 'bg-purple-600'
            : 'bg-slate-300 dark:bg-slate-700'
        }`}
      >
        <span
          className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
            enabled ? 'translate-x-6' : 'translate-x-1'
          }`}
        />
      </button>
    </div>
  );
}

// ─── AI Provider (BYOK, ROADMAP F7) ─────────────────────────────────────────
//
// Settings surface for the BYOK key. Lives under Sync alongside the
// Local-Only toggle because both control the user's relationship to the
// network — Local-Only disconnects from Sery, BYOK reroutes the LLM call
// around Sery. Per SPEC_BYOK.md v0.5.0 scope: Anthropic only, single
// keyring entry, validate-then-save flow, no key surfaced after save
// (the keyring is the only persistent copy).

interface ByokStatus {
  configured: boolean;
  provider: string | null;
}

function AIProviderPanel() {
  const [status, setStatus] = useState<ByokStatus | null>(null);
  const [provider, setProvider] = useState<'anthropic'>('anthropic');
  const [draft, setDraft] = useState('');
  const [busy, setBusy] = useState(false);
  const [validating, setValidating] = useState(false);
  const [errMsg, setErrMsg] = useState<string | null>(null);
  const toast = useToast();

  const refresh = async () => {
    try {
      const next = await invoke<ByokStatus>('get_byok_status');
      setStatus(next);
    } catch (err) {
      console.error('Failed to read BYOK status:', err);
      setStatus({ configured: false, provider: null });
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const validateAndSave = async () => {
    const key = draft.trim();
    if (!key) {
      setErrMsg('Paste your API key first.');
      return;
    }
    setBusy(true);
    setValidating(true);
    setErrMsg(null);
    try {
      // Validate the key by hitting the provider — if this fails, never
      // save it. Avoids landing the user in a "key saved but broken"
      // limbo where the Ask UI breaks silently.
      await invoke('validate_byok_key', { provider, apiKey: key });
      await invoke('save_byok_key', { provider, apiKey: key });
      setDraft('');
      await refresh();
      toast.success('AI provider connected');
    } catch (err) {
      const msg = typeof err === 'string' ? err : String(err);
      setErrMsg(msg);
    } finally {
      setBusy(false);
      setValidating(false);
    }
  };

  const clear = async () => {
    if (!window.confirm('Remove the saved AI provider key?')) return;
    setBusy(true);
    setErrMsg(null);
    try {
      await invoke('clear_byok_key', { provider });
      await refresh();
      toast.info('AI provider key removed');
    } catch (err) {
      toast.error(`Couldn't remove key: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  if (status === null) {
    return (
      <div className="flex items-start gap-4 rounded-lg border border-slate-200 bg-white p-4 opacity-60 dark:border-slate-800 dark:bg-slate-900">
        <div className="mt-0.5 text-purple-600 dark:text-purple-400">
          <Sparkles className="h-5 w-5" />
        </div>
        <div className="text-sm text-slate-500 dark:text-slate-400">
          Checking AI provider…
        </div>
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <div className="flex items-start gap-4">
        <div className="mt-0.5 text-purple-600 dark:text-purple-400">
          <Sparkles className="h-5 w-5" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium text-slate-900 dark:text-slate-100">
              AI provider (Bring Your Own Key)
            </span>
            {status.configured ? (
              <span className="inline-flex items-center gap-1 rounded-full bg-emerald-100 px-2 py-0.5 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300">
                <CheckCircle2 className="h-3 w-3" />
                Connected
              </span>
            ) : (
              <span className="inline-flex items-center rounded-full bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-600 dark:bg-slate-800 dark:text-slate-300">
                Not configured
              </span>
            )}
          </div>
          <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
            Use your own Anthropic API key for the Ask page. Your prompts go
            directly from this app to Anthropic — never through sery.ai. Key
            is stored in your OS keychain; we cannot read it.
          </p>
        </div>
      </div>

      <div className="mt-4 space-y-3">
        <div>
          <label className="mb-1 block text-xs font-medium text-slate-600 dark:text-slate-400">
            Provider
          </label>
          <select
            value={provider}
            onChange={(e) => setProvider(e.target.value as 'anthropic')}
            disabled={busy}
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          >
            <option value="anthropic">Anthropic (Claude)</option>
          </select>
          <p className="mt-1 text-[11px] text-slate-400 dark:text-slate-500">
            OpenAI support arriving in v0.5.x.
          </p>
        </div>

        <div>
          <label className="mb-1 block text-xs font-medium text-slate-600 dark:text-slate-400">
            API key
          </label>
          <input
            type="password"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder={
              status.configured
                ? 'A key is already saved — paste a new one to replace it'
                : 'sk-ant-…'
            }
            disabled={busy}
            autoComplete="off"
            spellCheck={false}
            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 font-mono text-sm text-slate-900 placeholder:text-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100"
          />
          <p className="mt-1 text-[11px] text-slate-400 dark:text-slate-500">
            Get a key at{' '}
            <a
              href="https://console.anthropic.com/settings/keys"
              target="_blank"
              rel="noreferrer"
              className="text-purple-600 hover:underline dark:text-purple-300"
            >
              console.anthropic.com/settings/keys
            </a>
            .
          </p>
        </div>

        {errMsg && (
          <div className="rounded-lg border border-red-200 bg-red-50 p-2 text-xs text-red-700 dark:border-red-900/50 dark:bg-red-950/40 dark:text-red-300">
            {errMsg}
          </div>
        )}

        <div className="flex items-center gap-2">
          <button
            onClick={() => void validateAndSave()}
            disabled={busy || draft.trim().length === 0}
            className="inline-flex items-center gap-1.5 rounded-lg bg-purple-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <CheckCircle2 className="h-3.5 w-3.5" />
            {validating ? 'Validating…' : 'Test & save'}
          </button>
          {status.configured && (
            <button
              onClick={() => void clear()}
              disabled={busy}
              className="inline-flex items-center gap-1.5 rounded-lg border border-slate-300 px-3 py-1.5 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-100 disabled:opacity-50 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
            >
              Remove key
            </button>
          )}
        </div>
      </div>
    </div>
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

      <Toggle
        icon={<Bell className="h-5 w-5" />}
        label="Schema-change toasts"
        hint="Pop a toast when a scan detects a new, removed, or changed column. The Notifications tab and Machines badge still update either way."
        checked={draft.app.schema_change_toasts_enabled}
        onChange={(v) =>
          setDraft({
            ...draft,
            app: { ...draft.app, schema_change_toasts_enabled: v },
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
        <VersionRow />
        <Row label="Agent ID" value={agentId ?? '—'} mono />
        <Row label="Workspace" value={workspaceId ?? '—'} mono />
        <Row label="Platform" value={draft.agent.platform} />
        <Row label="Hostname" value={draft.agent.hostname} />
        <Row label="API endpoint" value={draft.cloud.api_url} mono />
      </div>

      <UpdaterSection />

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

function VersionRow() {
  const [version, setVersion] = useState<string>('—');
  useEffect(() => {
    getVersion().then(setVersion).catch(() => setVersion('—'));
  }, []);
  return <Row label="Version" value={version} mono />;
}

// ─── Updater ─────────────────────────────────────────────────────────────

type UpdaterState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate'; at: Date }
  | { kind: 'available'; update: Update }
  | { kind: 'downloading'; progress: number | null }
  | { kind: 'installed' }
  | { kind: 'error'; message: string };

function UpdaterSection() {
  const [state, setState] = useState<UpdaterState>({ kind: 'idle' });

  const checkForUpdate = async () => {
    setState({ kind: 'checking' });
    try {
      const update = await check();
      if (update) {
        setState({ kind: 'available', update });
      } else {
        setState({ kind: 'upToDate', at: new Date() });
      }
    } catch (err) {
      setState({ kind: 'error', message: String(err) });
    }
  };

  const installUpdate = async () => {
    if (state.kind !== 'available') return;
    const { update } = state;
    setState({ kind: 'downloading', progress: null });
    try {
      let downloaded = 0;
      let contentLength: number | null = null;
      await update.downloadAndInstall((ev) => {
        // Tauri v2 updater events — Started | Progress | Finished
        if (ev.event === 'Started') {
          contentLength = ev.data.contentLength ?? null;
        } else if (ev.event === 'Progress') {
          downloaded += ev.data.chunkLength;
          const progress =
            contentLength && contentLength > 0
              ? Math.min(100, Math.round((downloaded / contentLength) * 100))
              : null;
          setState({ kind: 'downloading', progress });
        }
      });
      setState({ kind: 'installed' });
    } catch (err) {
      setState({ kind: 'error', message: String(err) });
    }
  };

  return (
    <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <h3 className="mb-3 flex items-center gap-2 text-sm font-semibold text-slate-900 dark:text-slate-50">
        <RefreshCw className="h-4 w-4" />
        Updates
      </h3>

      {state.kind === 'idle' && (
        <div className="flex items-center justify-between gap-3">
          <p className="text-xs text-slate-600 dark:text-slate-400">
            Sery Link checks for updates automatically. You can also check
            now.
          </p>
          <button
            onClick={checkForUpdate}
            className="shrink-0 rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
          >
            Check for updates
          </button>
        </div>
      )}

      {state.kind === 'checking' && (
        <p className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-400">
          <RefreshCw className="h-3.5 w-3.5 animate-spin" />
          Checking…
        </p>
      )}

      {state.kind === 'upToDate' && (
        <div className="flex items-center justify-between gap-3">
          <p className="flex items-center gap-2 text-xs text-emerald-700 dark:text-emerald-300">
            <CheckCircle2 className="h-3.5 w-3.5" />
            You're on the latest version.
          </p>
          <button
            onClick={checkForUpdate}
            className="shrink-0 text-xs text-slate-500 underline-offset-2 hover:text-slate-700 hover:underline dark:text-slate-400 dark:hover:text-slate-200"
          >
            Check again
          </button>
        </div>
      )}

      {state.kind === 'available' && (
        <div className="space-y-3">
          <div className="flex items-center gap-2 text-xs font-medium text-purple-700 dark:text-purple-300">
            <Sparkles className="h-3.5 w-3.5" />
            Version {state.update.version} is available
          </div>
          {state.update.body && (
            <pre className="max-h-48 overflow-y-auto whitespace-pre-wrap rounded border border-slate-200 bg-slate-50 p-3 text-xs text-slate-700 dark:border-slate-800 dark:bg-slate-950 dark:text-slate-300">
              {state.update.body}
            </pre>
          )}
          <button
            onClick={installUpdate}
            className="inline-flex items-center gap-2 rounded-lg bg-purple-600 px-3 py-1.5 text-xs font-semibold text-white transition-colors hover:bg-purple-700"
          >
            <Download className="h-3.5 w-3.5" />
            Download &amp; install
          </button>
        </div>
      )}

      {state.kind === 'downloading' && (
        <div className="space-y-2">
          <p className="flex items-center gap-2 text-xs text-slate-600 dark:text-slate-400">
            <Download className="h-3.5 w-3.5 animate-pulse" />
            Downloading update
            {state.progress !== null ? ` · ${state.progress}%` : '…'}
          </p>
          {state.progress !== null && (
            <div className="h-1.5 w-full overflow-hidden rounded-full bg-slate-200 dark:bg-slate-800">
              <div
                className="h-full bg-purple-600 transition-[width]"
                style={{ width: `${state.progress}%` }}
              />
            </div>
          )}
        </div>
      )}

      {state.kind === 'installed' && (
        <p className="flex items-center gap-2 text-xs text-emerald-700 dark:text-emerald-300">
          <CheckCircle2 className="h-3.5 w-3.5" />
          Update installed. Quit and reopen Sery Link to finish applying it.
        </p>
      )}

      {state.kind === 'error' && (
        <div className="space-y-2">
          <p className="text-xs text-rose-700 dark:text-rose-300">
            Couldn't check for updates: {state.message}
          </p>
          <button
            onClick={checkForUpdate}
            className="rounded-lg border border-slate-300 bg-white px-3 py-1.5 text-xs font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
          >
            Retry
          </button>
        </div>
      )}
    </div>
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
