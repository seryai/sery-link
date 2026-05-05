// F43 / F44 / F46 / F48 — Edit credentials for cache-and-scan
// source kinds (SFTP / WebDAV / Dropbox / Azure Blob).
//
// One generic dialog that switches form per kind. The same trade-
// off as the existing EditS3CredentialsDialog: rotation is a real
// workflow (token expiry, app-password rotation, key compromise),
// and Remove + Re-add destroys the source's name / group /
// sort_order / scan-cache / manifest. Edit credentials keeps all
// of that intact.
//
// Each kind's submit re-runs the protocol's pre-flight test
// (ssh2 handshake + sftp channel for SFTP, PROPFIND for WebDAV,
// /users/get_current_account for Dropbox, List Blobs maxresults=1
// for Azure) before persisting — bad new creds surface as inline
// errors here, not as silent empty rescans.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { KeyRound, Loader2, X } from 'lucide-react';
import { useToast } from './Toast';
import {
  getAzureBlobCredentialsForSource,
  getDropboxCredentialsForSource,
  getSftpCredentialsForSource,
  getWebDavCredentialsForSource,
  type SftpAuth,
  type WebDavAuth,
} from '../utils/sources';
import type { DataSource } from '../types/events';

interface Props {
  source: DataSource;
  onClose: () => void;
  onSaved: () => void;
}

type Loaded =
  | { kind: 'sftp'; host: string; port: number; username: string; auth: SftpAuth }
  | { kind: 'web_dav'; server_url: string; auth: WebDavAuth }
  | {
      kind: 'dropbox';
      access_token: string;
      // Present when the source was added via OAuth — when set,
      // the edit form swaps to a re-auth flow instead of pasting
      // a token. Absent for legacy PAT entries.
      is_oauth: boolean;
    }
  | { kind: 'azure_blob'; sas_token: string };

