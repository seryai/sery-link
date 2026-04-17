// Analytics hub — combines recipe library with context-aware suggestions
// and query builder (future). This is where users take action on their data.
//
// Layout:
//   1. Suggested Recipes (based on available data sources in Folders)
//   2. Recipe Library (full searchable catalog)
//   3. Query Builder (future feature placeholder)

import { useState, useEffect } from 'react';
import { useParams } from 'react-router-dom';
import { Sparkles, Info, Folder, Database, Table, FileText } from 'lucide-react';
import { RecipePanel } from './RecipePanel';
import { useAgentStore } from '../stores/agentStore';
import type { WatchedFolder } from '../types/events';

interface AnalyticsProps {
  filterByDataSource?: string | null;
  autoOpenRecipe?: string | null;
}

export function Analytics({ filterByDataSource, autoOpenRecipe }: AnalyticsProps = {}) {
  const { folderId } = useParams<{ folderId: string }>();
  const { config } = useAgentStore();
  const [detectedSources, setDetectedSources] = useState<string[]>([]);
  const [selectedFolder, setSelectedFolder] = useState<WatchedFolder | null>(null);

  // Detect available data sources from watched folders
  useEffect(() => {
    const detectDataSources = async () => {
      if (!config?.watched_folders) return;

      const sources = new Set<string>();
      let decodedFolderId: string | null = null;

      // Decode folder ID if present in URL
      if (folderId) {
        decodedFolderId = decodeURIComponent(folderId);
        const folder = config.watched_folders.find(f => f.path === decodedFolderId);
        setSelectedFolder(folder || null);
      } else {
        setSelectedFolder(null);
      }

      // Filter folders based on folderId parameter
      const foldersToAnalyze = decodedFolderId
        ? config.watched_folders.filter(f => f.path === decodedFolderId)
        : config.watched_folders;

      for (const folder of foldersToAnalyze) {
        // Heuristic: detect data sources from folder paths
        const path = folder.path.toLowerCase();

        if (path.includes('shopify')) {
          sources.add('Shopify');
        }
        if (path.includes('stripe')) {
          sources.add('Stripe');
        }
        if (path.includes('analytics') || path.includes('ga')) {
          sources.add('Google Analytics');
        }
        // Always include Generic and CSV as fallbacks
        if (folder.last_scan_stats && folder.last_scan_stats.datasets > 0) {
          sources.add('CSV');
          sources.add('Generic');
        }
      }

      setDetectedSources(Array.from(sources));
    };

    detectDataSources();
  }, [config, folderId]);

  const hasData = config?.watched_folders && config.watched_folders.length > 0;
  const hasSuggestions = detectedSources.length > 0 && !filterByDataSource;

  return (
    <div className="flex h-full flex-col overflow-hidden">
      {/* Header */}
      <div className="border-b border-slate-200 bg-white px-6 py-4 dark:border-slate-800 dark:bg-slate-900">
        <div className="flex items-start justify-between">
          <div>
            <h1 className="flex items-center gap-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
              <Sparkles className="h-6 w-6 text-purple-600 dark:text-purple-400" />
              Analytics
              {selectedFolder && (
                <span className="flex items-center gap-1.5 text-base font-normal text-slate-500 dark:text-slate-400">
                  <Folder className="h-4 w-4" />
                  {selectedFolder.path.split('/').pop()}
                </span>
              )}
            </h1>
            <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
              {selectedFolder
                ? `Recipes and queries for: ${selectedFolder.path}`
                : filterByDataSource
                ? `Recipes for ${filterByDataSource} data`
                : 'Pre-built SQL recipes and custom queries for your local data'}
            </p>
          </div>
        </div>
      </div>

      {/* Folder Context - show dataset stats when analyzing specific folder */}
      {selectedFolder && selectedFolder.last_scan_stats && (
        <div className="border-b border-slate-200 bg-slate-50 px-6 py-4 dark:border-slate-800 dark:bg-slate-900/50">
          <div className="grid grid-cols-3 gap-4">
            <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-800">
              <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                <Database className="h-4 w-4" />
                Datasets
              </div>
              <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                {selectedFolder.last_scan_stats.datasets}
              </div>
            </div>
            <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-800">
              <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                <Table className="h-4 w-4" />
                Columns
              </div>
              <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                {selectedFolder.last_scan_stats.columns.toLocaleString()}
              </div>
            </div>
            <div className="rounded-lg border border-slate-200 bg-white p-4 dark:border-slate-700 dark:bg-slate-800">
              <div className="flex items-center gap-2 text-sm font-medium text-slate-600 dark:text-slate-400">
                <FileText className="h-4 w-4" />
                Size
              </div>
              <div className="mt-2 text-2xl font-bold text-slate-900 dark:text-slate-50">
                {formatBytes(selectedFolder.last_scan_stats.total_bytes)}
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Content */}
      <div className="flex-1 overflow-y-auto">
        {!hasData ? (
          // Empty state: no folders yet
          <div className="flex h-full items-center justify-center p-8">
            <div className="max-w-md text-center">
              <Sparkles className="mx-auto mb-4 h-12 w-12 text-slate-300 dark:text-slate-600" />
              <h2 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-50">
                No data sources yet
              </h2>
              <p className="mb-4 text-sm text-slate-600 dark:text-slate-400">
                Add a folder in the Folders tab to start analyzing your data with pre-built SQL recipes.
              </p>
              <div className="rounded-lg border border-blue-200 bg-blue-50 p-4 text-left dark:border-blue-900 dark:bg-blue-950">
                <div className="flex gap-3">
                  <Info className="h-5 w-5 shrink-0 text-blue-600 dark:text-blue-400" />
                  <div className="text-sm text-blue-900 dark:text-blue-100">
                    <p className="font-medium">What are recipes?</p>
                    <p className="mt-1 text-blue-700 dark:text-blue-300">
                      Recipes are pre-built SQL templates for common business questions like
                      "Top products", "Revenue trends", and "Customer cohorts". No SQL knowledge required.
                    </p>
                  </div>
                </div>
              </div>
            </div>
          </div>
        ) : hasSuggestions ? (
          // Show suggestions + full library
          <div className="space-y-8 p-6">
            {/* Suggested Recipes Section */}
            <section>
              <div className="mb-4">
                <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
                  Suggested for your data
                </h2>
                <p className="text-sm text-slate-600 dark:text-slate-400">
                  Based on {detectedSources.slice(0, 3).join(', ')}
                  {detectedSources.length > 3 && ` and ${detectedSources.length - 3} more`}
                </p>
              </div>
              <div className="rounded-lg border border-slate-200 bg-slate-50 p-4 dark:border-slate-700 dark:bg-slate-800/50">
                <RecipePanel
                  initialDataSourceFilter={detectedSources[0]}
                  showSuggestedOnly={true}
                  maxResults={6}
                />
              </div>
            </section>

            {/* Full Recipe Library */}
            <section>
              <div className="mb-4">
                <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
                  All Recipes
                </h2>
                <p className="text-sm text-slate-600 dark:text-slate-400">
                  Browse the complete library of SQL templates
                </p>
              </div>
              <RecipePanel autoOpenRecipe={autoOpenRecipe} />
            </section>
          </div>
        ) : (
          // Just show full library (when filtering by specific data source)
          <RecipePanel
            initialDataSourceFilter={filterByDataSource ?? undefined}
            autoOpenRecipe={autoOpenRecipe}
          />
        )}
      </div>
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / Math.pow(1024, i);
  return `${value.toFixed(value >= 10 || i === 0 ? 0 : 1)} ${units[i]}`;
}
