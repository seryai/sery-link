// Add Another Machine modal — Sery Link v1 scaffold.
//
// Generates a one-time pair code from the backend (via the pair_request
// Tauri command), displays it prominently with a live expiry timer, and
// polls pair_status every 2s to detect when the second machine redeems
// the code. When the server returns status=completed, transitions to a
// "connected" confirmation state.
//
// Designed to stand alone — no router changes, no Settings modifications
// required. Call <AddMachineModal onClose={...}/> from wherever you want
// to offer the action (tray menu, fleet view, settings, etc.).
//
// Deliberately omitted in v1:
//   * QR code rendering — needs a JS lib; defer to v1.1. Users copy-paste
//     the code for now.
//   * Localization — English only, strings inline.
//   * Error recovery UI — errors surface in-modal as a simple banner.
//
// Paired backend: api/app/api/v1/agent_pairing.py + SPEC_PAIR_FLOW.md.

import { useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import QRCode from 'qrcode';

type PairRequestResponse = {
  pair_code: string;        // formatted "XXX-XXX-XXX-XXX"
  expires_at: string;
  expires_in_seconds: number;
  qr_url: string;
};

type PairStatusResponse = {
  status: 'pending' | 'completed' | 'expired';
  expires_at?: string | null;
  new_agent?: {
    agent_id?: string;
    display_name?: string;
    os_type?: string;
    hostname?: string;
    completed_at?: string;
  } | null;
};

type Phase = 'loading' | 'pending' | 'completed' | 'expired' | 'error';

interface Props {
  onClose: () => void;
  onPaired?: (newAgent: NonNullable<PairStatusResponse['new_agent']>) => void;
}

export function AddMachineModal({ onClose, onPaired }: Props) {
  const [phase, setPhase] = useState<Phase>('loading');
  const [pairCode, setPairCode] = useState<string | null>(null);
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [qrDataUrl, setQrDataUrl] = useState<string | null>(null);
  const [secondsLeft, setSecondsLeft] = useState(0);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [newAgent, setNewAgent] = useState<PairStatusResponse['new_agent'] | null>(null);

  const pollRef = useRef<number | null>(null);
  const tickRef = useRef<number | null>(null);

  // Render the QR code to a data URL whenever qrUrl changes. Local
  // generation (no external service) keeps the QR visible even when
  // offline and avoids leaking pair codes to third parties.
  useEffect(() => {
    if (!qrUrl) {
      setQrDataUrl(null);
      return;
    }
    let cancelled = false;
    QRCode.toDataURL(qrUrl, {
      errorCorrectionLevel: 'H',
      margin: 2,
      width: 240,
      color: {
        dark: '#7c3aed', // purple-600
        light: '#ffffff',
      },
    })
      .then(url => {
        if (!cancelled) setQrDataUrl(url);
      })
      .catch(() => {
        // QR render failure is non-fatal — the code + URL remain visible.
        if (!cancelled) setQrDataUrl(null);
      });
    return () => {
      cancelled = true;
    };
  }, [qrUrl]);

  // Kick off pair_request on mount; allow manual refresh on expiry.
  const requestCode = async () => {
    setPhase('loading');
    setErrorMsg(null);
    setNewAgent(null);
    try {
      const resp = await invoke<PairRequestResponse>('pair_request');
      setPairCode(resp.pair_code);
      setQrUrl(resp.qr_url);
      setSecondsLeft(resp.expires_in_seconds);
      setPhase('pending');
    } catch (err) {
      setErrorMsg(String(err));
      setPhase('error');
    }
  };

  useEffect(() => {
    requestCode();
    // Cleanup intervals on unmount
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current);
      if (tickRef.current) window.clearInterval(tickRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Countdown timer — updates secondsLeft every second while pending.
  useEffect(() => {
    if (phase !== 'pending') return;
    tickRef.current = window.setInterval(() => {
      setSecondsLeft(prev => {
        if (prev <= 1) {
          if (tickRef.current) window.clearInterval(tickRef.current);
          setPhase('expired');
          return 0;
        }
        return prev - 1;
      });
    }, 1000);
    return () => {
      if (tickRef.current) window.clearInterval(tickRef.current);
    };
  }, [phase]);

  // Poll /pair-status every 2s while pending to detect second-machine join.
  useEffect(() => {
    if (phase !== 'pending' || !pairCode) return;
    pollRef.current = window.setInterval(async () => {
      try {
        const s = await invoke<PairStatusResponse>('pair_status', { code: pairCode });
        if (s.status === 'completed' && s.new_agent) {
          setNewAgent(s.new_agent);
          setPhase('completed');
          if (pollRef.current) window.clearInterval(pollRef.current);
          onPaired?.(s.new_agent);
        } else if (s.status === 'expired') {
          setPhase('expired');
          if (pollRef.current) window.clearInterval(pollRef.current);
        }
      } catch (err) {
        // Keep polling even on transient network errors — the UI will
        // catch real expiry via the countdown timer. Log for debugging.
        // eslint-disable-next-line no-console
        console.warn('pair_status poll failed:', err);
      }
    }, 2000);
    return () => {
      if (pollRef.current) window.clearInterval(pollRef.current);
    };
  }, [phase, pairCode, onPaired]);

  const copyCode = async () => {
    if (!pairCode) return;
    try {
      await navigator.clipboard.writeText(pairCode);
    } catch {
      // clipboard may be blocked; no-op — the code is still visible.
    }
  };

  const timeDisplay = useMemo(() => {
    const m = Math.floor(secondsLeft / 60);
    const s = secondsLeft % 60;
    return `${m}:${s.toString().padStart(2, '0')}`;
  }, [secondsLeft]);

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/50"
      onClick={onClose}
      role="presentation"
    >
      <div
        className="w-full max-w-md rounded-xl bg-white p-6 shadow-xl dark:bg-slate-900"
        onClick={e => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-machine-title"
      >
        <div className="mb-4 flex items-start justify-between">
          <h2 id="add-machine-title" className="text-lg font-semibold text-slate-900 dark:text-slate-50">
            Add Another Machine
          </h2>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-slate-400 hover:bg-slate-100 hover:text-slate-700 dark:hover:bg-slate-800"
            aria-label="Close"
          >
            ×
          </button>
        </div>

        {phase === 'loading' && (
          <p className="text-sm text-slate-600 dark:text-slate-400">Generating pair code…</p>
        )}

        {phase === 'error' && (
          <div className="space-y-3">
            <div className="rounded-md border border-rose-300 bg-rose-50 p-3 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
              Couldn't generate a pair code. {errorMsg ?? 'Check your connection.'}
            </div>
            <button
              onClick={requestCode}
              className="rounded-lg bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Try again
            </button>
          </div>
        )}

        {phase === 'pending' && pairCode && (
          <div className="space-y-4">
            <p className="text-sm text-slate-600 dark:text-slate-400">
              Install Sery on another machine — your home PC, work laptop, server — then scan
              this QR with your phone or type the code below:
            </p>

            {qrDataUrl && (
              <div className="flex justify-center">
                <img
                  src={qrDataUrl}
                  alt="QR code for pairing"
                  width={240}
                  height={240}
                  className="rounded-lg border border-slate-200 bg-white dark:border-slate-800"
                />
              </div>
            )}

            <div className="flex items-center justify-center gap-2 rounded-lg border border-purple-200 bg-purple-50 py-4 dark:border-purple-900 dark:bg-purple-900/20">
              <span className="select-all font-mono text-2xl font-bold tracking-wider text-purple-700 dark:text-purple-200">
                {pairCode}
              </span>
              <button
                onClick={copyCode}
                className="ml-2 rounded-md border border-purple-300 px-2 py-1 text-xs font-medium text-purple-700 hover:bg-purple-100 dark:border-purple-700 dark:text-purple-300 dark:hover:bg-purple-900/40"
              >
                Copy
              </button>
            </div>

            <div className={`text-center text-xs ${secondsLeft < 30 ? 'text-rose-500' : 'text-slate-500 dark:text-slate-400'}`}>
              Expires in <span className="font-semibold">{timeDisplay}</span>
            </div>

            <p className="text-center text-xs text-slate-500 dark:text-slate-400">
              Waiting for the other machine…
            </p>
          </div>
        )}

        {phase === 'completed' && newAgent && (
          <div className="space-y-4 text-center">
            <div className="mx-auto flex h-12 w-12 items-center justify-center rounded-full bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300">
              ✓
            </div>
            <div>
              <p className="text-sm font-medium text-slate-900 dark:text-slate-100">
                New machine connected
              </p>
              <p className="mt-1 text-base font-semibold text-slate-900 dark:text-slate-50">
                {newAgent.display_name ?? 'Unnamed machine'}
              </p>
              <p className="text-xs text-slate-500 dark:text-slate-400">
                {newAgent.os_type ?? 'unknown OS'} • just now
              </p>
            </div>
            <button
              onClick={onClose}
              className="mx-auto rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Done
            </button>
          </div>
        )}

        {phase === 'expired' && (
          <div className="space-y-4">
            <div className="rounded-md border border-amber-300 bg-amber-50 p-3 text-sm text-amber-800 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-200">
              This code expired for security. Generate a fresh one below.
            </div>
            <button
              onClick={requestCode}
              className="w-full rounded-lg bg-purple-600 px-3 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Generate new code
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
