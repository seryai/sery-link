// 5-step onboarding wizard shown on first run.
//
// Steps:
//   1. Welcome      — value prop + privacy promise
//   2. Connect      — OAuth loopback flow
//   3. First folder — pick a folder to analyze (optional skip)
//   4. Privacy      — transparent summary of what leaves the device
//   5. Done         — success, CTA to open Sery in the browser
//
// On completion, `complete_first_run` flips config.app.first_run_completed
// so the wizard never shows again on this machine.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import {
  ArrowRight,
  Check,
  Cloud,
  Database,
  Eye,
  Folder as FolderIcon,
  Loader2,
  Lock,
  LogIn,
  Shield,
  Sparkles,
  Zap,
} from 'lucide-react';
import { useAgentStore, type AgentToken } from '../stores/agentStore';
import { useToast } from './Toast';
import type { AgentConfig } from '../types/events';

type Step = 'welcome' | 'connect' | 'folder' | 'privacy' | 'done';

const STEPS: Step[] = ['welcome', 'connect', 'folder', 'privacy', 'done'];

export function OnboardingWizard() {
  const [step, setStep] = useState<Step>('welcome');
  const toast = useToast();

  const goNext = () => {
    const idx = STEPS.indexOf(step);
    if (idx < STEPS.length - 1) setStep(STEPS[idx + 1]);
  };

  const goPrev = () => {
    const idx = STEPS.indexOf(step);
    if (idx > 0) setStep(STEPS[idx - 1]);
  };

  return (
    <div className="flex min-h-screen flex-col bg-gradient-to-br from-slate-50 via-white to-purple-50 dark:from-slate-950 dark:via-slate-900 dark:to-purple-950">
      {/* Progress indicator */}
      <div className="flex items-center justify-center gap-2 pt-8">
        {STEPS.map((s, i) => {
          const current = STEPS.indexOf(step);
          const filled = i <= current;
          return (
            <div
              key={s}
              className={`h-1.5 w-10 rounded-full transition-colors ${
                filled
                  ? 'bg-purple-600 dark:bg-purple-500'
                  : 'bg-slate-200 dark:bg-slate-800'
              }`}
            />
          );
        })}
      </div>

      <div className="flex flex-1 items-center justify-center p-8">
        <div className="w-full max-w-lg">
          {step === 'welcome' && <WelcomeStep onNext={goNext} />}
          {step === 'connect' && <ConnectStep onNext={goNext} onBack={goPrev} toast={toast} />}
          {step === 'folder' && <FolderStep onNext={goNext} onBack={goPrev} toast={toast} />}
          {step === 'privacy' && <PrivacyStep onNext={goNext} onBack={goPrev} />}
          {step === 'done' && <DoneStep />}
        </div>
      </div>
    </div>
  );
}

// ─── Step 1: Welcome ────────────────────────────────────────────────────────

function WelcomeStep({ onNext }: { onNext: () => void }) {
  return (
    <Card>
      <div className="mb-6 flex items-center justify-center">
        <div className="flex h-16 w-16 items-center justify-center rounded-2xl bg-purple-100 dark:bg-purple-900/40">
          <Sparkles className="h-8 w-8 text-purple-600 dark:text-purple-300" />
        </div>
      </div>

      <h1 className="mb-2 text-center text-3xl font-bold text-slate-900 dark:text-slate-50">
        Welcome to Sery Link
      </h1>
      <p className="mb-8 text-center text-slate-600 dark:text-slate-300">
        Query your local files with natural language. Your data stays on your
        machine — only schemas go to the cloud.
      </p>

      <div className="mb-8 space-y-3">
        <Bullet
          icon={<Database className="h-5 w-5" />}
          title="Analyze any dataset"
          body="Parquet, CSV, and Excel files work out of the box."
        />
        <Bullet
          icon={<Lock className="h-5 w-5" />}
          title="Private by design"
          body="Raw rows never leave this device — queries run locally."
        />
        <Bullet
          icon={<Zap className="h-5 w-5" />}
          title="Powered by Claude"
          body="Ask in English. We translate to SQL and execute locally."
        />
      </div>

      <PrimaryButton onClick={onNext}>
        Get started <ArrowRight className="h-4 w-4" />
      </PrimaryButton>
    </Card>
  );
}

// ─── Step 2: Connect ────────────────────────────────────────────────────────

