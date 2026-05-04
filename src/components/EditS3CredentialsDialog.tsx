// F42 — Edit existing S3 credentials for a registered S3 source.
//
// Shown when the user picks "Edit credentials…" from the right-click
// menu on an S3 source. Loads the current creds via Tauri (they're
// in the OS keychain), pre-populates the form, and saves via the
// same add_remote_source command — which overwrites the keychain
// entry and runs the same pre-flight test as add. So the user gets
// the same "bad keys surface here, not as a silent empty scan"
// guarantee on rotation as on initial add.
//
// Why this exists vs Remove + re-Add: rotation is a real workflow
// (AWS short-lived STS tokens, periodic key rotation). The remove
// path also drops the source from sources Vec + scan cache, which
// is destructive and loses sort_order / group / name. Edit creds
// keeps everything else intact.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import { getS3CredentialsForUrl, type S3Credentials } from '../utils/sources';
import type { DataSource } from '../types/events';

interface Props {
  /** The S3 source whose creds are being edited. The kind is asserted
   *  to be S3 by the caller — this dialog only ever opens for S3
   *  sources via the context menu's `if (source.kind.kind === 's3')`
   *  guard. */
  source: DataSource;
  onClose: () => void;
  /** Fires after a successful save. The parent should reload its
   *  config (mostly so the in-flight scan kicked by the rescan call
   *  registers in scansInFlight for the StatusPill). */
  onSaved: () => void;
}

export function EditS3CredentialsDialog({ source, onClose, onSaved }: Props) {
  const toast = useToast();
  // Loading state: null = still fetching from keychain, then a real
  // S3Credentials once loaded. Bad outcome (no entry / read failure)
  // surfaces as the form-level error.
  const [loaded, setLoaded] = useState<S3Credentials | null>(null);
  const [accessKey, setAccessKey] = useState('');
  const [secretKey, setSecretKey] = useState('');
  const [region, setRegion] = useState('');
  const [sessionToken, setSessionToken] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Sanity: only S3 should reach here, but be defensive.
  const url =
    source.kind.kind === 's3' ? source.kind.url : '';

  // Load existing creds on mount.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const creds = await getS3CredentialsForUrl(url);
        if (cancelled) return;
        if (creds) {
          setLoaded(creds);
          setAccessKey(creds.access_key_id);
          setSecretKey(creds.secret_access_key);
          setRegion(creds.region);
          setSessionToken(creds.session_token ?? '');
        } else {
          setError(
            "Couldn't find existing credentials for this source. " +
              'Enter new credentials below to save them.',
          );
          // Fall through with empty fields — the user can still save
          // fresh creds and the existing source is unaffected.
          setLoaded({
            access_key_id: '',
            secret_access_key: '',
            region: 'us-east-1',
            session_token: undefined,
          });
          setRegion('us-east-1');
        }
      } catch (err) {
        if (cancelled) return;
        setError(`Couldn't load credentials: ${err}`);
        // Same fall-through: render the form so the user can write
        // creds even if the read failed.
        setLoaded({
          access_key_id: '',
          secret_access_key: '',
          region: 'us-east-1',
          session_token: undefined,
        });
        setRegion('us-east-1');
      }
    })();
    return () => {
      cancelled = true;
    };
    // url is stable per dialog open — passing source.id here would
    // also work; keep url since it's the actual lookup key.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [url]);

  // Esc to cancel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const canSubmit =
    loaded !== null &&
    !busy &&
    accessKey.trim() !== '' &&
    secretKey.trim() !== '' &&
    region.trim() !== '';

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const creds: S3Credentials = {
        access_key_id: accessKey.trim(),
        secret_access_key: secretKey.trim(),
        region: region.trim(),
        session_token: sessionToken.trim() || undefined,
      };
      // add_remote_source's pre-flight runs DuckDB-side region/auth
      // verification before persisting — so bad keys surface here
      // as an inline error instead of as a silent empty rescan
      // ten minutes later.
      await invoke<string>('add_remote_source', { url, credentials: creds });
      toast.success('Credentials updated');
      // Kick a fresh rescan so the user sees data flow with the new
      // keys without having to right-click → Rescan.
      invoke('rescan_folder', { folderPath: url }).catch((err) => {
        console.error('Rescan after cred update failed:', err);
      });
      onSaved();
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
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
              Edit credentials
            </h2>
            <p className="mt-0.5 truncate text-xs text-slate-500 dark:text-slate-400">
              {source.name} · <span className="font-mono">{url}</span>
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
          {loaded === null ? (
            <div className="flex items-center gap-2 text-sm text-slate-500">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading existing credentials…
            </div>
          ) : (
            <>
              <CredField
                label="Access key ID"
                value={accessKey}
                onChange={setAccessKey}
                placeholder="AKIA…"
              />
              <CredField
                label="Secret access key"
                value={secretKey}
                onChange={setSecretKey}
                type="password"
                placeholder="•••• stored — re-enter to change or keep as-is"
              />
              <CredField
                label="Region"
                value={region}
                onChange={setRegion}
                placeholder="us-east-1"
              />
              <CredField
                label="Session token (optional)"
                value={sessionToken}
                onChange={setSessionToken}
                type="password"
                placeholder="Only for temporary STS creds"
              />
              <p className="text-xs text-slate-500 dark:text-slate-400">
                Pre-flight tested before save — bad keys surface here,
                not later as a silent empty rescan. Old keychain entry
                is overwritten on success.
              </p>
            </>
          )}

          {error && (
            <div className="rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
              {error}
            </div>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-slate-200 px-5 py-3 dark:border-slate-800">
          <button
            onClick={onClose}
            disabled={busy}
            className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
          >
            Cancel
          </button>
          <button
            onClick={submit}
            disabled={!canSubmit}
            className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
          >
            {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            Save credentials
          </button>
        </div>
      </div>
    </div>
  );
}

function CredField({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: 'text' | 'password';
}) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {label}
      </span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder}
        className="w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
      />
    </label>
  );
}
