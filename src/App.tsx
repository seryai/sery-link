// Top-level app shell.
//
// Responsibilities:
//   1. Bootstrap: check for an existing token, load config, start tunnel + watcher.
//   2. Gate: route unauthenticated users to OnboardingWizard.
//   3. Layout: sidebar + main content with four tabs (Folders/History/Privacy/Settings).
//   4. Background: wire useAgentEvents, useTheme, and the ReAuthModal.
//   5. Providers: ToastProvider wraps everything so all components can show toasts.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  Clock,
  Folder,
  Loader2,
  Settings as SettingsIcon,
  Shield,
} from 'lucide-react';
import { useAgentStore, type AgentToken } from './stores/agentStore';
import seryLogo from './assets/sery-logo.svg';
import { useAgentEvents } from './hooks/useAgentEvents';
import { useTheme } from './hooks/useTheme';
import { ToastProvider } from './components/Toast';
import { OnboardingWizard } from './components/OnboardingWizard';
import { StatusBar } from './components/StatusBar';
import { FolderList } from './components/FolderList';
import { History } from './components/History';
import { Privacy } from './components/Privacy';
import { Settings } from './components/Settings';
import { ReAuthModal } from './components/ReAuthModal';
import { KeyboardShortcuts } from './components/KeyboardShortcuts';
import { CommandPalette } from './components/CommandPalette';
import type { AgentConfig, AgentStats } from './types/events';

type Tab = 'folders' | 'history' | 'privacy' | 'settings';

export default function App() {
  return (
    <ToastProvider>
      <AppInner />
    </ToastProvider>
  );
}

function AppInner() {
  const [activeTab, setActiveTab] = useState<Tab>('folders');
  const [bootstrapping, setBootstrapping] = useState(true);
  const {
    authenticated,
    agentInfo,
    config,
    setAuthenticated,
    setAgentInfo,
    setConfig,
    setStats,
  } = useAgentStore();

  // Keep the `html.dark` class and `html.theme` in sync with config
  useTheme();
  // Subscribe to every Tauri event we care about
  useAgentEvents();

  // Bootstrap: existing token? → load config + stats, start tunnel + watcher
  useEffect(() => {
    let cancelled = false;

    const bootstrap = async () => {
      try {
        const hasToken = await invoke<boolean>('has_token');
        if (hasToken) {
          const agentInfo = await invoke<AgentToken | null>('get_agent_info');
          if (agentInfo) {
            setAgentInfo(agentInfo);
            setAuthenticated(true);

            try {
              const config = await invoke<AgentConfig>('get_config');
              if (!cancelled) setConfig(config);
            } catch (err) {
              console.error('Failed to load config:', err);
            }

            try {
              const stats = await invoke<AgentStats>('get_stats');
              if (!cancelled) setStats(stats);
            } catch (err) {
              console.error('Failed to load stats:', err);
            }

            // Fire-and-forget — the event listeners will surface success/failure
            invoke('start_websocket_tunnel').catch((err) =>
              console.error('WebSocket tunnel failed to start:', err),
            );
            invoke('start_file_watcher').catch((err) =>
              console.error('File watcher failed to start:', err),
            );
          }
        } else {
          // No token — still try to load config so theme/onboarding state is set
          try {
            const config = await invoke<AgentConfig>('get_config');
            if (!cancelled) setConfig(config);
          } catch (err) {
            console.error('Failed to load config:', err);
          }
        }
      } catch (err) {
        console.error('Bootstrap failed:', err);
      } finally {
        if (!cancelled) setBootstrapping(false);
      }
    };

    bootstrap();
    return () => {
      cancelled = true;
    };
  }, [setAuthenticated, setAgentInfo, setConfig, setStats]);

  if (bootstrapping) {
    return (
      <div className="flex h-screen items-center justify-center bg-slate-50 dark:bg-slate-950">
        <div className="text-center">
          <Loader2 className="mx-auto mb-4 h-10 w-10 animate-spin text-purple-600 dark:text-purple-400" />
          <p className="text-sm text-slate-600 dark:text-slate-400">
            Starting up…
          </p>
        </div>
      </div>
    );
  }

  // Not authenticated → show onboarding
  // (Once authenticated, user can access the app regardless of onboarding completion)
  if (!authenticated) {
    return <OnboardingWizard />;
  }

  return (
    <div className="flex h-screen flex-col bg-slate-50 dark:bg-slate-950">
      <StatusBar />

      <div className="flex flex-1 overflow-hidden">
        {/* Sidebar */}
        <aside className="flex w-56 flex-col border-r border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
          <div className="flex items-center gap-2.5 border-b border-slate-200 px-4 py-4 dark:border-slate-800">
            <img src={seryLogo} alt="Sery" className="h-8 w-8" />
            <div>
              <h1 className="text-base font-bold text-slate-900 dark:text-slate-50">
                Sery Link
              </h1>
              <p className="text-[10px] text-slate-500 dark:text-slate-400">
                Local-first data analytics
              </p>
            </div>
          </div>

          <nav className="flex-1 space-y-0.5 p-2">
            <NavButton
              active={activeTab === 'folders'}
              onClick={() => setActiveTab('folders')}
              icon={<Folder className="h-4 w-4" />}
              label="Folders"
            />
            <NavButton
              active={activeTab === 'history'}
              onClick={() => setActiveTab('history')}
              icon={<Clock className="h-4 w-4" />}
              label="History"
            />
            <NavButton
              active={activeTab === 'privacy'}
              onClick={() => setActiveTab('privacy')}
              icon={<Shield className="h-4 w-4" />}
              label="Privacy"
            />
            <NavButton
              active={activeTab === 'settings'}
              onClick={() => setActiveTab('settings')}
              icon={<SettingsIcon className="h-4 w-4" />}
              label="Settings"
            />
          </nav>
        </aside>

        {/* Main content */}
        <main className="flex-1 overflow-auto">
          {activeTab === 'folders' && <FolderList />}
          {activeTab === 'history' && <History />}
          {activeTab === 'privacy' && <Privacy />}
          {activeTab === 'settings' && <Settings />}
        </main>
      </div>

      {/* Global overlays */}
      <ReAuthModal />
      <KeyboardShortcuts />
      <CommandPalette
        config={config}
        workspaceId={agentInfo?.workspace_id ?? null}
        onNavigate={(tab) => setActiveTab(tab)}
        onAddFolder={() => {
          // Switch to folders tab and trigger add folder action
          // The FolderList component will handle the actual folder picker
          setActiveTab('folders');
          // TODO: Emit event to trigger folder picker in FolderList
        }}
      />
    </div>
  );
}

function NavButton({
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
      className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
        active
          ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
          : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
      }`}
    >
      {icon}
      {label}
    </button>
  );
}