function ConnectStep({
  onNext,
  onBack,
  toast,
}: {
  onNext: () => void;
  onBack: () => void;
  toast: ReturnType<typeof useToast>;
}) {
  const [agentName, setAgentName] = useState(() => defaultAgentName());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { setAuthenticated, setAgentInfo, setConfig } = useAgentStore();

  const handleConnect = async () => {
    if (!agentName.trim()) {
      setError('Please enter a name for this agent.');
      return;
    }
    setLoading(true);
    setError(null);
    try {
      const token = await invoke<AgentToken>('start_auth_flow', {
        agentName: agentName.trim(),
        platform: detectPlatform(),
      });
      setAgentInfo(token);
      setAuthenticated(true);

      // Refresh config so downstream steps see `authenticated` state
      try {
        const config = await invoke<AgentConfig>('get_config');
        setConfig(config);
      } catch {
        /* ignore — config reload is best-effort here */
      }

      // Start the tunnel in the background — not fatal if it fails
      try {
        await invoke('start_websocket_tunnel');
      } catch (err) {
        console.error('WebSocket tunnel failed to start:', err);
      }

      toast.success('Connected to Sery');
      onNext();
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  };

  return (
    <Card>
      <StepHeader
        icon={<LogIn className="h-6 w-6" />}
        title="Connect your account"
        subtitle="We'll open your browser to sign in. No credentials are stored here."
      />

      <label
        htmlFor="agentName"
        className="mb-2 block text-sm font-medium text-slate-700 dark:text-slate-200"
      >
        Give this device a name
      </label>
      <input
        id="agentName"
        type="text"
        value={agentName}
        onChange={(e) => setAgentName(e.target.value)}
        placeholder="e.g., Work MacBook"
        disabled={loading}
        className="mb-4 w-full rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-100 dark:placeholder-slate-500"
      />

      {error && (
        <div className="mb-4 rounded-lg border border-rose-200 bg-rose-50 px-4 py-2.5 text-sm text-rose-700 dark:border-rose-900 dark:bg-rose-950/50 dark:text-rose-300">
          {error}
        </div>
      )}

      <div className="flex gap-2">
        <SecondaryButton onClick={onBack} disabled={loading}>
          Back
        </SecondaryButton>
        <PrimaryButton onClick={handleConnect} disabled={loading}>
          {loading ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Opening browser…
            </>
          ) : (
            <>
              <LogIn className="h-4 w-4" />
              Sign in to Sery
            </>
          )}
        </PrimaryButton>
      </div>
    </Card>
  );
}

// ─── Step 3: First folder ───────────────────────────────────────────────────

