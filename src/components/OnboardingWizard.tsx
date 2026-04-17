// First-run onboarding — the "60 seconds to first value" path.
//
// Target from PLAN_12WEEKS.md / SPEC_FIRST_INSTALL.md: one explicit
// decision from the user (pick a folder). Everything else happens in
// the background:
//   1. User picks a folder (native picker).
//   2. bootstrap_workspace() creates an anonymous workspace + agent +
//      30-day token and stores the token in the OS keyring.
//   3. The picked folder is added to the watched-folder list and a
//      background scan kicks off.
//   4. first_run_completed is flipped; App.tsx drops the wizard and
//      renders the main UI.
//
// No mode selection. No OAuth prompt. No privacy explainer screen.
// Workspace key enrollment and advanced auth modes live in Settings
// for users who want them later.
//
// The "Skip for now" path does the same bootstrap without a folder;
// user can add folders from the Folders tab after they're in the app.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  ArrowRight,
  Check,
  Folder as FolderIcon,
  Loader2,
  Lock,
} from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import seryLogo from '../assets/sery-logo.svg';
import { useToast } from './Toast';
import type { AgentConfig } from '../types/events';
import { JoinFleetForm } from './JoinFleetForm';

// 'idle'      – fresh install, offer bootstrap or "already have a machine"
// 'joined'    – just paired via code, waiting for user to pick a folder
// 'working'   – bootstrap / folder add / complete_first_run in flight
// 'error'     – setup threw, show retry
type Phase = 'idle' | 'joined' | 'working' | 'error';

export function OnboardingWizard() {
  const [phase, setPhase] = useState<Phase>('idle');
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [status, setStatus] = useState<string>('');
  const [showJoin, setShowJoin] = useState(false);
  const toast = useToast();
  const {
    setAuthenticated,
    setAgentInfo,
    setConfig,
    setOnboardingComplete,
  } = useAgentStore();

  /**
   * Runs the whole setup flow: bootstrap if needed → add folder (if any)
   * → start watcher + tunnel → mark first_run_completed. Idempotent on
   * retry after error (bootstrap is skipped when a token already exists).
   */
  const runSetup = async (folderPath: string | null) => {
    setPhase('working');
    setErrorMessage(null);

    try {
      // 1. Bootstrap workspace unless we already have a token (e.g. user
      // paired on an earlier attempt that errored after bootstrap).
      const hasToken = await invoke<boolean>('has_token').catch(() => false);
      if (!hasToken) {
        setStatus('Setting up your workspace…');
        const token = await invoke<AgentToken>('bootstrap_workspace', {
          displayName: defaultAgentName(),
        });
        setAgentInfo(token);
        setAuthenticated(true);
      } else {
        // Reload whatever we have so the store is in sync.
        const token = await invoke<AgentToken | null>('get_agent_info');
        if (token) {
          setAgentInfo(token);
          setAuthenticated(true);
        }
      }

      // 2. Add the chosen folder (optional).
      if (folderPath) {
        setStatus('Adding your folder…');
        await invoke('add_watched_folder', {
          path: folderPath,
          recursive: true,
        });
        // Kick off the initial scan fire-and-forget — events surface
        // progress in the main UI once the wizard closes.
        invoke('rescan_folder', { folderPath }).catch(err => {
          console.error('Initial scan failed:', err);
        });
      }

      // 3. Start the tunnel + file watcher (non-fatal if they hiccup).
      invoke('start_websocket_tunnel').catch(err =>
        console.error('WebSocket tunnel failed to start:', err),
      );
      invoke('start_file_watcher').catch(err =>
        console.error('File watcher failed to start:', err),
      );

      // 4. Mark onboarding complete. App.tsx reads first_run_completed
      // from config, so we reload after flipping it.
      setStatus('Finishing up…');
      await invoke('complete_first_run');
      const updatedConfig = await invoke<AgentConfig>('get_config');
      setConfig(updatedConfig);
      setOnboardingComplete(true);

      toast.success(
        folderPath
          ? 'Setup complete. Scanning in the background.'
          : 'Setup complete. Add a folder whenever you like.',
      );
    } catch (err) {
      setErrorMessage(friendlyBootstrapError(err));
      setPhase('error');
    }
  };

  const pickFolder = async () => {
    try {
      const result = await openDialog({ directory: true, multiple: false });
      if (typeof result === 'string') {
        setSelectedFolder(result);
        // Run setup immediately on folder pick. No separate Continue
        // button — this is the 1-step flow.
        void runSetup(result);
      }
    } catch (err) {
      console.error('Folder picker failed:', err);
      toast.error(`Couldn't open folder picker: ${err}`);
    }
  };

  const skip = () => {
    void runSetup(null);
  };

  const retry = () => {
    void runSetup(selectedFolder);
  };

  return (
    <div className="flex h-screen items-center justify-center bg-gradient-to-br from-slate-50 via-white to-purple-50 p-8 dark:from-slate-950 dark:via-slate-900 dark:to-purple-950">
      <div className="w-full max-w-md">
        <Card>
          <div className="mb-6 flex items-center justify-center">
            <img src={seryLogo} alt="Sery" className="h-16 w-16" />
          </div>

          {phase === 'idle' && (
            <>
              <h1 className="mb-2 text-center text-3xl font-bold text-slate-900 dark:text-slate-50">
                Welcome to Sery
              </h1>
              <p className="mb-8 text-center text-slate-600 dark:text-slate-300">
                Pick a folder to analyze. Your files never leave this machine
                — only your questions and the answers travel.
              </p>

              <button
                onClick={pickFolder}
                className="mb-3 flex w-full items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-3 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700"
              >
                <FolderIcon className="h-5 w-5" />
                Pick a folder
              </button>

              <button
                onClick={skip}
                className="w-full rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-sm font-medium text-slate-600 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
              >
                Skip for now — I'll add folders later
              </button>

              <button
                onClick={() => setShowJoin(true)}
                className="mt-3 w-full text-center text-xs font-medium text-purple-600 hover:underline dark:text-purple-400"
              >
                I already have a Sery machine — join my fleet
              </button>

              <p className="mt-6 flex items-center justify-center gap-2 text-xs text-slate-500 dark:text-slate-400">
                <Lock className="h-3.5 w-3.5" />
                No sign-up. No account. Nothing uploaded until you ask.
              </p>
            </>
          )}

          {phase === 'joined' && (
            <>
              <h1 className="mb-2 text-center text-3xl font-bold text-slate-900 dark:text-slate-50">
                You're in the fleet
              </h1>
              <p className="mb-8 text-center text-slate-600 dark:text-slate-300">
                Now pick a folder on <strong>this</strong> machine to make it
                queryable across your fleet.
              </p>

              <button
                onClick={pickFolder}
                className="mb-3 flex w-full items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-3 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700"
              >
                <FolderIcon className="h-5 w-5" />
                Pick a folder
              </button>

              <button
                onClick={skip}
                className="w-full rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-sm font-medium text-slate-600 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
              >
                Skip for now — I'll add folders later
              </button>
            </>
          )}

          {phase === 'working' && (
            <>
              <div className="mb-4 flex items-center justify-center">
                <Loader2 className="h-10 w-10 animate-spin text-purple-600 dark:text-purple-400" />
              </div>
              <h2 className="mb-2 text-center text-xl font-semibold text-slate-900 dark:text-slate-50">
                {status || 'Setting up…'}
              </h2>
              {selectedFolder && (
                <p className="text-center text-xs text-slate-500 dark:text-slate-400">
                  {folderName(selectedFolder)}
                </p>
              )}
              <p className="mt-6 flex items-center justify-center gap-2 text-xs text-slate-500 dark:text-slate-400">
                <Check className="h-3.5 w-3.5" />
                This takes a few seconds on first run.
              </p>
            </>
          )}

          {phase === 'error' && (
            <>
              <h2 className="mb-2 text-center text-xl font-semibold text-slate-900 dark:text-slate-50">
                Setup didn't finish
              </h2>
              <p className="mb-4 text-center text-sm text-slate-600 dark:text-slate-300">
                Something went wrong. Check your connection and try again.
              </p>
              <div className="mb-6 max-h-32 overflow-y-auto rounded-lg border border-rose-200 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
                {errorMessage ?? 'Unknown error'}
              </div>

              <button
                onClick={retry}
                className="mb-3 flex w-full items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-3 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700"
              >
                Try again <ArrowRight className="h-4 w-4" />
              </button>

              <button
                onClick={() => {
                  setPhase('idle');
                  setErrorMessage(null);
                }}
                className="w-full rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-sm font-medium text-slate-600 transition-colors hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
              >
                Back to start
              </button>
            </>
          )}
        </Card>
      </div>

      {showJoin && (
        <JoinFleetForm
          defaultDisplayName={defaultAgentName()}
          onClose={() => setShowJoin(false)}
          onPaired={token => {
            // pair_complete saved the token to keyring on the Rust side.
            // Sync Zustand so the rest of the app sees us as authenticated
            // immediately, then prompt the user to pick a folder on THIS
            // machine. runSetup() will skip bootstrap because has_token
            // returns true.
            setAgentInfo(token);
            setAuthenticated(true);
            setShowJoin(false);
            setPhase('joined');
            toast.success('Joined fleet. Pick a folder to finish setup.');
          }}
        />
      )}
    </div>
  );
}

