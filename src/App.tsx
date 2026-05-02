// Top-level app shell.
//
// Responsibilities:
//   1. Bootstrap: check for an existing token, load config, start tunnel + watcher.
//   2. Gate: route unauthenticated users to OnboardingWizard.
//   3. Layout: sidebar + main content with four tabs (Folders/History/Privacy/Settings).
//   4. Background: wire useAgentEvents, useTheme, and the ReAuthModal.
//   5. Providers: ToastProvider wraps everything so all components can show toasts.

import { useEffect, useState } from 'react';
import { HashRouter, Routes, Route, Navigate, NavLink, useNavigate, useLocation } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  BarChart3,
  Bell,
  ChevronDown,
  Folder,
  Laptop,
  Loader2,
  Search,
  Settings as SettingsIcon,
  Shield,
  Sparkles,
} from 'lucide-react';
import { useAgentStore, type AgentToken } from './stores/agentStore';
import seryLogo from './assets/sery-logo.svg';
import { useAgentEvents } from './hooks/useAgentEvents';
import { useTheme } from './hooks/useTheme';
import { ToastProvider, useToast } from './components/Toast';
import { OnboardingWizard } from './components/OnboardingWizard';
import { StatusBar } from './components/StatusBar';
import { FolderList } from './components/FolderList';
import { FolderDetail } from './components/FolderDetail';
import { FileDetail } from './components/FileDetail';
import { SearchPage } from './components/SearchPage';
import { History } from './components/History';
import { Privacy } from './components/Privacy';
import { Settings } from './components/Settings';
import { MachinesView } from './components/MachinesView';
import { Notifications } from './components/Notifications';
import { Recipes } from './components/Recipes';
import { Ask } from './components/Ask';
import { ReAuthModal } from './components/ReAuthModal';
import { KeyboardShortcuts } from './components/KeyboardShortcuts';
import { CommandPalette } from './components/CommandPalette';
import type { AgentConfig, AgentStats, StoredSchemaNotification } from './types/events';

export default function App() {
  return (
    <HashRouter>
      <ToastProvider>
        <AppInner />
      </ToastProvider>
    </HashRouter>
  );
}

