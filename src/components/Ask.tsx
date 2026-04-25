// ROADMAP F7 — BYOK Ask UI (single-question, no streaming).
//
// The smallest first cut of "ask a question, get an answer with your own
// LLM key" inside sery-link. Architectural choices, all per SPEC_BYOK.md:
//
//   - Single question per turn (NOT a full chat surface). Transcript shows
//     past turns from this session for context but every turn is an
//     independent call.
//   - In-memory transcript only. No persistence in v0.5.0.
//   - Per-turn provenance badge — green "via your Anthropic key" makes the
//     privacy guarantee visible. If we ever route through Sery, that badge
//     would have to flip to purple. Until then, every turn here is BYOK.
//   - No tool-use / SQL generation. Just text in → text out. The catalog
//     is not consulted; the question must be self-contained. (Catalog
//     grounding lands in v0.6 once the privacy story for "what bytes did
//     we send to Anthropic?" is well-defined.)
//
// If no BYOK key is configured we route the user to Settings → Sync rather
// than rendering a broken input.

import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Link } from 'react-router-dom';
import {
  AlertCircle,
  ArrowRight,
  Cpu,
  Lock,
  Send,
  Settings as SettingsIcon,
  Sparkles,
} from 'lucide-react';

interface ByokStatus {
  configured: boolean;
  provider: string | null;
}

interface AskUsage {
  input_tokens: number;
  output_tokens: number;
}

interface AskResponse {
  text: string;
  stop_reason: string | null;
  usage: AskUsage | null;
}

interface Turn {
  id: number;
  question: string;
  answer: string;
  provider: string;
  usage: AskUsage | null;
  asked_at: string;
}

export function Ask() {
  const [status, setStatus] = useState<ByokStatus | null>(null);
  const [prompt, setPrompt] = useState('');
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [turns, setTurns] = useState<Turn[]>([]);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    invoke<ByokStatus>('get_byok_status')
      .then(setStatus)
      .catch((err) => {
        console.error('Failed to read BYOK status:', err);
        setStatus({ configured: false, provider: null });
      });
  }, []);

  useEffect(() => {
    if (status?.configured) {
      inputRef.current?.focus();
    }
  }, [status?.configured]);

  const ask = async () => {
    const trimmed = prompt.trim();
    if (!trimmed || asking || !status?.configured) return;
    setAsking(true);
    setError(null);
    try {
      const result = await invoke<AskResponse>('ask_byok', { prompt: trimmed });
      setTurns((prev) => [
        {
          id: Date.now(),
          question: trimmed,
          answer: result.text,
          provider: status.provider ?? 'anthropic',
          usage: result.usage,
          asked_at: new Date().toISOString(),
        },
        ...prev,
      ]);
      setPrompt('');
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    } finally {
      setAsking(false);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
      e.preventDefault();
      void ask();
    }
  };

  if (status === null) {
    return (
      <div className="mx-auto max-w-3xl px-6 py-12">
        <div className="h-32 animate-pulse rounded-2xl border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900" />
      </div>
    );
  }

  if (!status.configured) {
    return <NotConfigured />;
  }

  const providerLabel =
    status.provider === 'anthropic' ? 'Anthropic' : status.provider ?? 'your provider';

  return (
    <div className="mx-auto max-w-3xl px-6 py-8">
      <header className="mb-6">
        <div className="flex items-center gap-2">
          <Sparkles className="h-5 w-5 text-purple-500" />
          <h1 className="text-2xl font-semibold text-slate-900 dark:text-slate-100">
            Ask
          </h1>
        </div>
        <p className="mt-1 text-sm text-slate-500 dark:text-slate-400">
          One question, one answer — sent direct to {providerLabel} with
          your own key. Nothing routed through Sery.
        </p>
      </header>

      <div className="rounded-2xl border border-slate-200 bg-white shadow-sm dark:border-slate-800 dark:bg-slate-900">
        <textarea
          ref={inputRef}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Ask anything…"
          rows={4}
          disabled={asking}
          className="w-full resize-none rounded-t-2xl bg-transparent px-4 py-3 text-sm text-slate-900 placeholder:text-slate-400 focus:outline-none disabled:opacity-60 dark:text-slate-100"
        />
        <div className="flex items-center justify-between border-t border-slate-100 px-3 py-2 dark:border-slate-800">
          <PrivacyBadge provider={providerLabel} />
          <button
            onClick={() => void ask()}
            disabled={asking || prompt.trim().length === 0}
            className="inline-flex items-center gap-1.5 rounded-lg bg-purple-600 px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-purple-700 disabled:cursor-not-allowed disabled:opacity-50"
          >
            <Send className="h-3.5 w-3.5" />
            {asking ? 'Asking…' : 'Ask'}
            <span className="ml-1 text-xs opacity-70">⌘↵</span>
          </button>
        </div>
      </div>

      {error && (
        <div className="mt-4 flex items-start gap-2 rounded-lg border border-red-200 bg-red-50 p-3 text-sm text-red-700 dark:border-red-900/50 dark:bg-red-950/40 dark:text-red-300">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <div>
            <div className="font-medium">Couldn&apos;t reach {providerLabel}.</div>
            <div className="mt-0.5 text-xs opacity-80">{error}</div>
          </div>
        </div>
      )}

      {turns.length > 0 && (
        <div className="mt-8 space-y-4">
          <h2 className="text-xs font-medium uppercase tracking-wide text-slate-500 dark:text-slate-400">
            This session
          </h2>
          {turns.map((turn) => (
            <TurnCard key={turn.id} turn={turn} />
          ))}
        </div>
      )}

      {turns.length === 0 && !error && (
        <div className="mt-8 rounded-xl border border-dashed border-slate-200 bg-slate-50/50 p-6 text-center text-sm text-slate-500 dark:border-slate-800 dark:bg-slate-900/40 dark:text-slate-400">
          Your transcript will appear here. Single questions only — every
          turn is a fresh call to {providerLabel}, not a continuing
          conversation.
        </div>
      )}
    </div>
  );
}

