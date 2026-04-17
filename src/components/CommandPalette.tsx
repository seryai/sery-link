// Command Palette — universal keyboard-first command launcher
// Press Cmd+K to open, fuzzy search for commands, navigate with arrow keys.
//
// Inspired by Obsidian's command palette and modern code editors.

import { useEffect, useState, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import {
  BarChart3,
  Search,
  Folder,
  FolderOpen,
  Laptop,
  Plus,
  RefreshCw,
  Settings as SettingsIcon,
  Shield,
  Sparkles,
  Database,
  Trash2,
  X,
} from 'lucide-react';
import type { AgentConfig } from '../types/events';
import { useMetadataCache, type CachedDataset } from '../hooks/useMetadataCache';

interface Command {
  id: string;
  label: string;
  description?: string;
  icon: React.ReactNode;
  keywords: string[];
  action: () => void | Promise<void>;
  section: 'navigation' | 'folders' | 'actions' | 'datasets' | 'recent';
}

interface CommandPaletteProps {
  config: AgentConfig | null;
  workspaceId: string | null;
  onNavigate: (tab: 'folders' | 'analytics' | 'results' | 'fleet' | 'privacy' | 'settings') => void;
  onAddFolder: () => void;
  onRescanFolder?: (path: string) => void;
  onRemoveFolder?: (path: string) => void;
}

export function CommandPalette({
  config,
  workspaceId,
  onNavigate,
  onAddFolder,
  onRescanFolder,
  onRemoveFolder,
}: CommandPaletteProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [datasetResults, setDatasetResults] = useState<CachedDataset[]>([]);

  const cache = useMetadataCache(workspaceId);

  // Build command list from available actions
  const commands = useMemo<Command[]>(() => {
    const baseCommands: Command[] = [
      // Navigation
      {
        id: 'nav-folders',
        label: 'Go to Folders',
        icon: <Folder className="h-4 w-4" />,
        keywords: ['folders', 'data', 'navigate'],
        action: () => {
          onNavigate('folders');
          setIsOpen(false);
        },
        section: 'navigation',
      },
      {
        id: 'nav-analytics',
        label: 'Go to Analytics',
        icon: <Sparkles className="h-4 w-4" />,
        keywords: ['analytics', 'recipes', 'query', 'sql', 'navigate'],
        action: () => {
          onNavigate('analytics');
          setIsOpen(false);
        },
        section: 'navigation',
      },
      {
        id: 'nav-results',
        label: 'Go to Results',
        icon: <BarChart3 className="h-4 w-4" />,
        keywords: ['results', 'history', 'queries', 'navigate'],
        action: () => {
          onNavigate('results');
          setIsOpen(false);
        },
        section: 'navigation',
      },
      {
        id: 'nav-fleet',
        label: 'Go to Fleet',
        description: 'See all the machines connected to this workspace',
        icon: <Laptop className="h-4 w-4" />,
        keywords: ['fleet', 'machines', 'devices', 'agents', 'pair', 'navigate'],
        action: () => {
          onNavigate('fleet');
          setIsOpen(false);
        },
        section: 'navigation',
      },
      {
        id: 'add-machine',
        label: 'Add Another Machine',
        description: 'Pair a second machine via QR code or pair code',
        icon: <Plus className="h-4 w-4" />,
        keywords: ['pair', 'add machine', 'connect machine', 'fleet', 'qr'],
        action: () => {
          // Route through /fleet — the FleetView page has the "Add a
          // machine" button which opens AddMachineModal. Avoids duplicating
          // the modal-trigger wiring in two places. (The tray menu uses
          // a Tauri event for the same effect.)
          onNavigate('fleet');
          setIsOpen(false);
        },
        section: 'actions',
      },
      {
        id: 'nav-privacy',
        label: 'Go to Privacy',
        icon: <Shield className="h-4 w-4" />,
        keywords: ['privacy', 'security', 'navigate'],
        action: () => {
          onNavigate('privacy');
          setIsOpen(false);
        },
        section: 'navigation',
      },
      {
        id: 'nav-settings',
        label: 'Go to Settings',
        icon: <SettingsIcon className="h-4 w-4" />,
        keywords: ['settings', 'config', 'preferences', 'navigate'],
        action: () => {
          onNavigate('settings');
          setIsOpen(false);
        },
        section: 'navigation',
      },

      // Folder actions
      {
        id: 'add-folder',
        label: 'Watch New Folder',
        description: 'Add a folder to watch for data files',
        icon: <FolderOpen className="h-4 w-4" />,
        keywords: ['add', 'watch', 'folder', 'new'],
        action: () => {
          onAddFolder();
          setIsOpen(false);
        },
        section: 'actions',
      },
    ];

    // Add per-folder commands (rescan, remove)
    if (config?.watched_folders) {
      config.watched_folders.forEach((folder) => {
        const folderName = folderBasename(folder.path);

        if (onRescanFolder) {
          baseCommands.push({
            id: `rescan-${folder.path}`,
            label: `Rescan ${folderName}`,
            description: folder.path,
            icon: <RefreshCw className="h-4 w-4" />,
            keywords: ['rescan', 'refresh', 'sync', folderName.toLowerCase()],
            action: () => {
              onRescanFolder(folder.path);
              setIsOpen(false);
            },
            section: 'folders',
          });
        }

        if (onRemoveFolder) {
          baseCommands.push({
            id: `remove-${folder.path}`,
            label: `Remove ${folderName}`,
            description: folder.path,
            icon: <Trash2 className="h-4 w-4" />,
            keywords: ['remove', 'delete', 'unwatch', folderName.toLowerCase()],
            action: () => {
              onRemoveFolder(folder.path);
              setIsOpen(false);
            },
            section: 'folders',
          });
        }
      });
    }

    return baseCommands;
  }, [config, onNavigate, onAddFolder, onRescanFolder, onRemoveFolder]);

  // Search datasets from cache when query changes
  useEffect(() => {
    if (!query.trim() || !workspaceId) {
      setDatasetResults([]);
      return;
    }

    // Debounce dataset search (only search if query is 2+ chars)
    if (query.length < 2) {
      setDatasetResults([]);
      return;
    }

    cache.search(query, 10).then((results) => {
      setDatasetResults(results.map((r) => r.dataset));
    });
  }, [query, workspaceId, cache]);

  // Build dataset commands from search results
  const datasetCommands = useMemo<Command[]>(() => {
    return datasetResults.map((dataset) => ({
      id: `dataset-${dataset.id}`,
      label: dataset.name,
      description: dataset.path,
      icon: <Database className="h-4 w-4" />,
      keywords: [dataset.name.toLowerCase(), dataset.path.toLowerCase()],
      action: async () => {
        // Copy path to clipboard
        await invoke('reveal_in_finder', { path: dataset.path });
        setIsOpen(false);
      },
      section: 'datasets' as const,
    }));
  }, [datasetResults]);

  // Fuzzy filter commands based on query
  const filteredCommands = useMemo(() => {
    if (!query.trim()) return commands;

    const lowerQuery = query.toLowerCase();
    return commands.filter((cmd) => {
      const searchText = [
        cmd.label,
        cmd.description || '',
        ...cmd.keywords,
      ].join(' ').toLowerCase();
      return searchText.includes(lowerQuery);
    });
  }, [commands, query]);

  // Combine filtered commands with dataset commands
  const allCommands = useMemo(() => {
    return [...filteredCommands, ...datasetCommands];
  }, [filteredCommands, datasetCommands]);

  // Reset selection when combined list changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [allCommands]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Open with Cmd+K (Mac) or Ctrl+K (Windows/Linux)
      if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
        e.preventDefault();
        setIsOpen(true);
        setQuery('');
        return;
      }

      if (!isOpen) return;

      // Navigation within palette
      switch (e.key) {
        case 'Escape':
          e.preventDefault();
          setIsOpen(false);
          setQuery('');
          break;

        case 'ArrowDown':
          e.preventDefault();
          setSelectedIndex((i) =>
            i < allCommands.length - 1 ? i + 1 : i,
          );
          break;

        case 'ArrowUp':
          e.preventDefault();
          setSelectedIndex((i) => (i > 0 ? i - 1 : i));
          break;

        case 'Enter':
          e.preventDefault();
          if (allCommands[selectedIndex]) {
            allCommands[selectedIndex].action();
          }
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen, allCommands, selectedIndex]);

  if (!isOpen) return null;

  const sections = Array.from(
    new Set(allCommands.map((c) => c.section)),
  ) as Array<'navigation' | 'folders' | 'actions' | 'datasets' | 'recent'>;

  const sectionLabels: Record<string, string> = {
    navigation: 'Navigation',
    folders: 'Folders',
    actions: 'Actions',
    datasets: 'Datasets',
    recent: 'Recent',
  };

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 pt-32">
      <div className="relative mx-4 w-full max-w-2xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl dark:border-slate-800 dark:bg-slate-900">
        {/* Search Input */}
        <div className="flex items-center gap-3 border-b border-slate-200 px-4 py-3 dark:border-slate-800">
          <Search className="h-5 w-5 text-slate-400" />
          <input
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Type a command or search..."
            className="flex-1 bg-transparent text-sm text-slate-900 placeholder-slate-400 outline-none dark:text-slate-50 dark:placeholder-slate-500"
            autoFocus
          />
          <button
            onClick={() => {
              setIsOpen(false);
              setQuery('');
            }}
            className="rounded-lg p-1.5 text-slate-400 transition-colors hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-800 dark:hover:text-slate-300"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Command List */}
        <div className="max-h-96 overflow-y-auto">
          {allCommands.length === 0 ? (
            <div className="px-4 py-8 text-center text-sm text-slate-500 dark:text-slate-400">
              No commands or datasets found for "{query}"
            </div>
          ) : (
            sections.map((section) => {
              const sectionCommands = allCommands.filter(
                (c) => c.section === section,
              );
              if (sectionCommands.length === 0) return null;

              return (
                <div key={section}>
                  <div className="px-4 py-2 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                    {sectionLabels[section]}
                  </div>
                  {sectionCommands.map((cmd) => {
                    const globalIndex = allCommands.indexOf(cmd);
                    const isSelected = globalIndex === selectedIndex;

                    return (
                      <button
                        key={cmd.id}
                        onClick={() => cmd.action()}
                        onMouseEnter={() => setSelectedIndex(globalIndex)}
                        className={`flex w-full items-center gap-3 px-4 py-2.5 text-left transition-colors ${
                          isSelected
                            ? 'bg-purple-50 text-purple-700 dark:bg-purple-900/30 dark:text-purple-200'
                            : 'text-slate-700 hover:bg-slate-50 dark:text-slate-300 dark:hover:bg-slate-800/50'
                        }`}
                      >
                        <div
                          className={`flex h-8 w-8 items-center justify-center rounded-lg ${
                            isSelected
                              ? 'bg-purple-100 text-purple-600 dark:bg-purple-900/60 dark:text-purple-300'
                              : 'bg-slate-100 text-slate-600 dark:bg-slate-800 dark:text-slate-400'
                          }`}
                        >
                          {cmd.icon}
                        </div>
                        <div className="min-w-0 flex-1">
                          <div className="text-sm font-medium">{cmd.label}</div>
                          {cmd.description && (
                            <div
                              className={`truncate text-xs ${
                                isSelected
                                  ? 'text-purple-600/80 dark:text-purple-300/80'
                                  : 'text-slate-500 dark:text-slate-400'
                              }`}
                            >
                              {cmd.description}
                            </div>
                          )}
                        </div>
                      </button>
                    );
                  })}
                </div>
              );
            })
          )}
        </div>

        {/* Footer */}
        <div className="border-t border-slate-200 bg-slate-50 px-4 py-2.5 dark:border-slate-800 dark:bg-slate-800/50">
          <div className="flex items-center gap-4 text-xs text-slate-500 dark:text-slate-400">
            <span className="flex items-center gap-1.5">
              <kbd className="rounded bg-slate-200 px-1.5 py-0.5 font-mono dark:bg-slate-700">
                ↵
              </kbd>{' '}
              to select
            </span>
            <span className="flex items-center gap-1.5">
              <kbd className="rounded bg-slate-200 px-1.5 py-0.5 font-mono dark:bg-slate-700">
                ↑↓
              </kbd>{' '}
              to navigate
            </span>
            <span className="flex items-center gap-1.5">
              <kbd className="rounded bg-slate-200 px-1.5 py-0.5 font-mono dark:bg-slate-700">
                esc
              </kbd>{' '}
              to close
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}

// ─── Helpers ────────────────────────────────────────────────────────────────

function folderBasename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] || path;
}