function FolderStep({
  onNext,
  onBack,
  toast,
}: {
  onNext: () => void;
  onBack: () => void;
  toast: ReturnType<typeof useToast>;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  const [scanning, setScanning] = useState(false);
  const { setConfig } = useAgentStore();

  const pick = async () => {
    try {
      const result = await openDialog({ directory: true, multiple: false });
      if (typeof result === 'string') {
        setSelected(result);
      }
    } catch (err) {
      console.error('Folder picker failed:', err);
    }
  };

  const addAndScan = async () => {
    if (!selected) return;
    setScanning(true);
    try {
      await invoke('add_watched_folder', { path: selected, recursive: true });

      // Start the file watcher so the folder is actively monitored
      try {
        await invoke('start_file_watcher');
      } catch (err) {
        console.error('File watcher failed to start:', err);
      }

      // Kick off the initial scan (fire-and-forget — event stream will
      // surface progress on the dashboard).
      invoke('rescan_folder', { folder: selected }).catch((err) => {
        console.error('Initial scan failed:', err);
      });

      const config = await invoke<AgentConfig>('get_config');
      setConfig(config);
      toast.success('Folder added — scanning in the background');
      onNext();
    } catch (err) {
      toast.error(`Couldn't add folder: ${err}`);
    } finally {
      setScanning(false);
    }
  };

  return (
    <Card>
      <StepHeader
        icon={<FolderIcon className="h-6 w-6" />}
        title="Pick your first folder"
        subtitle="Choose a folder that contains Parquet, CSV, or Excel files."
      />

      <button
        onClick={pick}
        disabled={scanning}
        className="mb-4 flex w-full items-center gap-3 rounded-lg border-2 border-dashed border-slate-300 bg-slate-50 p-4 text-left transition-colors hover:border-purple-400 hover:bg-purple-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:hover:border-purple-600 dark:hover:bg-purple-950/30"
      >
        <FolderIcon className="h-6 w-6 shrink-0 text-slate-400 dark:text-slate-500" />
        <div className="min-w-0 flex-1">
          {selected ? (
            <>
              <div className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
                {folderName(selected)}
              </div>
              <div className="truncate text-xs text-slate-500 dark:text-slate-400">
                {selected}
              </div>
            </>
          ) : (
            <>
              <div className="text-sm font-medium text-slate-700 dark:text-slate-200">
                Click to browse
              </div>
              <div className="text-xs text-slate-500 dark:text-slate-400">
                You can add more folders later from Settings
              </div>
            </>
          )}
        </div>
      </button>

      <div className="mb-6 rounded-lg bg-sky-50 px-4 py-3 text-xs text-sky-900 dark:bg-sky-950/40 dark:text-sky-200">
        <strong>Tip:</strong> Start with a folder you query often — like
        ~/Downloads or a data project directory.
      </div>

      <div className="flex gap-2">
        <SecondaryButton onClick={onBack} disabled={scanning}>
          Back
        </SecondaryButton>
        <SecondaryButton onClick={onNext} disabled={scanning}>
          Skip for now
        </SecondaryButton>
        <PrimaryButton onClick={addAndScan} disabled={!selected || scanning}>
          {scanning ? (
            <>
              <Loader2 className="h-4 w-4 animate-spin" />
              Adding…
            </>
          ) : (
            <>
              Continue <ArrowRight className="h-4 w-4" />
            </>
          )}
        </PrimaryButton>
      </div>
    </Card>
  );
}

// ─── Step 4: Privacy ────────────────────────────────────────────────────────

function PrivacyStep({
  onNext,
  onBack,
}: {
  onNext: () => void;
  onBack: () => void;
}) {
  return (
    <Card>
      <StepHeader
        icon={<Shield className="h-6 w-6" />}
        title="Your data stays local"
        subtitle="Here's exactly what the agent sends to the cloud."
      />

      <div className="mb-6 space-y-3">
        <PrivacyRow
          icon={<Cloud className="h-5 w-5 text-emerald-500" />}
          kind="sent"
          label="File paths, schemas, column names"
          detail="So Sery can plan queries and show column pickers."
        />
        <PrivacyRow
          icon={<Cloud className="h-5 w-5 text-emerald-500" />}
          kind="sent"
          label="Row counts and file sizes"
          detail="Used to estimate query cost and show dataset stats."
        />
        <PrivacyRow
          icon={<Cloud className="h-5 w-5 text-emerald-500" />}
          kind="sent"
          label="Query results you run"
          detail="The final result set of queries you explicitly ask for."
        />
        <PrivacyRow
          icon={<Lock className="h-5 w-5 text-rose-500" />}
          kind="kept"
          label="Raw file contents"
          detail="Files are never uploaded. SQL runs locally via DuckDB."
        />
        <PrivacyRow
          icon={<Lock className="h-5 w-5 text-rose-500" />}
          kind="kept"
          label="Files outside watched folders"
          detail="The agent only reads what you explicitly share."
        />
      </div>

      <div className="mb-6 rounded-lg border border-purple-200 bg-purple-50 p-4 dark:border-purple-900 dark:bg-purple-950/30">
        <div className="mb-1 flex items-center gap-2 text-sm font-semibold text-purple-900 dark:text-purple-200">
          <Eye className="h-4 w-4" />
          Full transparency
        </div>
        <p className="text-xs text-purple-800 dark:text-purple-300">
          Every sync is logged to a local audit file. You can review exactly
          what was sent, when — from the Privacy tab.
        </p>
      </div>

      <div className="flex gap-2">
        <SecondaryButton onClick={onBack}>Back</SecondaryButton>
        <PrimaryButton onClick={onNext}>
          I understand <Check className="h-4 w-4" />
        </PrimaryButton>
      </div>
    </Card>
  );
}

// ─── Step 5: Done ───────────────────────────────────────────────────────────

function DoneStep() {
  const [finishing, setFinishing] = useState(false);
  const { setOnboardingComplete } = useAgentStore();
  const toast = useToast();

  const finish = async () => {
    setFinishing(true);
    try {
      await invoke('complete_first_run');
      setOnboardingComplete(true);
    } catch (err) {
      toast.error(`Couldn't save onboarding state: ${err}`);
      setFinishing(false);
    }
  };

  const openCloud = async () => {
    try {
      await invoke('open_in_sery_cloud');
    } catch (err) {
      console.error('Failed to open cloud:', err);
    }
    finish();
  };

  return (
    <Card>
      <div className="mb-6 flex items-center justify-center">
        <div className="flex h-16 w-16 items-center justify-center rounded-full bg-emerald-100 dark:bg-emerald-900/40">
          <Check className="h-8 w-8 text-emerald-600 dark:text-emerald-300" />
        </div>
      </div>

      <h1 className="mb-2 text-center text-3xl font-bold text-slate-900 dark:text-slate-50">
        You're all set
      </h1>
      <p className="mb-8 text-center text-slate-600 dark:text-slate-300">
        Head over to sery.ai to start asking questions in plain English.
        This app will keep running in your menu bar.
      </p>

      <div className="space-y-2">
        <PrimaryButton onClick={openCloud} disabled={finishing}>
          {finishing ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <>
              Open Sery <ArrowRight className="h-4 w-4" />
            </>
          )}
        </PrimaryButton>
        <SecondaryButton onClick={finish} disabled={finishing}>
          I'll explore the app first
        </SecondaryButton>
      </div>
    </Card>
  );
}