// ─── Primitives ────────────────────────────────────────────────────────────

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="animate-slide_up rounded-2xl border border-slate-200 bg-white p-8 shadow-xl dark:border-slate-800 dark:bg-slate-900">
      {children}
    </div>
  );
}

// ─── Helpers ───────────────────────────────────────────────────────────────

function detectPlatform(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes('mac')) return 'macOS';
  if (ua.includes('win')) return 'Windows';
  if (ua.includes('linux')) return 'Linux';
  return 'Unknown';
}

function defaultAgentName(): string {
  const platform = detectPlatform();
  return platform === 'Unknown' ? 'My Computer' : `My ${platform}`;
}

function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Translate raw Rust-side errors into something a user-of-Sery can act on.
 * Raw errors look like: "Authentication error: bootstrap failed: 429 ..."
 * or "HTTP error: error sending request for url (...): operation timed out".
 *
 * Anything we don't recognize falls through with the raw text — never
 * silently swallow a genuine bug signal.
 */
function friendlyBootstrapError(err: unknown): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (lower.includes('timed out') || lower.includes('timeout')) {
    return "Can't reach Sery. Check your internet connection and try again.";
  }
  if (lower.includes('network') || lower.includes('connection refused')) {
    return "Can't reach Sery. Check your internet connection and try again.";
  }
  if (raw.includes('429')) {
    return 'Too many signups from your network. Wait a minute and try again.';
  }
  if (raw.includes('500') || raw.includes('502') || raw.includes('503')) {
    return "Sery's servers are having a moment. Try again in a minute.";
  }
  if (lower.includes('keyring')) {
    return 'Your OS keyring rejected the new token. On Linux, make sure gnome-keyring (or equivalent) is running.';
  }
  return raw;
}