function AppInner() {
  const navigate = useNavigate();
  const location = useLocation();
  const [showMoreDropdown, setShowMoreDropdown] = useState(false);
  const [bootstrapping, setBootstrapping] = useState(true);

  const {
    agentInfo,
    config,
    setAuthenticated,
    setAgentInfo,
    setConfig,
    setStats,
    setSchemaNotifications,
  } = useAgentStore();

  // Check if current route is in More dropdown
  const isMoreActive = location.pathname.startsWith('/settings') || location.pathname.startsWith('/privacy');

  // Keep the `html.dark` class and `html.theme` in sync with config
  useTheme();
  // Subscribe to every Tauri event we care about
  useAgentEvents();
  // Surface Drive watch progress globally — the modal closes
  // immediately after auto-watch starts, so any later progress /
  // completion / error has to be visible from anywhere in the app.
  useGdriveWatchToasts();

  // ROADMAP F9: the global Quick-Ask hotkey (registered in
  // src-tauri/src/hotkey.rs) emits a `quick-ask` event whenever it
  // fires. Routing depends on whether BYOK is configured:
  //   - BYOK key present → /ask (the natural "ask a question" surface)
  //   - No BYOK key      → /search (still useful, and the user discovers
  //                        Ask via the sidebar)
  // Either way, the input is focused so the user can type immediately.
  // Window show/focus is handled on the Rust side.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen('quick-ask', async () => {
      // Best-effort BYOK probe — if it fails we fall through to /search
      // rather than blocking the hotkey.
      let target = '/search';
      try {
        const status = await invoke<{ configured: boolean }>('get_byok_status');
        if (status?.configured) {
          target = '/ask';
        }
      } catch (err) {
        console.warn('Quick-Ask: could not read BYOK status, defaulting to /search:', err);
      }
      navigate(target);
      // Defer to the next tick so the route has mounted before we look
      // for the input. Input components have `autoFocus`, but if the
      // user was already on the target route it doesn't remount, so we
      // need to focus explicitly. Both /search and /ask have a single
      // dominant input — find the first textarea or text input and
      // focus it.
      window.setTimeout(() => {
        const input =
          document.querySelector<HTMLTextAreaElement>('textarea') ||
          document.querySelector<HTMLInputElement>('input[type="text"]');
        if (input) {
          input.focus();
          if ('select' in input) input.select();
        }
      }, 50);
    })
      .then((fn) => {
        unlisten = fn;
      })
      .catch((err) => {
        console.error('Failed to listen for quick-ask event:', err);
      });
    return () => {
      unlisten?.();
    };
  }, [navigate]);

  // Close More dropdown when clicking outside
  useEffect(() => {
    if (!showMoreDropdown) return;

    const handleClickOutside = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      // Don't close if clicking inside the dropdown or on NavLinks
      if (target.closest('.dropdown-container') || target.closest('a[href]')) {
        return;
      }
      setShowMoreDropdown(false);
    };

    // Use setTimeout to allow NavLink clicks to register first
    const timeoutId = setTimeout(() => {
      document.addEventListener('click', handleClickOutside);
    }, 0);

    return () => {
      clearTimeout(timeoutId);
      document.removeEventListener('click', handleClickOutside);
    };
  }, [showMoreDropdown]);

  // Bootstrap on launch.
  //
  // Local-first: we always load config + stats + start the file
  // watcher so local scanning works even when the user has never
  // connected to the cloud. Only cloud-specific pieces (tunnel,
  // cached agent info, schema-notification hydration) are gated on
  // `has_token` being true.
  useEffect(() => {
    let cancelled = false;

    const bootstrap = async () => {
      try {
        // 1. Always-on local state: config (theme, first_run flag,
        //    watched folders), stats (queries-today, etc.).
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

        // 2. Local file watcher — runs in both local-only AND
        //    connected mode. The watcher only dispatches cloud sync
        //    inside sync_folder when a token is present (see
        //    src-tauri/src/watcher.rs).
        invoke('start_file_watcher').catch((err) =>
          console.error('File watcher failed to start:', err),
        );

        // 3. Cloud-only pieces — gated on an existing token. New
        //    installs skip this entire block until the user clicks
        //    Connect from the StatusBar.
        const hasToken = await invoke<boolean>('has_token').catch(() => false);
        if (hasToken) {
          const agentInfo = await invoke<AgentToken | null>('get_agent_info');
          if (agentInfo && !cancelled) {
            setAgentInfo(agentInfo);
            setAuthenticated(true);

            try {
              const stored = await invoke<StoredSchemaNotification[]>(
                'get_schema_notifications',
                { limit: 200 },
              );
              if (!cancelled) setSchemaNotifications(stored);
            } catch (err) {
              console.error('Failed to load schema notifications:', err);
            }

            invoke('start_websocket_tunnel').catch((err) =>
              console.error('WebSocket tunnel failed to start:', err),
            );
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
  }, [setAuthenticated, setAgentInfo, setConfig, setStats, setSchemaNotifications]);

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

  // Show onboarding for first-time users (regardless of auth)
  // Users can choose Local Vault (no auth) or Workspace mode
  if (!config?.app?.first_run_completed) {
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
                Understand your data
              </p>
            </div>
          </div>

          <nav className="flex flex-1 flex-col space-y-0.5 p-2">
            <NavLink
              to="/search"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <Search className="h-4 w-4" />
              Find
            </NavLink>
            <NavLink
              to="/ask"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <Sparkles className="h-4 w-4" />
              Ask
            </NavLink>
            <NavLink
              to="/folders"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <Folder className="h-4 w-4" />
              Folders
            </NavLink>
            <NavLink
              to="/results"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <BarChart3 className="h-4 w-4" />
              Results
            </NavLink>
            <NavLink
              to="/machines"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <Laptop className="h-4 w-4" />
              Machines
            </NavLink>
            <NavLink
              to="/recipes"
              className={({ isActive }) =>
                `flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`
              }
            >
              <Sparkles className="h-4 w-4" />
              Recipes
            </NavLink>
            <NotificationsNavLink />

            {/* Spacer to push More to bottom */}
            <div className="flex-1" />

            {/* More dropdown */}
            <div className="dropdown-container relative">
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  setShowMoreDropdown(!showMoreDropdown);
                }}
                className={`flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isMoreActive
                    ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                    : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                }`}
              >
                <div className="flex items-center gap-3">
                  <SettingsIcon className="h-4 w-4" />
                  <span>More</span>
                </div>
                <ChevronDown
                  className={`h-4 w-4 transition-transform ${showMoreDropdown ? 'rotate-180' : ''}`}
                />
              </button>

              {/* Dropdown menu */}
              {showMoreDropdown && (
                <div className="dropdown-container absolute bottom-full left-0 mb-1 w-full overflow-hidden rounded-lg border border-slate-200 bg-white shadow-lg dark:border-slate-700 dark:bg-slate-800">
                  <NavLink
                    to="/settings"
                    onClick={(e) => {
                      e.stopPropagation();
                      setShowMoreDropdown(false);
                    }}
                    className={({ isActive }) =>
                      `flex w-full items-center gap-3 px-3 py-2 text-sm transition-colors ${
                        isActive
                          ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                          : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                      }`
                    }
                  >
                    <SettingsIcon className="h-4 w-4" />
                    <span>Settings</span>
                  </NavLink>
                  <NavLink
                    to="/privacy"
                    onClick={(e) => {
                      e.stopPropagation();
                      setShowMoreDropdown(false);
                    }}
                    className={({ isActive }) =>
                      `flex w-full items-center gap-3 px-3 py-2 text-sm transition-colors ${
                        isActive
                          ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                          : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
                      }`
                    }
                  >
                    <Shield className="h-4 w-4" />
                    <span>Privacy</span>
                  </NavLink>
                </div>
              )}
            </div>
          </nav>
        </aside>

        {/* Main content */}
        <main className="flex-1 overflow-auto">
          <Routes>
            <Route path="/" element={<Navigate to="/search" replace />} />
            <Route path="/search" element={<SearchPage />} />
            <Route path="/folders" element={<FolderList />} />
            <Route path="/folders/:folderId" element={<FolderDetail />} />
            <Route
              path="/folders/:folderId/files/:filePath"
              element={<FileDetail />}
            />
            <Route path="/results" element={<History />} />
            <Route path="/machines" element={<MachinesView />} />
            <Route path="/recipes" element={<Recipes />} />
            <Route path="/ask" element={<Ask />} />
            <Route path="/notifications" element={<Notifications />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/privacy" element={<Privacy />} />
          </Routes>
        </main>
      </div>

      {/* Global overlays */}
      <ReAuthModal />
      <KeyboardShortcuts />
      <CommandPalette
        config={config}
        workspaceId={agentInfo?.workspace_id ?? null}
        onNavigate={(tab) => {
          navigate(`/${tab}`);
          setShowMoreDropdown(false);
        }}
        onAddFolder={() => {
          navigate('/folders');
          setShowMoreDropdown(false);
        }}
      />
    </div>
  );
}

// Sidebar link with an unread-count badge. Split out so it can read
// from the store independently and avoid re-rendering the whole nav
// on every notification arrival.
function NotificationsNavLink() {
  const unread = useAgentStore((s) =>
    s.schemaNotifications.reduce((n, x) => n + (x.read ? 0 : 1), 0),
  );
  return (
    <NavLink
      to="/notifications"
      className={({ isActive }) =>
        `flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
          isActive
            ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
            : 'text-slate-700 hover:bg-slate-100 dark:text-slate-300 dark:hover:bg-slate-800'
        }`
      }
    >
      <div className="flex items-center gap-3">
        <Bell className="h-4 w-4" />
        <span>Notifications</span>
      </div>
      {unread > 0 && (
        <span className="rounded-full bg-purple-600 px-1.5 py-0.5 text-[10px] font-semibold leading-none text-white">
          {unread > 99 ? '99+' : unread}
        </span>
      )}
    </NavLink>
  );
}

/** Surface Drive watch progress as toasts at "milestone" events
 *  (downloading start, scanning, done, error). Per-file progress
 *  goes to the console — toasting on every file would drown out
 *  every other notification in the app for users with big Drives. */
function useGdriveWatchToasts() {
  const toast = useToast();
  useEffect(() => {
    let lastFolderId: string | null = null;
    let downloadStartShown = false;

    const unlisten = listen<
      | { folder_id: string; phase: 'walking' }
      | {
          folder_id: string;
          phase: 'downloading';
          current: number;
          total: number;
          file_name: string;
        }
      | { folder_id: string; phase: 'scanning' }
      | {
          folder_id: string;
          phase: 'done';
          total_files: number;
          skipped_native: number;
          skipped_too_large?: number;
        }
    >('gdrive-watch-progress', (event) => {
      const p = event.payload;
      // Reset per-folder state when the folder_id changes mid-stream.
      if (p.folder_id !== lastFolderId) {
        lastFolderId = p.folder_id;
        downloadStartShown = false;
      }
      switch (p.phase) {
        case 'downloading':
          if (!downloadStartShown && p.total > 0) {
            toast.info(`Downloading ${p.total} file${p.total === 1 ? '' : 's'} from Google Drive…`);
            downloadStartShown = true;
          }
          break;
        case 'done': {
          const tooBig = p.skipped_too_large ?? 0;
          const reasons: string[] = [];
          if (p.skipped_native > 0) reasons.push(`${p.skipped_native} Docs/Forms`);
          if (tooBig > 0) reasons.push(`${tooBig} over 1 GiB`);
          const skippedLabel = reasons.length ? ` (skipped: ${reasons.join(', ')})` : '';
          toast.success(
            `Google Drive indexed — ${p.total_files} file${p.total_files === 1 ? '' : 's'}${skippedLabel}`,
          );
          break;
        }
        // walking / scanning are short — no need to chatter.
      }
    });
    return () => {
      unlisten.then((u) => u());
    };
  }, [toast]);
}

