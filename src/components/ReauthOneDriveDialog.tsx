// F49 — Re-authorize an existing OneDrive source.
//
// Mirrors the device-code flow from AddSourceModal's OneDriveStage
// but doesn't ask for a base_path (already set on the source) and
// calls reauth_onedrive_source instead of add_onedrive_source —
// preserving the source's name / group / sort_order / cache /
// sync-manifest. Used when the stored refresh_token has been
// revoked (password change, permission revocation, ...).

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import type { DataSource } from '../types/events';

interface OneDriveCreds {
  access_token: string;
  refresh_token: string;
  expires_at: string;
}

interface DeviceCodeStart {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
  message: string;
}

type PollResult =
  | { status: 'pending' }
  | { status: 'slow_down' }
  | { status: 'completed'; creds: OneDriveCreds };

interface Props {
  source: DataSource;
  onClose: () => void;
  onSaved: () => void;
}

export function ReauthOneDriveDialog({ source, onClose, onSaved }: Props) {
  const toast = useToast();
  const [phase, setPhase] = useState<
    | { kind: 'idle' }
    | { kind: 'starting' }
    | {
        kind: 'polling';
        deviceCode: string;
        userCode: string;
        verificationUri: string;
        intervalMs: number;
      }
    | { kind: 'completed'; creds: OneDriveCreds }
    | { kind: 'saving' }
  >({ kind: 'idle' });
  const [error, setError] = useState<string | null>(null);

  // Esc to cancel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // Poll loop while we're in 'polling' phase.
  useEffect(() => {
    if (phase.kind !== 'polling') return;
    let cancelled = false;
    let interval = phase.intervalMs;

    const tick = async () => {
      if (cancelled) return;
      try {
        const result = await invoke<PollResult>('poll_onedrive_auth', {
          deviceCode: phase.deviceCode,
        });
        if (cancelled) return;
        if (result.status === 'completed') {
          // Auto-save: re-auth flow's whole purpose is the new tokens;
          // no extra fields to collect, so persist immediately.
          setPhase({ kind: 'saving' });
          try {
            await invoke<void>('reauth_onedrive_source', {
              sourceId: source.id,
              creds: result.creds,
            });
            toast.success('OneDrive re-authorized');
            onSaved();
            onClose();
          } catch (saveErr) {
            setError(String(saveErr));
            setPhase({ kind: 'idle' });
          }
          return;
        }
        if (result.status === 'slow_down') {
          interval = Math.min(interval * 2, 30_000);
        }
        setTimeout(tick, interval);
      } catch (err) {
        if (cancelled) return;
        setError(String(err));
        setPhase({ kind: 'idle' });
      }
    };
    setTimeout(tick, interval);
    return () => {
      cancelled = true;
    };
  }, [phase, source.id, toast, onSaved, onClose]);

  const startAuth = async () => {
    setError(null);
    setPhase({ kind: 'starting' });
    try {
      const start = await invoke<DeviceCodeStart>('start_onedrive_auth');
      setPhase({
        kind: 'polling',
        deviceCode: start.device_code,
        userCode: start.user_code,
        verificationUri: start.verification_uri,
        intervalMs: Math.max(start.interval, 1) * 1000,
      });
    } catch (err) {
      setError(String(err));
      setPhase({ kind: 'idle' });
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-4"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-full max-w-md rounded-lg border border-slate-200 bg-white shadow-xl dark:border-slate-700 dark:bg-slate-900"
      >
        <div className="flex items-start justify-between border-b border-slate-200 px-5 py-4 dark:border-slate-800">
          <div>
            <h2 className="flex items-center gap-2 text-base font-semibold text-slate-800 dark:text-slate-100">
              <KeyRound className="h-4 w-4 text-purple-600 dark:text-purple-400" />
              Re-authorize OneDrive
            </h2>
            <p className="mt-0.5 truncate text-xs text-slate-500 dark:text-slate-400">
              {source.name}
            </p>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-slate-500 hover:bg-slate-100 dark:hover:bg-slate-800"
            aria-label="Close"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        <div className="space-y-3 p-5">
          {phase.kind === 'idle' && (
            <>
              <p className="text-sm text-slate-700 dark:text-slate-300">
                Sign in again via Microsoft's device code flow. Useful
                if your refresh token was revoked (password change,
                permission revocation, etc.) — the source's name,
                group, cache, and sync history all stay intact.
              </p>
              {error && (
                <div className="rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
                  {error}
                </div>
              )}
              <div className="flex items-center justify-end gap-2 pt-3">
                <button
                  onClick={onClose}
                  className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
                >
                  Cancel
                </button>
                <button
                  onClick={startAuth}
                  className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
                >
                  Start re-auth
                </button>
              </div>
            </>
          )}

          {phase.kind === 'starting' && (
            <div className="flex items-center gap-2 text-sm text-slate-500">
              <Loader2 className="h-4 w-4 animate-spin" />
              Requesting device code from Microsoft…
            </div>
          )}

          {phase.kind === 'polling' && (
            <>
              <div className="rounded-lg border border-purple-200 bg-purple-50 p-4 text-center dark:border-purple-900/60 dark:bg-purple-950/30">
                <p className="text-xs uppercase tracking-wide text-purple-700 dark:text-purple-200">
                  Enter this code at{' '}
                  <span className="font-mono">
                    {phase.verificationUri}
                  </span>
                </p>
                <p className="mt-2 font-mono text-3xl font-bold tracking-widest text-purple-900 dark:text-purple-100">
                  {phase.userCode}
                </p>
              </div>
              <p className="flex items-center gap-2 text-sm text-slate-600 dark:text-slate-400">
                <Loader2 className="h-4 w-4 animate-spin" />
                Waiting for sign-in to complete…
              </p>
              <div className="flex justify-end pt-3">
                <button
                  onClick={() => setPhase({ kind: 'idle' })}
                  className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
                >
                  Cancel sign-in
                </button>
              </div>
            </>
          )}

          {phase.kind === 'saving' && (
            <div className="flex items-center gap-2 text-sm text-slate-500">
              <Loader2 className="h-4 w-4 animate-spin" />
              Verifying tokens + saving…
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