export function EditCredentialsDialog({ source, onClose, onSaved }: Props) {
  const toast = useToast();
  const [loaded, setLoaded] = useState<Loaded | null | 'missing'>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Esc to cancel.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  // Load existing creds on mount based on source kind.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        let result: Loaded | null = null;
        if (source.kind.kind === 'sftp') {
          const c = await getSftpCredentialsForSource(source.id);
          if (c)
            result = {
              kind: 'sftp',
              host: c.host,
              port: c.port,
              username: c.username,
              auth: c.auth,
            };
        } else if (source.kind.kind === 'web_dav') {
          const c = await getWebDavCredentialsForSource(source.id);
          if (c) result = { kind: 'web_dav', server_url: c.server_url, auth: c.auth };
        } else if (source.kind.kind === 'dropbox') {
          const c = await getDropboxCredentialsForSource(source.id);
          if (c)
            result = {
              kind: 'dropbox',
              access_token: c.access_token,
              is_oauth: !!(c.refresh_token && c.refresh_token.trim() !== ''),
            };
        } else if (source.kind.kind === 'azure_blob') {
          const c = await getAzureBlobCredentialsForSource(source.id);
          if (c) result = { kind: 'azure_blob', sas_token: c.sas_token };
        }
        if (cancelled) return;
        setLoaded(result ?? 'missing');
      } catch (err) {
        if (cancelled) return;
        setError(`Couldn't load credentials: ${err}`);
        setLoaded('missing');
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source.id, source.kind.kind]);

  const titleForKind = () => {
    switch (source.kind.kind) {
      case 'sftp':
        return 'SFTP credentials';
      case 'web_dav':
        return 'WebDAV credentials';
      case 'dropbox':
        return 'Dropbox token';
      case 'azure_blob':
        return 'Azure SAS token';
      default:
        return 'Credentials';
    }
  };

  const onSubmit = async (next: Loaded) => {
    setError(null);
    setBusy(true);
    try {
      switch (next.kind) {
        case 'sftp':
          await invoke<void>('update_sftp_credentials', {
            sourceId: source.id,
            creds: {
              host: next.host,
              port: next.port,
              username: next.username,
              auth: next.auth,
            },
          });
          break;
        case 'web_dav':
          await invoke<void>('update_webdav_credentials', {
            sourceId: source.id,
            creds: { server_url: next.server_url, auth: next.auth },
          });
          break;
        case 'dropbox':
          await invoke<void>('update_dropbox_credentials', {
            sourceId: source.id,
            creds: { access_token: next.access_token },
          });
          break;
        case 'azure_blob': {
          if (source.kind.kind !== 'azure_blob') break;
          await invoke<void>('update_azure_blob_credentials', {
            sourceId: source.id,
            accountUrl: source.kind.account_url,
            creds: { sas_token: next.sas_token },
          });
          break;
        }
      }
      toast.success('Credentials updated');
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
              Edit {titleForKind()}
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
          {loaded === null ? (
            <div className="flex items-center gap-2 text-sm text-slate-500">
              <Loader2 className="h-4 w-4 animate-spin" />
              Loading existing credentials…
            </div>
          ) : loaded === 'missing' ? (
            <div className="rounded-md border border-amber-300 bg-amber-50 p-2 text-xs text-amber-800 dark:border-amber-900 dark:bg-amber-950/40 dark:text-amber-300">
              No existing credentials found for this source. Enter
              new credentials to save them — the source itself is
              unaffected.
            </div>
          ) : (
            <KindSpecificForm
              source={source}
              initial={loaded}
              busy={busy}
              error={error}
              onSubmit={onSubmit}
              onCancel={onClose}
            />
          )}

          {loaded === 'missing' && (
            <FreshFormFor
              source={source}
              busy={busy}
              error={error}
              onSubmit={onSubmit}
              onCancel={onClose}
            />
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Kind-specific forms ──────────────────────────────────────────

function KindSpecificForm({
  source,
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  source: DataSource;
  initial: Loaded;
  busy: boolean;
  error: string | null;
  onSubmit: (next: Loaded) => void;
  onCancel: () => void;
}) {
  return (
    <FormFor
      source={source}
      initial={initial}
      busy={busy}
      error={error}
      onSubmit={onSubmit}
      onCancel={onCancel}
    />
  );
}

function FreshFormFor({
  source,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  source: DataSource;
  busy: boolean;
  error: string | null;
  onSubmit: (next: Loaded) => void;
  onCancel: () => void;
}) {
  const empty: Loaded =
    source.kind.kind === 'sftp'
      ? {
          kind: 'sftp',
          host: source.kind.host,
          port: source.kind.port,
          username: source.kind.username,
          auth: { type: 'password', password: '' },
        }
      : source.kind.kind === 'web_dav'
        ? {
            kind: 'web_dav',
            server_url: source.kind.server_url,
            auth: { type: 'basic', username: '', password: '' },
          }
        : source.kind.kind === 'dropbox'
          ? { kind: 'dropbox', access_token: '', is_oauth: false }
          : { kind: 'azure_blob', sas_token: '' };
  return (
    <FormFor
      source={source}
      initial={empty}
      busy={busy}
      error={error}
      onSubmit={onSubmit}
      onCancel={onCancel}
    />
  );
}

function FormFor({
  source,
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  source: DataSource;
  initial: Loaded;
  busy: boolean;
  error: string | null;
  onSubmit: (next: Loaded) => void;
  onCancel: () => void;
}) {
  if (initial.kind === 'sftp')
    return (
      <SftpEditForm
        source={source}
        initial={initial}
        busy={busy}
        error={error}
        onSubmit={onSubmit}
        onCancel={onCancel}
      />
    );
  if (initial.kind === 'web_dav')
    return (
      <WebDavEditForm
        initial={initial}
        busy={busy}
        error={error}
        onSubmit={onSubmit}
        onCancel={onCancel}
      />
    );
  if (initial.kind === 'dropbox')
    return (
      <DropboxEditForm
        source={source}
        initial={initial}
        busy={busy}
        error={error}
        onSubmit={onSubmit}
        onCancel={onCancel}
      />
    );
  return (
    <AzureBlobEditForm
      initial={initial}
      busy={busy}
      error={error}
      onSubmit={onSubmit}
      onCancel={onCancel}
    />
  );
}

interface SubFormProps<T extends Loaded> {
  initial: T;
  busy: boolean;
  error: string | null;
  onSubmit: (next: Loaded) => void;
  onCancel: () => void;
}

function SftpEditForm({
  source,
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: { source: DataSource } & SubFormProps<Extract<Loaded, { kind: 'sftp' }>>) {
  const [authMode, setAuthMode] = useState<'password' | 'private_key'>(
    initial.auth.type,
  );
  const [password, setPassword] = useState(
    initial.auth.type === 'password' ? initial.auth.password : '',
  );
  const [keyPath, setKeyPath] = useState(
    initial.auth.type === 'private_key' ? initial.auth.private_key_path : '',
  );
  const [passphrase, setPassphrase] = useState(
    initial.auth.type === 'private_key' ? initial.auth.passphrase ?? '' : '',
  );

  // SFTP host/port/username come from the source kind — read-only
  // here. To change them the user removes + re-adds. Editing creds
  // is for password / key rotation only.
  const buildAuth = (): SftpAuth | null => {
    if (authMode === 'password') {
      if (!password) return null;
      return { type: 'password', password };
    }
    if (!keyPath.trim()) return null;
    return {
      type: 'private_key',
      private_key_path: keyPath.trim(),
      passphrase: passphrase || undefined,
    };
  };
  const auth = buildAuth();
  const canSubmit = !busy && auth !== null;

  return (
    <>
      <Field label="Host" value={initial.host} readonly />
      <div className="grid grid-cols-3 gap-2">
        <div className="col-span-1">
          <Field label="Port" value={String(initial.port)} readonly />
        </div>
        <div className="col-span-2">
          <Field label="Username" value={initial.username} readonly />
        </div>
      </div>
      <p className="text-xs text-slate-500 dark:text-slate-400">
        To change host / port / username, remove this source and add
        a new one. (Edit credentials only rotates the secret.)
      </p>

      <div className="rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
          <KeyRound className="h-3.5 w-3.5" />
          Authentication
        </div>
        <div className="mb-3 flex gap-2 text-xs">
          <PillButton
            active={authMode === 'password'}
            onClick={() => setAuthMode('password')}
          >
            Password
          </PillButton>
          <PillButton
            active={authMode === 'private_key'}
            onClick={() => setAuthMode('private_key')}
          >
            SSH key
          </PillButton>
        </div>
        {authMode === 'password' ? (
          <Field
            label="Password"
            value={password}
            onChange={setPassword}
            type="password"
            placeholder="re-enter to rotate"
          />
        ) : (
          <div className="grid gap-2 sm:grid-cols-2">
            <Field
              label="Private key path"
              value={keyPath}
              onChange={setKeyPath}
              placeholder="~/.ssh/id_ed25519"
            />
            <Field
              label="Passphrase (optional)"
              value={passphrase}
              onChange={setPassphrase}
              type="password"
              placeholder="if key is encrypted"
            />
          </div>
        )}
      </div>

      {error && <ErrorBox message={error} />}

      <Footer
        busy={busy}
        canSubmit={canSubmit}
        onSubmit={() =>
          auth &&
          onSubmit({
            kind: 'sftp',
            host: source.kind.kind === 'sftp' ? source.kind.host : initial.host,
            port: source.kind.kind === 'sftp' ? source.kind.port : initial.port,
            username:
              source.kind.kind === 'sftp'
                ? source.kind.username
                : initial.username,
            auth,
          })
        }
        onCancel={onCancel}
        submitLabel="Save credentials"
      />
    </>
  );
}

function WebDavEditForm({
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: SubFormProps<Extract<Loaded, { kind: 'web_dav' }>>) {
  const [authMode, setAuthMode] = useState<'anonymous' | 'basic' | 'digest'>(
    initial.auth.type,
  );
  const [username, setUsername] = useState(
    initial.auth.type !== 'anonymous' ? initial.auth.username : '',
  );
  const [password, setPassword] = useState(
    initial.auth.type !== 'anonymous' ? initial.auth.password : '',
  );

  const buildAuth = (): WebDavAuth | null => {
    if (authMode === 'anonymous') return { type: 'anonymous' };
    if (!username.trim() || !password) return null;
    return authMode === 'basic'
      ? { type: 'basic', username: username.trim(), password }
      : { type: 'digest', username: username.trim(), password };
  };
  const auth = buildAuth();
  const canSubmit = !busy && auth !== null;

  return (
    <>
      <Field label="Server URL" value={initial.server_url} readonly />
      <p className="text-xs text-slate-500 dark:text-slate-400">
        To change the server URL, remove + re-add. (Edit credentials
        only rotates the auth payload.)
      </p>
      <div className="rounded-lg border border-purple-200 bg-purple-50/60 p-3 dark:border-purple-900/60 dark:bg-purple-950/20">
        <div className="mb-2 flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-purple-700 dark:text-purple-200">
          <KeyRound className="h-3.5 w-3.5" />
          Authentication
        </div>
        <div className="mb-3 flex flex-wrap gap-2 text-xs">
          <PillButton
            active={authMode === 'basic'}
            onClick={() => setAuthMode('basic')}
          >
            Basic
          </PillButton>
          <PillButton
            active={authMode === 'digest'}
            onClick={() => setAuthMode('digest')}
          >
            Digest
          </PillButton>
          <PillButton
            active={authMode === 'anonymous'}
            onClick={() => setAuthMode('anonymous')}
          >
            Anonymous
          </PillButton>
        </div>
        {authMode !== 'anonymous' ? (
          <div className="grid gap-2 sm:grid-cols-2">
            <Field label="Username" value={username} onChange={setUsername} />
            <Field
              label="Password / app token"
              value={password}
              onChange={setPassword}
              type="password"
              placeholder="re-enter to rotate"
            />
          </div>
        ) : (
          <p className="text-xs text-purple-800/80 dark:text-purple-200/80">
            No credentials sent. Public WebDAV only.
          </p>
        )}
      </div>
      {error && <ErrorBox message={error} />}
      <Footer
        busy={busy}
        canSubmit={canSubmit}
        onSubmit={() =>
          auth && onSubmit({ kind: 'web_dav', server_url: initial.server_url, auth })
        }
        onCancel={onCancel}
        submitLabel="Save credentials"
      />
    </>
  );
}

function DropboxEditForm({
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
  source,
}: SubFormProps<Extract<Loaded, { kind: 'dropbox' }>> & {
  source: DataSource;
}) {
  // OAuth-shaped entries get a re-auth flow (no token to paste);
  // PAT entries get the existing rotate-token form. The user can
  // toggle to PAT mode if they want to switch auth styles.
  const [mode, setMode] = useState<'oauth' | 'pat'>(
    initial.is_oauth ? 'oauth' : 'pat',
  );
  return (
    <>
      <div className="mb-3 inline-flex items-center rounded-md border border-slate-200 bg-slate-50 p-0.5 text-xs dark:border-slate-700 dark:bg-slate-800/60">
        <button
          onClick={() => setMode('oauth')}
          disabled={busy}
          className={`rounded px-2.5 py-1 font-medium ${
            mode === 'oauth'
              ? 'bg-white text-slate-900 shadow-sm dark:bg-slate-900 dark:text-slate-100'
              : 'text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-slate-100'
          }`}
        >
          Sign in with Dropbox
        </button>
        <button
          onClick={() => setMode('pat')}
          disabled={busy}
          className={`rounded px-2.5 py-1 font-medium ${
            mode === 'pat'
              ? 'bg-white text-slate-900 shadow-sm dark:bg-slate-900 dark:text-slate-100'
              : 'text-slate-600 hover:text-slate-900 dark:text-slate-300 dark:hover:text-slate-100'
          }`}
        >
          Access token
        </button>
      </div>
      {mode === 'oauth' ? (
        <DropboxOAuthReauthBlock
          source={source}
          isCurrentlyOAuth={initial.is_oauth}
          busy={busy}
          error={error}
          onCancel={onCancel}
        />
      ) : (
        <DropboxPatRotateBlock
          initial={initial}
          busy={busy}
          error={error}
          onSubmit={onSubmit}
          onCancel={onCancel}
        />
      )}
    </>
  );
}

interface DropboxAuthStart {
  authorize_url: string;
  code_verifier: string;
}

function DropboxOAuthReauthBlock({
  source,
  isCurrentlyOAuth,
  busy: parentBusy,
  error: parentError,
  onCancel,
}: {
  source: DataSource;
  isCurrentlyOAuth: boolean;
  busy: boolean;
  error: string | null;
  onCancel: () => void;
}) {
  const toast = useToast();
  const [phase, setPhase] = useState<
    | { kind: 'idle' }
    | { kind: 'awaiting'; codeVerifier: string; authorizeUrl: string }
  >({ kind: 'idle' });
  const [code, setCode] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const startAuth = async () => {
    setError(null);
    setBusy(true);
    try {
      const start = await invoke<DropboxAuthStart>('start_dropbox_oauth');
      try {
        const opener = await import('@tauri-apps/plugin-opener');
        await opener.openUrl(start.authorize_url);
      } catch {
        // Falls through; URL is shown below.
      }
      setPhase({
        kind: 'awaiting',
        codeVerifier: start.code_verifier,
        authorizeUrl: start.authorize_url,
      });
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const submit = async () => {
    if (phase.kind !== 'awaiting') return;
    setError(null);
    setBusy(true);
    try {
      await invoke<void>('reauth_dropbox_source', {
        sourceId: source.id,
        code: code.trim(),
        codeVerifier: phase.codeVerifier,
      });
      toast.success('Dropbox re-authenticated');
      // Close the dialog. The dialog's own onSaved is wired by the
      // parent — but this submit path bypasses parent.onSubmit, so
      // we just dismiss via onCancel (closes without save toast).
      onCancel();
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  };

  const showBusy = busy || parentBusy;
  return (
    <>
      {phase.kind === 'idle' && (
        <>
          <p className="text-sm text-slate-700 dark:text-slate-300">
            {isCurrentlyOAuth
              ? 'This source is already authenticated via OAuth — tokens refresh automatically. Re-authenticate only if you revoked access on Dropbox or hit a stuck refresh.'
              : 'Upgrade this source from Personal Access Token to OAuth. The old token is replaced; the source name and scan cache stay intact.'}
          </p>
          {(error || parentError) && (
            <ErrorBox message={(error || parentError) as string} />
          )}
          <div className="mt-4 flex items-center justify-end gap-2">
            <button
              onClick={onCancel}
              disabled={showBusy}
              className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
            >
              Cancel
            </button>
            <button
              onClick={startAuth}
              disabled={showBusy}
              className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
            >
              {showBusy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
              {isCurrentlyOAuth ? 'Re-authenticate' : 'Open Dropbox'}
            </button>
          </div>
        </>
      )}

      {phase.kind === 'awaiting' && (
        <>
          <div className="rounded-lg border border-purple-200 bg-purple-50 p-3 text-xs text-purple-900 dark:border-purple-900/60 dark:bg-purple-950/30 dark:text-purple-100">
            Dropbox should have opened. If not,{' '}
            <a
              href={phase.authorizeUrl}
              target="_blank"
              rel="noreferrer"
              className="underline"
            >
              open this link
            </a>
            . After clicking Allow, paste the code Dropbox shows.
          </div>
          <div className="mt-3">
            <Field
              label="Authorization code"
              value={code}
              onChange={setCode}
              placeholder="Paste the code Dropbox showed you"
            />
          </div>
          {(error || parentError) && (
            <ErrorBox message={(error || parentError) as string} />
          )}
          <div className="mt-4 flex items-center justify-between gap-2">
            <button
              onClick={() => {
                setPhase({ kind: 'idle' });
                setCode('');
              }}
              disabled={showBusy}
              className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
            >
              Back
            </button>
            <div className="flex items-center gap-2">
              <button
                onClick={onCancel}
                disabled={showBusy}
                className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
              >
                Cancel
              </button>
              <button
                onClick={submit}
                disabled={showBusy || code.trim() === ''}
                className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
              >
                {showBusy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                Save
              </button>
            </div>
          </div>
        </>
      )}
    </>
  );
}

function DropboxPatRotateBlock({
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  initial: Extract<Loaded, { kind: 'dropbox' }>;
  busy: boolean;
  error: string | null;
  onSubmit: (next: Loaded) => void;
  onCancel: () => void;
}) {
  // Pre-fill from the current entry only when the source is already
  // PAT-shaped — no point seeding the input with a 4-hour OAuth
  // access_token that won't be valid by save time.
  const [accessToken, setAccessToken] = useState(
    initial.is_oauth ? '' : initial.access_token,
  );
  const canSubmit = !busy && accessToken.trim() !== '';
  return (
    <>
      <Field
        label="Access token"
        value={accessToken}
        onChange={setAccessToken}
        type="password"
        placeholder="sl.B…"
      />
      <p className="text-xs text-slate-500 dark:text-slate-400">
        {initial.is_oauth
          ? 'Switching from OAuth to Personal Access Token. The OAuth refresh token will be discarded.'
          : 'Generate a new token at dropbox.com/developers/apps if rotating. The old keychain entry is overwritten on save.'}
      </p>
      {error && <ErrorBox message={error} />}
      <Footer
        busy={busy}
        canSubmit={canSubmit}
        onSubmit={() =>
          onSubmit({
            kind: 'dropbox',
            access_token: accessToken.trim(),
            is_oauth: false,
          })
        }
        onCancel={onCancel}
        submitLabel="Save token"
      />
    </>
  );
}

function AzureBlobEditForm({
  initial,
  busy,
  error,
  onSubmit,
  onCancel,
}: SubFormProps<Extract<Loaded, { kind: 'azure_blob' }>>) {
  const [sasToken, setSasToken] = useState(initial.sas_token);
  const canSubmit = !busy && sasToken.trim().length > 16;
  return (
    <>
      <Field
        label="SAS token"
        value={sasToken}
        onChange={setSasToken}
        type="password"
        placeholder="?sv=…"
      />
      <p className="text-xs text-slate-500 dark:text-slate-400">
        Generate a new SAS in the Azure portal (Storage account →
        Shared access tokens) with Read + List permissions. Stored
        in your OS keychain.
      </p>
      {error && <ErrorBox message={error} />}
      <Footer
        busy={busy}
        canSubmit={canSubmit}
        onSubmit={() =>
          onSubmit({ kind: 'azure_blob', sas_token: sasToken.trim() })
        }
        onCancel={onCancel}
        submitLabel="Save token"
      />
    </>
  );
}

// ─── Atoms ────────────────────────────────────────────────────────

function Field({
  label,
  value,
  onChange,
  placeholder,
  type = 'text',
  readonly,
}: {
  label: string;
  value: string;
  onChange?: (v: string) => void;
  placeholder?: string;
  type?: 'text' | 'password';
  readonly?: boolean;
}) {
  return (
    <label className="block">
      <span className="mb-0.5 block text-[11px] font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
        {label}
      </span>
      <input
        type={type}
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        placeholder={placeholder}
        readOnly={readonly}
        className={`w-full rounded-md border border-slate-200 bg-white px-2 py-1.5 font-mono text-xs text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500 ${
          readonly ? 'opacity-60' : ''
        }`}
      />
    </label>
  );
}

function PillButton({
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
      type="button"
      onClick={onClick}
      className={`rounded-md border px-3 py-1 transition-colors ${
        active
          ? 'border-purple-500 bg-purple-600 text-white'
          : 'border-slate-200 bg-white text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200'
      }`}
    >
      {children}
    </button>
  );
}

function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-md border border-rose-300 bg-rose-50 p-2 text-xs text-rose-700 dark:border-rose-900 dark:bg-rose-950/40 dark:text-rose-300">
      {message}
    </div>
  );
}

function Footer({
  busy,
  canSubmit,
  onSubmit,
  onCancel,
  submitLabel,
}: {
  busy: boolean;
  canSubmit: boolean;
  onSubmit: () => void;
  onCancel: () => void;
  submitLabel: string;
}) {
  return (
    <div className="mt-3 flex items-center justify-end gap-2">
      <button
        onClick={onCancel}
        disabled={busy}
        className="rounded-md border border-slate-200 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-200"
      >
        Cancel
      </button>
      <button
        onClick={onSubmit}
        disabled={!canSubmit}
        className="inline-flex items-center gap-1.5 rounded-md bg-purple-600 px-3 py-1.5 text-sm font-semibold text-white hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-60"
      >
        {busy && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
        {submitLabel}
      </button>
    </div>
  );
}
