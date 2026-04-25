// ROADMAP F11 — cross-machine recipe sync (sery-link side).
//
// Reads the workspace recipe library from /v1/agent/workspace-recipes
// using the agent token and surfaces it on every machine in the
// workspace. The "save once on machine A, see it on machine B" half
// of the recipe story.
//
// Running a recipe opens app.sery.ai/chat?question=... in the user's
// browser. Sery Link doesn't have a chat surface today (and isn't going
// to grow one without an explicit decision — the desktop is for local
// search + file profiles; conversational AI lives on the dashboard).
//
// Read-only from this side: saving still happens in app-dashboard chat
// where the user is JWT-authenticated. No edit / delete affordance here
// either — those land in the dashboard /recipes page.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  AlertCircle,
  Clock,
  ExternalLink,
  Play,
  RefreshCw,
  Sparkles,
} from 'lucide-react';

interface WorkspaceRecipe {
  id: string;
  workspace_id: string;
  created_by: string | null;
  name: string;
  question: string;
  source_message_id: string | null;
  created_at: string;
  last_run_at: string | null;
  run_count: number;
}

export function Recipes() {
  const [recipes, setRecipes] = useState<WorkspaceRecipe[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);

  const load = async (initial = false) => {
    if (initial) setLoading(true);
    else setRefreshing(true);
    try {
      const result = await invoke<WorkspaceRecipe[]>('fetch_workspace_recipes');
      setRecipes(result ?? []);
      setError(null);
    } catch (err) {
      setError(typeof err === 'string' ? err : String(err));
    } finally {
      if (initial) setLoading(false);
      else setRefreshing(false);
    }
  };

  useEffect(() => {
    void load(true);
  }, []);

  const runRecipe = async (recipe: WorkspaceRecipe) => {
    try {
      await invoke('open_recipe_in_browser', { question: recipe.question });
    } catch (err) {
      alert(`Failed to open recipe: ${err}`);
    }
  };

  return (
    <div className="mx-auto max-w-4xl px-6 py-8">
      <Header onRefresh={() => load(false)} refreshing={refreshing} />

      {loading && (
        <div className="mt-6 space-y-3">
          {[...Array(3)].map((_, i) => (
            <div
              key={i}
              className="h-20 rounded-xl border border-slate-200 bg-white animate-pulse dark:border-slate-800 dark:bg-slate-900"
            />
          ))}
        </div>
      )}

      {!loading && error && <ErrorPanel error={error} onRetry={() => load(false)} />}

      {!loading && !error && recipes.length === 0 && <EmptyState />}

      {!loading && !error && recipes.length > 0 && (
        <ul className="mt-6 space-y-3">
          {recipes.map((recipe) => (
            <RecipeRow
              key={recipe.id}
              recipe={recipe}
              onRun={() => runRecipe(recipe)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function Header({
  onRefresh,
  refreshing,
}: {
  onRefresh: () => void;
  refreshing: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4">
      <div>
        <h1 className="flex items-center gap-3 text-2xl font-semibold text-slate-900 dark:text-slate-100">
          <Sparkles className="h-6 w-6 text-purple-600 dark:text-purple-400" strokeWidth={1.5} />
          Workspace recipes
        </h1>
        <p className="mt-2 text-sm text-slate-600 dark:text-slate-400">
          Saved questions from your workspace. Save once anywhere — see them
          on every machine you own. Run opens the question in your
          browser&apos;s chat surface.
        </p>
      </div>
      <button
        onClick={onRefresh}
        disabled={refreshing}
        className="flex items-center gap-1.5 rounded-lg border border-slate-200 bg-white px-3 py-1.5 text-sm text-slate-700 hover:bg-slate-50 disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-300 dark:hover:bg-slate-800"
        title="Refresh from workspace"
      >
        <RefreshCw
          className={`h-3.5 w-3.5 ${refreshing ? 'animate-spin' : ''}`}
          strokeWidth={1.75}
        />
        Refresh
      </button>
    </div>
  );
}

function RecipeRow({
  recipe,
  onRun,
}: {
  recipe: WorkspaceRecipe;
  onRun: () => void;
}) {
  return (
    <li className="rounded-xl border border-slate-200 bg-white p-4 transition-colors hover:border-purple-300 dark:border-slate-800 dark:bg-slate-900 dark:hover:border-purple-700">
      <div className="flex items-start gap-4">
        <div className="flex h-10 w-10 flex-shrink-0 items-center justify-center rounded-lg bg-purple-50 dark:bg-purple-900/20">
          <Sparkles
            className="h-5 w-5 text-purple-600 dark:text-purple-400"
            strokeWidth={1.5}
          />
        </div>
        <div className="min-w-0 flex-1">
          <h3 className="truncate text-base font-semibold text-slate-900 dark:text-slate-100">
            {recipe.name}
          </h3>
          <p className="mt-1 line-clamp-2 text-sm text-slate-600 dark:text-slate-400">
            {recipe.question}
          </p>
          <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-slate-500 dark:text-slate-400">
            <span className="inline-flex items-center gap-1">
              <Play className="h-3 w-3" strokeWidth={1.75} />
              {recipe.run_count === 0
                ? 'Never run'
                : `Run ${recipe.run_count} ${recipe.run_count === 1 ? 'time' : 'times'}`}
            </span>
            {recipe.last_run_at && (
              <>
                <span className="text-slate-300 dark:text-slate-600">•</span>
                <span className="inline-flex items-center gap-1">
                  <Clock className="h-3 w-3" strokeWidth={1.75} />
                  Last run {formatRelativeTime(recipe.last_run_at)}
                </span>
              </>
            )}
          </div>
        </div>
        <button
          onClick={onRun}
          className="inline-flex flex-shrink-0 items-center gap-1.5 rounded-lg bg-purple-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-purple-700"
          title="Open this recipe in app.sery.ai chat"
        >
          <ExternalLink className="h-3.5 w-3.5" strokeWidth={1.75} />
          Run
        </button>
      </div>
    </li>
  );
}

function EmptyState() {
  return (
    <div className="mt-10 rounded-xl border-2 border-dashed border-slate-200 bg-white p-10 text-center dark:border-slate-800 dark:bg-slate-900">
      <Sparkles
        className="mx-auto mb-3 h-12 w-12 text-slate-300 dark:text-slate-600"
        strokeWidth={1.5}
      />
      <p className="text-sm font-medium text-slate-900 dark:text-slate-100">
        No recipes in your workspace yet
      </p>
      <p className="mx-auto mt-2 max-w-sm text-xs text-slate-500 dark:text-slate-400">
        Save a question as a recipe from{' '}
        <span className="font-medium text-slate-700 dark:text-slate-300">
          app.sery.ai chat
        </span>{' '}
        — it&apos;ll appear here on every machine in your workspace.
      </p>
    </div>
  );
}

function ErrorPanel({ error, onRetry }: { error: string; onRetry: () => void }) {
  return (
    <div className="mt-6 rounded-xl border border-amber-200 bg-amber-50 p-4 dark:border-amber-900/40 dark:bg-amber-900/10">
      <div className="flex items-start gap-3">
        <AlertCircle
          className="mt-0.5 h-4 w-4 flex-shrink-0 text-amber-600 dark:text-amber-400"
          strokeWidth={1.75}
        />
        <div className="flex-1">
          <p className="text-sm font-medium text-amber-900 dark:text-amber-200">
            Could not load recipes
          </p>
          <p className="mt-1 text-xs text-amber-800 dark:text-amber-300">{error}</p>
          <p className="mt-2 text-xs text-amber-700 dark:text-amber-400">
            If you&apos;re in Local-only mode (Settings → Sync), recipes
            sync is paused — toggle network mode back on to refresh.
          </p>
        </div>
        <button
          onClick={onRetry}
          className="inline-flex flex-shrink-0 items-center gap-1 rounded border border-amber-300 bg-white px-2 py-1 text-xs text-amber-900 hover:bg-amber-100 dark:border-amber-800 dark:bg-transparent dark:text-amber-200 dark:hover:bg-amber-900/20"
        >
          <RefreshCw className="h-3 w-3" strokeWidth={1.75} />
          Retry
        </button>
      </div>
    </div>
  );
}

function formatRelativeTime(iso: string): string {
  const date = new Date(iso);
  const diffMs = Date.now() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return 'just now';
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 30) return `${diffDay}d ago`;
  return date.toLocaleDateString();
}
