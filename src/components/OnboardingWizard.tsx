// First-run onboarding — local-first.
//
// Updated 2026-04-18: removed the anonymous bootstrap_workspace path.
// The app now stays fully local on first run; no cloud contact happens
// until the user clicks Connect from the StatusBar. Silent anonymous
// bootstrap was contradicting the brand promise of "no cloud contact
// until you ask for it."
//
// New one-step flow:
//   1. User picks a folder (native picker).
//   2. add_watched_folder + start_file_watcher (both local).
//   3. first_run_completed = true → App.tsx renders the main UI.
//   4. No token, no workspace, no cloud. Machines / query / schema-change
//      broadcast all gated on Connect until the user says so.
//
// The "Skip for now" path does the same without a folder.
//
// Machine-2 scenario (user already has a cloud workspace):
//   Sidebar StatusBar → Connect → paste workspace key.
//   That flow lives in ConnectModal, not here. Removing it from the
//   first-run wizard keeps onboarding single-decision.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { documentDir } from '@tauri-apps/api/path';
import {
  ArrowRight,
  Check,
  Folder as FolderIcon,
  Loader2,
  Lock,
} from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';
import seryLogo from '../assets/sery-logo.svg';
import { useToast } from './Toast';
import type { AgentConfig } from '../types/events';

// 'idle'    – fresh install, offer folder pick or skip
// 'working' – add_watched_folder / complete_first_run in flight
// 'error'   – something threw, show retry
type Phase = 'idle' | 'working' | 'error';

export function OnboardingWizard() {
  const [phase, setPhase] = useState<Phase>('idle');
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [status, setStatus] = useState<string>('');
  const toast = useToast();
  const { setConfig, setOnboardingComplete } = useAgentStore();

  /**
   * Local-only setup. No network. No keyring token written.
   * Idempotent: re-running on error is safe because all the steps
   * below are themselves idempotent (add_watched_folder dedupes,
   * start_file_watcher noops if already running).
   */
  const runSetup = async (folderPath: string | null) => {
    setPhase('working');
    setErrorMessage(null);

    try {
      // 1. Add the chosen folder (optional).
      if (folderPath) {
        setStatus('Adding your folder…');
        await invoke('add_watched_folder', {
          path: folderPath,
          recursive: true,
        });
        // Kick off the initial scan — fire-and-forget. Progress
        // surfaces in the main UI after the wizard closes.
        invoke('rescan_folder', { folderPath }).catch(err => {
          console.error('Initial scan failed:', err);
        });
      }

      // 2. Start the local file watcher so future edits get picked up.
      // We await so we can warn the user if it fails — a silent watcher
      // failure means future file changes won't be indexed, which looks
      // like a broken search two weeks later. The folder itself is
      // still added even if the watcher fails.
      try {
        await invoke('start_file_watcher');
      } catch (err) {
        console.error('File watcher failed to start:', err);
        toast.error(
          `Folder added, but the file watcher couldn't start. New files won't be detected automatically until you restart Sery. (${err})`,
        );
      }

      // NOTE: we deliberately do NOT start_websocket_tunnel here.
      // No token = no tunnel. The tunnel starts only when the user
      // clicks Connect and pastes a workspace key.

      // 3. Mark onboarding complete + reload config so App.tsx drops
      // the wizard.
      setStatus('Finishing up…');
      await invoke('complete_first_run');
      const updatedConfig = await invoke<AgentConfig>('get_config');
      setConfig(updatedConfig);
      setOnboardingComplete(true);

      toast.success(
        folderPath
          ? 'Ready to go. Sery is watching your folder locally.'
          : 'Ready to go. Add a folder from the Folders tab whenever you like.',
      );
    } catch (err) {
      setErrorMessage(friendlyError(err));
      setPhase('error');
    }
  };

  const pickFolder = async () => {
    try {
      // Open the picker at ~/Documents by default. Most users have
      // years of content there, so the first scan looks interesting
      // out of the box. Falls back to the system default if we can't
      // resolve the directory (e.g., sandbox without access).
      let defaultPath: string | undefined;
      try {
        defaultPath = await documentDir();
      } catch {
        defaultPath = undefined;
      }
      const result = await openDialog({
        directory: true,
        multiple: false,
        defaultPath,
      });
      if (typeof result === 'string') {
        setSelectedFolder(result);
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
              <p className="mb-6 text-center text-slate-600 dark:text-slate-300">
                Search your CSVs, spreadsheets, and documents by
                filename or column name. Works fully offline; no
                account needed.
              </p>
              <p className="mb-8 text-center text-sm text-slate-500 dark:text-slate-400">
                Pick a folder to get started.
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

              <p className="mt-6 flex items-center justify-center gap-2 text-xs text-slate-500 dark:text-slate-400">
                <Lock className="h-3.5 w-3.5" />
                No sign-up. No account. 100% local until you say otherwise.
              </p>

              {/* Closes I2 from UI_AUDIT_2026_05.md — surface
                  Convert-to-Parquet during the onboarding so users
                  with piles of CSVs know the feature exists. */}
              <p className="mt-3 text-center text-[11px] text-slate-400 dark:text-slate-500">
                💡 Tip: open any CSV / Excel file to convert it to
                fast Parquet in place.
              </p>
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
                Just indexing locally — nothing leaves your machine.
              </p>
            </>
          )}

          {phase === 'error' && (
            <>
              <h2 className="mb-2 text-center text-xl font-semibold text-slate-900 dark:text-slate-50">
                Setup didn't finish
              </h2>
              <p className="mb-4 text-center text-sm text-slate-600 dark:text-slate-300">
                Something went wrong adding your folder. Try again.
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

function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}

/**
 * Translate raw Rust-side errors into something a user-of-Sery can act on.
 * With the local-first rewrite, the only operations that can fail are
 * add_watched_folder (permission denied, path doesn't exist) and
 * complete_first_run (disk write). Everything network-y moved to
 * ConnectModal.
 */
function friendlyError(err: unknown): string {
  const raw = String(err);
  const lower = raw.toLowerCase();
  if (lower.includes('permission denied')) {
    return "Sery doesn't have permission to read that folder. Try a different one, or grant full-disk access in System Settings.";
  }
  if (lower.includes('not found') || lower.includes('no such file')) {
    return "That folder doesn't exist. Try picking it again.";
  }
  return raw;
}
