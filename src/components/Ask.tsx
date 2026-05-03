// Ask page placeholder.
//
// Pre-pivot, this was a BYOK-powered text-to-SQL UI. v0.5.3 → file-
// manager pivot moved AI to the cloud dashboard, so this surface
// becomes a thin "where to find AI now" page:
//
//   - Local-only / unauthenticated → "Connect to use AI" CTA pointing
//     at the StatusBar's Connect button (no link out — the user
//     can't use cloud AI without a workspace).
//   - Connected → "Open AI in dashboard" link to app.sery.ai/chat
//     so the user keeps a one-click path from desktop to the cloud
//     AI surface.
//
// We deliberately keep the /ask route + sidebar entry rather than
// removing them entirely so users with muscle memory for "I ask
// questions there" don't get confused. The page just redirects
// their attention.

import { ExternalLink, Link as LinkIcon, Sparkles } from 'lucide-react';
import { useAgentStore } from '../stores/agentStore';

export function Ask() {
  const { authenticated, config } = useAgentStore();
  const dashboardUrl = config?.cloud.web_url || 'https://app.sery.ai';
  const askUrl = `${dashboardUrl.replace(/\/$/, '')}/chat`;

  return (
    <div className="mx-auto max-w-2xl px-6 py-16">
      <div className="rounded-2xl border border-slate-200 bg-white p-8 text-center dark:border-slate-800 dark:bg-slate-900">
        <div className="mx-auto mb-4 inline-flex h-12 w-12 items-center justify-center rounded-full bg-purple-100 text-purple-600 dark:bg-purple-900/30 dark:text-purple-300">
          <Sparkles className="h-6 w-6" />
        </div>

        {authenticated ? (
          <>
            <h1 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
              AI lives in the dashboard now
            </h1>
            <p className="mx-auto mt-2 max-w-md text-sm text-slate-500 dark:text-slate-400">
              Ask questions about your indexed data — Sery&apos;s recipes
              and SQL agent run server-side and route queries to the
              right machine in your workspace.
            </p>
            <a
              href={askUrl}
              target="_blank"
              rel="noopener noreferrer"
              className="mt-6 inline-flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white hover:bg-purple-700"
            >
              Open AI in dashboard
              <ExternalLink className="h-4 w-4" />
            </a>
          </>
        ) : (
          <>
            <h1 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
              Connect to use AI
            </h1>
            <p className="mx-auto mt-2 max-w-md text-sm text-slate-500 dark:text-slate-400">
              AI happens in the dashboard now — Sery&apos;s recipes and
              text-to-SQL agent run server-side. Connect this machine
              to a workspace to enable them.
            </p>
            <div className="mx-auto mt-6 max-w-md rounded-lg border border-slate-200 bg-slate-50 p-4 text-left text-xs text-slate-600 dark:border-slate-700 dark:bg-slate-800/40 dark:text-slate-400">
              <p className="mb-2 font-semibold text-slate-700 dark:text-slate-200">
                <LinkIcon className="mr-1 inline h-3 w-3" />
                How to connect
              </p>
              <p>
                Click <strong>Connect</strong> in the status bar at the
                top of this window, paste a workspace key from the
                dashboard, and the AI surface unlocks for this account.
              </p>
            </div>
          </>
        )}

        <p className="mx-auto mt-6 max-w-md text-xs text-slate-400 dark:text-slate-500">
          Sery Link still does the work locally — it indexes, watches,
          and runs DuckDB queries against your files. The AI brain
          just lives elsewhere.
        </p>
      </div>
    </div>
  );
}
