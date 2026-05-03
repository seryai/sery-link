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
import { useAgentStore } from '../stores/agentStore';
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

type SqlOutcome =
  | {
      kind: 'rows';
      columns: string[];
      rows: string[][];
      total_rows: number;
      truncated: boolean;
    }
  | { kind: 'empty' }
  | { kind: 'insufficient_data'; reason: string }
  | { kind: 'error'; message: string }
  | { kind: 'no_sql_generated' };

interface SqlAttempt {
  sql: string;
  outcome: SqlOutcome;
}

interface AskResponse {
  text: string;
  stop_reason: string | null;
  usage: AskUsage | null;
  sql_attempt: SqlAttempt | null;
  considered_table_count: number;
}

interface Turn {
  id: number;
  question: string;
  answer: string;
  provider: string;
  usage: AskUsage | null;
  asked_at: string;
  sql_attempt: SqlAttempt | null;
  considered_table_count: number;
}

export function Ask() {
  const [status, setStatus] = useState<ByokStatus | null>(null);
  // Draft + conversation lifted to the store so navigating to
  // another tab doesn't wipe what the user typed or the answers
  // they were reading. `asking` and `error` stay local — they're
  // tied to an in-flight request and don't need to survive
  // navigation (the request itself completes in the background
  // either way).
  const prompt = useAgentStore((s) => s.askDraft);
  const setPrompt = useAgentStore((s) => s.setAskDraft);
  const turns = useAgentStore((s) => s.askTurns);
  const appendTurn = useAgentStore((s) => s.appendAskTurn);
  // clearAskTurns is exposed in the store for a future "Clear
  // conversation" affordance — not yet wired into the UI but kept
  // out of useState so the lift covers it once the button lands.
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
      // Store's appendAskTurn prepends — newest at the top, same
      // ordering the UI used before this lift.
      appendTurn({
        id: Date.now(),
        question: trimmed,
        answer: result.text,
        provider: status.provider ?? 'anthropic',
        usage: result.usage,
        asked_at: new Date().toISOString(),
        sql_attempt: result.sql_attempt ?? null,
        considered_table_count: result.considered_table_count ?? 0,
      });
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
          placeholder="Ask anything — or ask about your local files, e.g. 'which file mentions invoice Q3'"
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

      {turn.sql_attempt && <SqlAttemptPanel attempt={turn.sql_attempt} />}
      {turn.considered_table_count > 0 && (
        <div className="mt-2 text-[11px] text-slate-400 dark:text-slate-500">
          Considered {turn.considered_table_count} table
          {turn.considered_table_count === 1 ? '' : 's'} when answering.
        </div>
      )}

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

/** SQL trail + result table for one Ask turn. Renders inline in
 *  the TurnCard. The SQL block is collapsible so casual users see
 *  the answer + table; SQL-curious users open the disclosure. */
function SqlAttemptPanel({ attempt }: { attempt: SqlAttempt }) {
  const [showSql, setShowSql] = useState(false);
  const { outcome } = attempt;

  // No SQL attempted — nothing to show. The answer text already
  // explains what happened ("I don't have access to a table that
  // can answer this", etc).
  if (outcome.kind === 'no_sql_generated') return null;

  return (
    <div className="mt-3 border-t border-slate-100 pt-3 dark:border-slate-800">
      {/* Collapsible SQL — hidden by default, opens on click. */}
      {attempt.sql && (
        <div className="mb-2">
          <button
            type="button"
            onClick={() => setShowSql((v) => !v)}
            className="text-[11px] font-medium uppercase tracking-wide text-purple-600 hover:underline dark:text-purple-300"
          >
            {showSql ? '▾ Hide SQL' : '▸ Show SQL'}
          </button>
          {showSql && (
            <pre className="mt-1 max-h-48 overflow-auto rounded-md bg-slate-50 p-2 font-mono text-[11px] leading-relaxed text-slate-700 dark:bg-slate-950 dark:text-slate-300">
              {attempt.sql}
            </pre>
          )}
        </div>
      )}

      {outcome.kind === 'rows' && (
        <ResultTable
          columns={outcome.columns}
          rows={outcome.rows}
          totalRows={outcome.total_rows}
          truncated={outcome.truncated}
        />
      )}
      {outcome.kind === 'empty' && (
        <p className="text-[11px] italic text-slate-500 dark:text-slate-400">
          (query returned no rows)
        </p>
      )}
      {outcome.kind === 'error' && (
        <p className="text-[11px] text-rose-600 dark:text-rose-300">
          SQL error: {outcome.message}
        </p>
      )}
      {outcome.kind === 'insufficient_data' && (
        <p className="text-[11px] italic text-slate-500 dark:text-slate-400">
          Insufficient data: {outcome.reason}
        </p>
      )}
    </div>
  );
}

/** Compact table render for the SQL result. Header row + striped
 *  body. Caps cell width via CSS so a wide column doesn't blow
 *  the layout. */
function ResultTable({
  columns,
  rows,
  totalRows,
  truncated,
}: {
  columns: string[];
  rows: string[][];
  totalRows: number;
  truncated: boolean;
}) {
  return (
    <div className="overflow-x-auto rounded-md border border-slate-200 dark:border-slate-700">
      <table className="w-full text-[11px]">
        <thead className="bg-slate-50 dark:bg-slate-800/60">
          <tr>
            {columns.map((c) => (
              <th
                key={c}
                className="border-b border-slate-200 px-2 py-1 text-left font-semibold text-slate-700 dark:border-slate-700 dark:text-slate-200"
              >
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="even:bg-slate-50/40 dark:even:bg-slate-900/40">
              {row.map((cell, j) => (
                <td
                  key={j}
                  className="max-w-xs truncate px-2 py-1 font-mono text-slate-600 dark:text-slate-300"
                  title={cell}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
      <div className="border-t border-slate-200 bg-slate-50 px-2 py-1 text-[10px] text-slate-500 dark:border-slate-700 dark:bg-slate-800/60 dark:text-slate-400">
        {truncated
          ? `Showing ${rows.length} of ${totalRows.toLocaleString()} rows`
          : `${totalRows.toLocaleString()} row${totalRows === 1 ? '' : 's'}`}
      </div>
    </div>
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
