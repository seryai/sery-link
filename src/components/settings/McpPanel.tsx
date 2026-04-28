// Settings → MCP panel.
//
// One row per watched folder. Each row has:
//   - "Expose via MCP" toggle (persists to config.watched_folders[i].mcp_enabled)
//   - When ON, expands to show ready-to-paste config snippets for each
//     known LLM client (Claude Desktop, Cursor, Continue), plus a
//     copy-to-clipboard button + the platform-specific path hint.
//
// We deliberately don't auto-write into LLM client configs — the
// failure mode of a borked claude_desktop_config.json is dire, and
// users own their AI tooling. Show the snippet, let them paste.

import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Check, Copy, Plug, Terminal } from 'lucide-react';
import type { AgentConfig, McpSnippet, WatchedFolder } from '../../types/events';
import { useToast } from '../Toast';

export function McpPanel({
  draft,
  setDraft,
}: {
  draft: AgentConfig;
  setDraft: (c: AgentConfig) => void;
}) {
  const localFolders = draft.watched_folders.filter(
    // Remote URLs (s3://, https://) can't be exposed via stdio MCP —
    // the MCP server reads files from disk, and remote URLs aren't
    // mounted as files. Filter to just local-disk paths.
    (f) => !f.path.includes('://'),
  );

  return (
    <div className="space-y-6">
      <header>
        <h2 className="mb-2 flex items-center gap-2 text-lg font-semibold text-slate-900 dark:text-slate-50">
          <Plug className="h-5 w-5 text-purple-600 dark:text-purple-400" />
          Model Context Protocol (MCP)
        </h2>
        <p className="text-sm text-slate-600 dark:text-slate-400">
          Expose your watched folders to AI clients (Claude Desktop, Cursor,
          Continue, …) so the LLM can search filenames, read schemas, sample
          rows, extract document text, and run SQL — all on your machine, no
          uploads. Toggle a folder on, copy the snippet into your AI
          client&apos;s config, and restart the client.
        </p>
        <p className="mt-2 text-xs text-slate-500 dark:text-slate-400">
          The MCP server runs inside Sery Link&apos;s own binary —{' '}
          <code className="text-purple-700 dark:text-purple-300">
            sery-link --mcp-stdio --root &lt;folder&gt;
          </code>
          . Your AI client spawns this process when needed and stops it when
          done. No long-running daemons, no open ports.
        </p>
      </header>

      {localFolders.length === 0 ? (
        <div className="rounded-lg border-2 border-dashed border-slate-300 p-8 text-center dark:border-slate-700">
          <p className="text-sm text-slate-600 dark:text-slate-400">
            No local folders watched yet. Add one on the Folders tab to enable
            MCP exposure.
          </p>
        </div>
      ) : (
        <div className="space-y-3">
          {localFolders.map((folder) => (
            <FolderMcpRow
              key={folder.path}
              folder={folder}
              onToggle={(enabled) => {
                // Update local draft so the UI flips immediately…
                const next = {
                  ...draft,
                  watched_folders: draft.watched_folders.map((f) =>
                    f.path === folder.path ? { ...f, mcp_enabled: enabled } : f,
                  ),
                };
                setDraft(next);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

// ─── One folder row ──────────────────────────────────────────────

function FolderMcpRow({
  folder,
  onToggle,
}: {
  folder: WatchedFolder;
  onToggle: (enabled: boolean) => void;
}) {
  const enabled = folder.mcp_enabled === true;
  const toast = useToast();
  const [snippets, setSnippets] = useState<McpSnippet[]>([]);
  const [loadingSnippets, setLoadingSnippets] = useState(false);

  const handleToggle = async () => {
    try {
      // …and persist via the Tauri command. We don't go through
      // save_config because mcp_enabled is the only thing changing
      // and save_config does a full restart_file_watcher dance we
      // don't need for this flag.
      await invoke('set_folder_mcp_enabled', {
        path: folder.path,
        enabled: !enabled,
      });
      onToggle(!enabled);
      // Lazy-load snippets the first time we enable.
      if (!enabled && snippets.length === 0) {
        await loadSnippets();
      }
    } catch (err) {
      toast.error(`Failed to toggle MCP: ${err}`);
    }
  };

  const loadSnippets = async () => {
    setLoadingSnippets(true);
    try {
      const fetched = await invoke<McpSnippet[]>('get_mcp_snippets', {
        folderPath: folder.path,
      });
      setSnippets(fetched);
    } catch (err) {
      toast.error(`Couldn't generate MCP snippets: ${err}`);
    } finally {
      setLoadingSnippets(false);
    }
  };

  return (
    <div
      className={`rounded-lg border ${
        enabled
          ? 'border-purple-300 bg-purple-50/50 dark:border-purple-700 dark:bg-purple-950/20'
          : 'border-slate-200 dark:border-slate-800'
      }`}
    >
      <div className="flex items-center justify-between gap-3 p-4">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <Terminal className="h-4 w-4 flex-shrink-0 text-slate-500 dark:text-slate-400" />
            <span
              className="truncate font-mono text-sm text-slate-900 dark:text-slate-100"
              title={folder.path}
            >
              {folder.path}
            </span>
          </div>
          {enabled && (
            <p className="mt-1 text-xs text-slate-500 dark:text-slate-400">
              Exposed via MCP. Add the snippet below to your AI client&apos;s
              config and restart it.
            </p>
          )}
        </div>
        <Toggle checked={enabled} onChange={handleToggle} />
      </div>

      {enabled && (
        <div className="border-t border-slate-200 p-4 dark:border-slate-800">
          {loadingSnippets ? (
            <p className="text-sm text-slate-500 dark:text-slate-400">
              Generating config snippets…
            </p>
          ) : snippets.length === 0 ? (
            <button
              onClick={loadSnippets}
              className="text-sm text-purple-600 hover:underline dark:text-purple-400"
            >
              Show config snippets
            </button>
          ) : (
            <div className="space-y-3">
              {snippets.map((s) => (
                <SnippetCard key={s.client} snippet={s} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Snippet card ───────────────────────────────────────────────

function SnippetCard({ snippet }: { snippet: McpSnippet }) {
  const [copied, setCopied] = useState(false);
  const toast = useToast();

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(snippet.config);
      setCopied(true);
      // Restore icon after a beat so the user knows the click registered.
      setTimeout(() => setCopied(false), 1500);
    } catch (err) {
      toast.error(`Couldn't copy to clipboard: ${err}`);
    }
  };

  return (
    <div className="rounded-md border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900">
      <div className="flex items-center justify-between gap-2 border-b border-slate-200 px-3 py-2 dark:border-slate-800">
        <div className="min-w-0">
          <div className="text-sm font-medium text-slate-900 dark:text-slate-100">
            {snippet.label}
          </div>
          <div
            className="truncate font-mono text-[10px] text-slate-500 dark:text-slate-400"
            title={snippet.config_path_hint}
          >
            {snippet.config_path_hint}
          </div>
        </div>
        <button
          onClick={copy}
          className="inline-flex items-center gap-1 rounded border border-slate-300 px-2 py-1 text-xs text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:text-slate-200 dark:hover:bg-slate-800"
        >
          {copied ? (
            <>
              <Check className="h-3 w-3 text-emerald-500" />
              Copied
            </>
          ) : (
            <>
              <Copy className="h-3 w-3" />
              Copy
            </>
          )}
        </button>
      </div>
      <pre className="overflow-x-auto p-3 text-xs leading-relaxed text-slate-800 dark:text-slate-200">
        <code>{snippet.config}</code>
      </pre>
    </div>
  );
}

// ─── Toggle switch (local copy to avoid extracting from Settings.tsx) ─

function Toggle({
  checked,
  onChange,
}: {
  checked: boolean;
  onChange: () => void;
}) {
  return (
    <button
      onClick={onChange}
      className={`relative inline-flex h-6 w-11 flex-shrink-0 items-center rounded-full transition-colors ${
        checked ? 'bg-purple-600' : 'bg-slate-300 dark:bg-slate-700'
      }`}
    >
      <span
        className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
          checked ? 'translate-x-6' : 'translate-x-1'
        }`}
      />
    </button>
  );
}