// ─── Shared primitives ──────────────────────────────────────────────────────

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="animate-slide_up rounded-2xl border border-slate-200 bg-white p-8 shadow-xl dark:border-slate-800 dark:bg-slate-900">
      {children}
    </div>
  );
}

function StepHeader({
  icon,
  title,
  subtitle,
}: {
  icon: React.ReactNode;
  title: string;
  subtitle: string;
}) {
  return (
    <div className="mb-6">
      <div className="mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-purple-100 text-purple-600 dark:bg-purple-900/40 dark:text-purple-300">
        {icon}
      </div>
      <h2 className="mb-1 text-2xl font-bold text-slate-900 dark:text-slate-50">
        {title}
      </h2>
      <p className="text-sm text-slate-600 dark:text-slate-300">{subtitle}</p>
    </div>
  );
}

function Bullet({
  icon,
  title,
  body,
}: {
  icon: React.ReactNode;
  title: string;
  body: string;
}) {
  return (
    <div className="flex items-start gap-3 rounded-lg bg-slate-50 p-3 dark:bg-slate-800/50">
      <div className="mt-0.5 shrink-0 text-purple-600 dark:text-purple-300">
        {icon}
      </div>
      <div>
        <div className="text-sm font-semibold text-slate-900 dark:text-slate-100">
          {title}
        </div>
        <div className="text-xs text-slate-600 dark:text-slate-400">{body}</div>
      </div>
    </div>
  );
}

function PrivacyRow({
  icon,
  kind,
  label,
  detail,
}: {
  icon: React.ReactNode;
  kind: 'sent' | 'kept';
  label: string;
  detail: string;
}) {
  return (
    <div className="flex items-start gap-3 rounded-lg border border-slate-200 p-3 dark:border-slate-800">
      <div className="mt-0.5 shrink-0">{icon}</div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-slate-900 dark:text-slate-100">
            {label}
          </span>
          <span
            className={`rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase ${
              kind === 'sent'
                ? 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300'
                : 'bg-rose-100 text-rose-700 dark:bg-rose-900/40 dark:text-rose-300'
            }`}
          >
            {kind === 'sent' ? 'Uploaded' : 'Stays local'}
          </span>
        </div>
        <div className="mt-0.5 text-xs text-slate-600 dark:text-slate-400">
          {detail}
        </div>
      </div>
    </div>
  );
}

function PrimaryButton({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex w-full items-center justify-center gap-2 rounded-lg bg-purple-600 px-4 py-2.5 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:cursor-not-allowed disabled:bg-slate-300 dark:disabled:bg-slate-700"
    >
      {children}
    </button>
  );
}

function SecondaryButton({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="flex items-center justify-center gap-2 rounded-lg border border-slate-300 bg-white px-4 py-2.5 text-sm font-medium text-slate-700 transition-colors hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
    >
      {children}
    </button>
  );
}

// ─── Helpers ────────────────────────────────────────────────────────────────

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
