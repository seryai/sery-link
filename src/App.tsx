// Top-level app shell.
//
// Responsibilities:
//   1. Bootstrap: check for an existing token, load config, start tunnel + watcher.
//   2. Gate: route unauthenticated users to OnboardingWizard.
//   3. Layout: sidebar + main content with four tabs (Folders/History/Privacy/Settings).
//   4. Background: wire useAgentEvents, useTheme, and the ReAuthModal.
//   5. Providers: ToastProvider wraps everything so all components can show toasts.

import { useCallback, useEffect, useRef, useState } from 'react';
import { HashRouter, Routes, Route, Navigate, useNavigate } from 'react-router-dom';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { Loader2 } from 'lucide-react';
import { useAgentStore, type AgentToken } from './stores/agentStore';
import { useAgentEvents } from './hooks/useAgentEvents';
import { useTheme } from './hooks/useTheme';
import { ToastProvider, useToast } from './components/Toast';
import { OnboardingWizard } from './components/OnboardingWizard';
import { StatusBar } from './components/StatusBar';
import { DatabaseDetail } from './components/DatabaseDetail';
import { FolderDetail } from './components/FolderDetail';
import { SourcesSidebar } from './components/SourcesSidebar';
import { FileDetail } from './components/FileDetail';
import { SearchPage } from './components/SearchPage';
import { History } from './components/History';
import { Privacy } from './components/Privacy';
import { Settings } from './components/Settings';
import { Notifications } from './components/Notifications';
import { Recipes } from './components/Recipes';
import { ReAuthModal } from './components/ReAuthModal';
import { WorkspaceKeyRevokedModal } from './components/WorkspaceKeyRevokedModal';
import { KeyboardShortcuts } from './components/KeyboardShortcuts';
import { TitleBar } from './components/TitleBar';
import { CommandPalette } from './components/CommandPalette';
import { Dashboard } from './components/Dashboard';
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
  // fires. Post-pivot the desktop no longer hosts AI, so the hotkey
  // routes to /search — the local data discovery surface. AI lives
  // in the cloud dashboard at <web_url>/chat now.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen('quick-ask', async () => {
      navigate('/search');
      // Defer to the next tick so the route has mounted before we look
      // for the input. Input components have `autoFocus`, but if the
      // user was already on the target route it doesn't remount, so we
      // need to focus explicitly. /search has a single dominant input
      // — find the first textarea or text input and focus it.
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
      <div className="flex h-screen items-center justify-center bg-transparent">
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
    <div className="flex h-screen flex-col bg-transparent">
      <TitleBar />
      {/* StatusBar in headless mode — runs all event listeners and
          renders ConnectModal / CatchUpDialog via fixed-position
          portals, but shows no visible bar (that's now in TitleBar). */}
      <StatusBar headless />

      <div className="flex flex-1 overflow-hidden">
        {/* Left: resizable sources sidebar */}
        <ResizableSidebar>
          <SourcesSidebar />
        </ResizableSidebar>

        {/* Right: main content */}
        <main className="flex-1 overflow-auto bg-white/85 dark:bg-[#1c1c1e]/90">
          <Routes>
            <Route path="/" element={<Dashboard />} />
            <Route path="/search" element={<SearchPage />} />
            <Route path="/history" element={<History />} />
            <Route path="/sources" element={<Navigate to="/" replace />} />
            <Route path="/results" element={<Navigate to="/history" replace />} />
            <Route path="/recipes" element={<Recipes />} />
            <Route path="/settings" element={<Settings />} />
            <Route path="/notifications" element={<Notifications />} />
            <Route path="/privacy" element={<Privacy />} />
            <Route path="/db/:sourceId" element={<DatabaseDetail />} />
            <Route path="/folders/:folderId" element={<FolderDetail />} />
            <Route
              path="/folders/:folderId/files/:filePath"
              element={<FileDetail />}
            />
            <Route path="/folders" element={<Navigate to="/" replace />} />
          </Routes>
        </main>
      </div>

      {/* Global overlays */}
      <ReAuthModal />
      <WorkspaceKeyRevokedModal />
      <KeyboardShortcuts />
      <CommandPalette
        config={config}
        workspaceId={agentInfo?.workspace_id ?? null}
        onNavigate={(tab) => {
          navigate(`/${tab}`);
        }}
        onAddFolder={() => {
          navigate('/');
        }}
      />
    </div>
  );
}

const SIDEBAR_MIN = 180;
const SIDEBAR_MAX = 480;
const SIDEBAR_DEFAULT = 256;

function ResizableSidebar({ children }: { children: React.ReactNode }) {
  const [width, setWidth] = useState(SIDEBAR_DEFAULT);
  const dragging = useRef(false);
  const startX = useRef(0);
  const startW = useRef(0);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    dragging.current = true;
    startX.current = e.clientX;
    startW.current = width;
    e.preventDefault();
  }, [width]);

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const delta = e.clientX - startX.current;
      setWidth(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, startW.current + delta)));
    };
    const onUp = () => { dragging.current = false; };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, []);

  return (
    <aside
      style={{ width }}
      className="relative flex flex-shrink-0 flex-col border-r border-black/[0.07] dark:border-white/[0.08] bg-slate-50 dark:bg-slate-900/60 overflow-hidden"
    >
      {children}
      {/* Drag handle */}
      <div
        onMouseDown={onMouseDown}
        className="absolute right-0 top-0 h-full w-1 cursor-col-resize hover:bg-purple-400/40 active:bg-purple-500/50 transition-colors"
      />
    </aside>
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
          skipped_unsupported?: number;
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
          const unsupported = p.skipped_unsupported ?? 0;
          const tooBig = p.skipped_too_large ?? 0;
          const reasons: string[] = [];
          if (p.skipped_native > 0) reasons.push(`${p.skipped_native} Docs/Forms`);
          if (unsupported > 0) reasons.push(`${unsupported} non-indexable types`);
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