function PrivacyBadge({ provider }: { provider: string }) {
  return (
    <div className="inline-flex items-center gap-1.5 rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-medium text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-300">
      <Lock className="h-3 w-3" />
      Direct to {provider}
    </div>
  );
}

function TurnCard({ turn }: { turn: Turn }) {
  const time = new Date(turn.asked_at).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
  });
  const providerLabel =
    turn.provider === 'anthropic' ? 'Anthropic' : turn.provider;

  return (
    <article className="rounded-xl border border-slate-200 bg-white p-4 dark:border-slate-800 dark:bg-slate-900">
      <header className="mb-2 flex items-center justify-between">
        <div className="text-xs uppercase tracking-wide text-slate-400 dark:text-slate-500">
          {time}
        </div>
        <PrivacyBadge provider={providerLabel} />
      </header>
      <div className="mb-3 whitespace-pre-wrap text-sm font-medium text-slate-900 dark:text-slate-100">
        {turn.question}
      </div>
      <div className="whitespace-pre-wrap text-sm leading-relaxed text-slate-700 dark:text-slate-300">
        {turn.answer}
      </div>
      {turn.usage && (
        <footer className="mt-3 flex items-center gap-3 border-t border-slate-100 pt-2 text-xs text-slate-500 dark:border-slate-800 dark:text-slate-400">
          <span className="inline-flex items-center gap-1">
            <Cpu className="h-3 w-3" />
            {turn.usage.input_tokens} in · {turn.usage.output_tokens} out
          </span>
          <span className="opacity-60">tokens billed to your account</span>
        </footer>
      )}
    </article>
  );
}

function NotConfigured() {
  return (
    <div className="mx-auto max-w-2xl px-6 py-16">
      <div className="rounded-2xl border border-slate-200 bg-white p-8 text-center dark:border-slate-800 dark:bg-slate-900">
        <div className="mx-auto mb-4 inline-flex h-12 w-12 items-center justify-center rounded-full bg-purple-100 text-purple-600 dark:bg-purple-900/30 dark:text-purple-300">
          <Sparkles className="h-6 w-6" />
        </div>
        <h1 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
          Add an AI provider to start asking
        </h1>
        <p className="mx-auto mt-2 max-w-md text-sm text-slate-500 dark:text-slate-400">
          Sery Link uses your own Anthropic key — your question goes
          straight from this app to Anthropic, never through our servers.
          We don&apos;t see the question or the answer.
        </p>
        <Link
          to="/settings"
          className="mt-6 inline-flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-medium text-white hover:bg-purple-700"
        >
          <SettingsIcon className="h-4 w-4" />
          Set up in Settings
          <ArrowRight className="h-4 w-4" />
        </Link>
        <p className="mt-4 text-xs text-slate-400 dark:text-slate-500">
          Don&apos;t have an Anthropic key?{' '}
          <a
            href="https://console.anthropic.com/settings/keys"
            target="_blank"
            rel="noreferrer"
            className="text-purple-600 underline-offset-4 hover:underline dark:text-purple-300"
          >
            Get one from the Anthropic console
          </a>
          .
        </p>
      </div>
    </div>
  );
}
