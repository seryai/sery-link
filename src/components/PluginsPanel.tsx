// Enhanced Plugins Panel with execution capabilities
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { Puzzle, RefreshCw, Trash2, Upload, Zap } from 'lucide-react';
import { useToast } from './Toast';

interface PluginFunction {
  name: string;
  description: string;
  parameters: Array<{ name: string; type: string }>;
  returns: string;
  requires_file: boolean;
}

interface Plugin {
  manifest: {
    id: string;
    name: string;
    version: string;
    author: string;
    description: string;
    capabilities: string[];
    permissions: string[];
    homepage?: string;
    functions?: PluginFunction[];
  };
  enabled: boolean;
}

interface ExecutionResult {
  plugin: string;
  file: string;
  size: number;
  function: string;
  result: number;
}

function Panel({ children }: { children: React.ReactNode }) {
  return <div className="space-y-6">{children}</div>;
}

export function PluginsPanel() {
  const toast = useToast();
  const [plugins, setPlugins] = useState<Plugin[]>([]);
  const [loading, setLoading] = useState(true);
  const [expandedPlugin, setExpandedPlugin] = useState<string | null>(null);
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [selectedFunction, setSelectedFunction] = useState<Record<string, string>>({});
  const [executing, setExecuting] = useState(false);
  const [executionResult, setExecutionResult] = useState<ExecutionResult | null>(null);

  useEffect(() => {
    loadPlugins();
  }, []);

  const loadPlugins = async () => {
    try {
      const list = await invoke<[[Plugin['manifest'], boolean]]>('list_plugins');
      setPlugins(
        list.map(([manifest, enabled]) => ({ manifest, enabled }))
      );
    } catch (err) {
      toast.error(`Failed to load plugins: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const togglePlugin = async (pluginId: string, currentlyEnabled: boolean) => {
    try {
      if (currentlyEnabled) {
        await invoke('disable_plugin', { pluginId });
        toast.info('Plugin disabled');
      } else {
        await invoke('enable_plugin', { pluginId });
        toast.success('Plugin enabled');
      }
      await loadPlugins();
    } catch (err) {
      toast.error(`Failed to toggle plugin: ${err}`);
    }
  };

  const uninstallPlugin = async (pluginId: string, name: string) => {
    if (
      !window.confirm(`Uninstall "${name}"? This will remove the plugin from disk.`)
    ) {
      return;
    }
    try {
      await invoke('uninstall_plugin', { pluginId });
      toast.success('Plugin uninstalled');
      await loadPlugins();
    } catch (err) {
      toast.error(`Failed to uninstall plugin: ${err}`);
    }
  };

  const selectFile = async () => {
    try {
      const file = await openDialog({
        multiple: false,
        filters: [
          { name: 'CSV Files', extensions: ['csv'] },
          { name: 'All Files', extensions: ['*'] },
        ],
      });
      if (file) {
        setSelectedFile(file);
        setExecutionResult(null);
      }
    } catch (err) {
      toast.error(`Failed to select file: ${err}`);
    }
  };

  const executePlugin = async (
    pluginId: string,
    functionName: string,
    requiresFile: boolean
  ) => {
    if (requiresFile && !selectedFile) {
      toast.error('Please select a file first');
      return;
    }

    setExecuting(true);
    setExecutionResult(null);

    try {
      // Load plugin into runtime if not already loaded
      await invoke('load_plugin_into_runtime', { pluginId });

      // Execute the plugin function
      const resultJson = await invoke<string>('execute_plugin_with_file', {
        pluginId,
        filePath: selectedFile || '',
        functionName,
      });

      const result: ExecutionResult = JSON.parse(resultJson);
      setExecutionResult(result);
      toast.success('Plugin executed successfully');
    } catch (err) {
      toast.error(`Plugin execution failed: ${err}`);
    } finally {
      setExecuting(false);
    }
  };

  const formatResult = (result: ExecutionResult): string => {
    // Unpack the result based on the CSV parser format:
    // (valid << 16) | (row_count << 8) | column_count
    const value = result.result;
    const valid = (value >> 16) & 0xFF;
    const rowCount = (value >> 8) & 0xFF;
    const columnCount = value & 0xFF;

    return `${columnCount} columns, ${rowCount} rows, ${valid ? 'valid' : 'invalid'} CSV`;
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <RefreshCw className="h-6 w-6 animate-spin text-slate-400" />
      </div>
    );
  }

  if (plugins.length === 0) {
    return (
      <Panel>
        <div className="rounded-lg border-2 border-dashed border-slate-200 p-12 text-center dark:border-slate-800">
          <Puzzle className="mx-auto mb-4 h-12 w-12 text-slate-400 dark:text-slate-600" />
          <h3 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-100">
            No plugins installed
          </h3>
          <p className="text-sm text-slate-500 dark:text-slate-400">
            Plugins extend Sery Link with custom data sources, transformations,
            and visualizations.
          </p>
          <p className="mt-2 text-sm text-slate-500 dark:text-slate-400">
            Install plugins by placing them in{' '}
            <code className="rounded bg-slate-100 px-1 text-xs dark:bg-slate-900">
              ~/.sery/plugins/
            </code>
          </p>
        </div>
      </Panel>
    );
  }

  return (
    <Panel>
      <div className="space-y-3">
        {plugins.map((plugin) => {
          const isExpanded = expandedPlugin === plugin.manifest.id;

          return (
            <div
              key={plugin.manifest.id}
              className="rounded-lg border border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900"
            >
              <div className="p-4">
                <div className="flex items-start justify-between">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <Puzzle className="h-5 w-5 shrink-0 text-purple-600 dark:text-purple-400" />
                      <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                        {plugin.manifest.name}
                      </h3>
                      <span className="text-xs text-slate-500 dark:text-slate-400">
                        v{plugin.manifest.version}
                      </span>
                    </div>
                    <p className="mt-1 text-xs text-slate-600 dark:text-slate-400">
                      {plugin.manifest.description}
                    </p>
                    <div className="mt-2 flex flex-wrap gap-1">
                      {plugin.manifest.capabilities.map((cap) => (
                        <span
                          key={cap}
                          className="rounded bg-purple-100 px-2 py-0.5 text-xs font-medium text-purple-700 dark:bg-purple-900/40 dark:text-purple-300"
                        >
                          {cap}
                        </span>
                      ))}
                    </div>
                    <div className="mt-1 text-xs text-slate-500 dark:text-slate-400">
                      by {plugin.manifest.author}
                      {plugin.manifest.homepage && (
                        <>
                          {' '}
                          •{' '}
                          <a
                            href={plugin.manifest.homepage}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="hover:underline"
                          >
                            website
                          </a>
                        </>
                      )}
                    </div>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {plugin.enabled && (
                      <button
                        onClick={() =>
                          setExpandedPlugin(isExpanded ? null : plugin.manifest.id)
                        }
                        className="rounded-lg bg-purple-100 px-3 py-1.5 text-xs font-medium text-purple-700 transition-colors hover:bg-purple-200 dark:bg-purple-900/40 dark:text-purple-300 dark:hover:bg-purple-900/60"
                      >
                        {isExpanded ? 'Close' : 'Run'}
                      </button>
                    )}
                    <button
                      onClick={() =>
                        togglePlugin(plugin.manifest.id, plugin.enabled)
                      }
                      role="switch"
                      aria-checked={plugin.enabled}
                      className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${
                        plugin.enabled
                          ? 'bg-purple-600'
                          : 'bg-slate-300 dark:bg-slate-700'
                      }`}
                    >
                      <span
                        className={`inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform ${
                          plugin.enabled ? 'translate-x-6' : 'translate-x-1'
                        }`}
                      />
                    </button>
                    <button
                      onClick={() =>
                        uninstallPlugin(plugin.manifest.id, plugin.manifest.name)
                      }
                      className="rounded-lg p-2 text-slate-400 transition-colors hover:bg-slate-100 hover:text-rose-600 dark:hover:bg-slate-800 dark:hover:text-rose-400"
                      title="Uninstall plugin"
                    >
                      <Trash2 className="h-4 w-4" />
                    </button>
                  </div>
                </div>

                {/* Execution panel */}
                {isExpanded && (() => {
                  const functions = plugin.manifest.functions || [];
                  const currentFunction =
                    selectedFunction[plugin.manifest.id] || functions[0]?.name;
                  const funcMetadata = functions.find((f) => f.name === currentFunction);

                  return (
                    <div className="mt-4 space-y-3 rounded-lg border border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-800/50">
                      {/* Function selector */}
                      {functions.length > 0 && (
                        <div>
                          <label className="mb-1 block text-xs font-medium text-slate-700 dark:text-slate-300">
                            Function
                          </label>
                          <select
                            value={currentFunction}
                            onChange={(e) =>
                              setSelectedFunction({
                                ...selectedFunction,
                                [plugin.manifest.id]: e.target.value,
                              })
                            }
                            className="w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-xs text-slate-900 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-100"
                          >
                            {functions.map((func) => (
                              <option key={func.name} value={func.name}>
                                {func.name} - {func.description}
                              </option>
                            ))}
                          </select>
                        </div>
                      )}

                      {/* File picker (only for functions that require files) */}
                      {funcMetadata?.requires_file && (
                        <div className="flex items-center gap-2">
                          <button
                            onClick={selectFile}
                            className="flex items-center gap-2 rounded-lg border border-slate-300 bg-white px-3 py-2 text-xs font-medium text-slate-700 transition-colors hover:bg-slate-50 dark:border-slate-600 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700"
                          >
                            <Upload className="h-3.5 w-3.5" />
                            {selectedFile ? 'Change file' : 'Select file'}
                          </button>
                          {selectedFile && (
                            <span className="truncate text-xs text-slate-600 dark:text-slate-400">
                              {selectedFile.split('/').pop()}
                            </span>
                          )}
                        </div>
                      )}

                      {/* Execute button */}
                      {currentFunction && (
                        <div className="flex gap-2">
                          <button
                            onClick={() =>
                              executePlugin(
                                plugin.manifest.id,
                                currentFunction,
                                funcMetadata?.requires_file || false
                              )
                            }
                            disabled={executing}
                            className="flex items-center gap-2 rounded-lg bg-purple-600 px-3 py-2 text-xs font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
                          >
                            {executing ? (
                              <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                            ) : (
                              <Zap className="h-3.5 w-3.5" />
                            )}
                            {executing ? 'Executing…' : `Run ${currentFunction}`}
                          </button>
                        </div>
                      )}

                      {/* Results */}
                      {executionResult && (
                        <div className="rounded-lg border border-green-200 bg-green-50 p-3 dark:border-green-900 dark:bg-green-950/30">
                          <div className="text-xs font-medium text-green-900 dark:text-green-100">
                            Result
                          </div>
                          <div className="mt-1 text-xs text-green-700 dark:text-green-300">
                            {formatResult(executionResult)}
                          </div>
                          <div className="mt-2 text-xs text-green-600 dark:text-green-400">
                            File size: {executionResult.size.toLocaleString()} bytes
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })()}
              </div>
            </div>
          );
        })}
      </div>
    </Panel>
  );
}
