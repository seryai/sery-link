// Plugin Marketplace Browser
import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Download, Search, Star, TrendingUp, Award, ExternalLink, RefreshCw } from 'lucide-react';
import { useToast } from './Toast';

interface PluginManifest {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  capabilities: string[];
  permissions: string[];
  homepage?: string;
}

interface PluginMetrics {
  downloads: number;
  stars: number;
  rating: number;
  review_count: number;
  last_updated: string;
}

interface MarketplaceEntry {
  manifest: PluginManifest;
  source: {
    type: string;
    owner?: string;
    repo?: string;
    tag?: string;
    url?: string;
    path?: string;
  };
  metrics: PluginMetrics;
  featured: boolean;
  verified: boolean;
  tags: string[];
  screenshots: string[];
}

type ViewMode = 'featured' | 'popular' | 'all';

export function MarketplacePanel() {
  const toast = useToast();
  const [loading, setLoading] = useState(true);
  const [plugins, setPlugins] = useState<MarketplaceEntry[]>([]);
  const [filteredPlugins, setFilteredPlugins] = useState<MarketplaceEntry[]>([]);
  const [searchQuery, setSearchQuery] = useState('');
  const [viewMode, setViewMode] = useState<ViewMode>('featured');
  const [selectedPlugin, setSelectedPlugin] = useState<MarketplaceEntry | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);

  useEffect(() => {
    loadMarketplace();
  }, []);

  useEffect(() => {
    filterPlugins();
  }, [searchQuery, viewMode, plugins]);

  const loadMarketplace = async () => {
    setLoading(true);
    try {
      const marketplace = await invoke<{ plugins: MarketplaceEntry[] }>('load_marketplace');
      setPlugins(marketplace.plugins);
    } catch (err) {
      toast.error(`Failed to load marketplace: ${err}`);
    } finally {
      setLoading(false);
    }
  };

  const filterPlugins = async () => {
    try {
      let filtered: MarketplaceEntry[] = [];

      if (viewMode === 'featured') {
        const featured = await invoke<MarketplaceEntry[]>('get_featured_plugins');
        filtered = featured;
      } else if (viewMode === 'popular') {
        const popular = await invoke<MarketplaceEntry[]>('get_popular_plugins', { limit: 10 });
        filtered = popular;
      } else {
        filtered = plugins;
      }

      if (searchQuery.trim()) {
        const results = await invoke<MarketplaceEntry[]>('search_marketplace', {
          query: searchQuery,
        });
        filtered = results;
      }

      setFilteredPlugins(filtered);
    } catch (err) {
      console.error('Filter error:', err);
      setFilteredPlugins(plugins);
    }
  };

  const installPlugin = async (pluginId: string) => {
    setInstalling(pluginId);
    try {
      await invoke('install_marketplace_plugin', { pluginId });
      toast.success('Plugin installed successfully');
      setSelectedPlugin(null);
    } catch (err) {
      toast.error(`Installation failed: ${err}`);
    } finally {
      setInstalling(null);
    }
  };

  const formatNumber = (num: number): string => {
    if (num >= 1000) {
      return `${(num / 1000).toFixed(1)}k`;
    }
    return num.toString();
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center py-12">
        <RefreshCw className="h-6 w-6 animate-spin text-slate-400" />
      </div>
    );
  }

  // Plugin detail modal
  if (selectedPlugin) {
    return (
      <div className="space-y-4">
        <button
          onClick={() => setSelectedPlugin(null)}
          className="text-sm text-purple-600 hover:text-purple-700 dark:text-purple-400 dark:hover:text-purple-300"
        >
          ← Back to marketplace
        </button>

        <div className="rounded-lg border border-slate-200 bg-white p-6 dark:border-slate-800 dark:bg-slate-900">
          <div className="flex items-start justify-between">
            <div className="flex-1">
              <div className="flex items-center gap-2">
                <h2 className="text-xl font-semibold text-slate-900 dark:text-slate-100">
                  {selectedPlugin.manifest.name}
                </h2>
                {selectedPlugin.verified && (
                  <span title="Verified by Sery">
                    <Award className="h-5 w-5 text-purple-600 dark:text-purple-400" />
                  </span>
                )}
              </div>
              <p className="mt-1 text-sm text-slate-600 dark:text-slate-400">
                v{selectedPlugin.manifest.version} by {selectedPlugin.manifest.author}
              </p>
            </div>
            <button
              onClick={() => installPlugin(selectedPlugin.manifest.id)}
              disabled={installing === selectedPlugin.manifest.id}
              className="flex items-center gap-2 rounded-lg bg-purple-600 px-4 py-2 text-sm font-semibold text-white shadow-sm transition-colors hover:bg-purple-700 disabled:opacity-50"
            >
              {installing === selectedPlugin.manifest.id ? (
                <>
                  <RefreshCw className="h-4 w-4 animate-spin" />
                  Installing...
                </>
              ) : (
                <>
                  <Download className="h-4 w-4" />
                  Install
                </>
              )}
            </button>
          </div>

          <p className="mt-4 text-sm text-slate-700 dark:text-slate-300">
            {selectedPlugin.manifest.description}
          </p>

          <div className="mt-4 flex items-center gap-4 text-sm text-slate-500 dark:text-slate-400">
            <div className="flex items-center gap-1">
              <Download className="h-4 w-4" />
              {formatNumber(selectedPlugin.metrics.downloads)} downloads
            </div>
            <div className="flex items-center gap-1">
              <Star className="h-4 w-4 fill-yellow-400 text-yellow-400" />
              {selectedPlugin.metrics.rating.toFixed(1)} ({selectedPlugin.metrics.review_count} reviews)
            </div>
            <div className="flex items-center gap-1">
              {formatNumber(selectedPlugin.metrics.stars)} stars
            </div>
          </div>

          <div className="mt-4">
            <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
              Capabilities
            </h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {selectedPlugin.manifest.capabilities.map((cap) => (
                <span
                  key={cap}
                  className="rounded bg-purple-100 px-2 py-1 text-xs font-medium text-purple-700 dark:bg-purple-900/40 dark:text-purple-300"
                >
                  {cap}
                </span>
              ))}
            </div>
          </div>

          <div className="mt-4">
            <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
              Permissions Required
            </h3>
            <div className="mt-2 flex flex-wrap gap-2">
              {selectedPlugin.manifest.permissions.length > 0 ? (
                selectedPlugin.manifest.permissions.map((perm) => (
                  <span
                    key={perm}
                    className="rounded bg-amber-100 px-2 py-1 text-xs font-medium text-amber-700 dark:bg-amber-900/40 dark:text-amber-300"
                  >
                    {perm}
                  </span>
                ))
              ) : (
                <span className="text-xs text-slate-500 dark:text-slate-400">
                  No special permissions required
                </span>
              )}
            </div>
          </div>

          {selectedPlugin.manifest.homepage && (
            <div className="mt-4">
              <a
                href={selectedPlugin.manifest.homepage}
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-1 text-sm text-purple-600 hover:text-purple-700 dark:text-purple-400 dark:hover:text-purple-300"
              >
                <ExternalLink className="h-4 w-4" />
                View documentation
              </a>
            </div>
          )}
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* Search and filter bar */}
      <div className="flex items-center gap-3">
        <div className="relative flex-1">
          <Search className="absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-slate-400" />
          <input
            type="text"
            placeholder="Search plugins..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            className="w-full rounded-lg border border-slate-300 bg-white py-2 pl-10 pr-4 text-sm text-slate-900 placeholder-slate-400 focus:border-purple-500 focus:outline-none focus:ring-2 focus:ring-purple-500/20 dark:border-slate-700 dark:bg-slate-800 dark:text-slate-100 dark:placeholder-slate-500"
          />
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setViewMode('featured')}
            className={`flex items-center gap-1 rounded-lg px-3 py-2 text-xs font-medium transition-colors ${
              viewMode === 'featured'
                ? 'bg-purple-600 text-white'
                : 'bg-slate-100 text-slate-700 hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700'
            }`}
          >
            <Award className="h-3.5 w-3.5" />
            Featured
          </button>
          <button
            onClick={() => setViewMode('popular')}
            className={`flex items-center gap-1 rounded-lg px-3 py-2 text-xs font-medium transition-colors ${
              viewMode === 'popular'
                ? 'bg-purple-600 text-white'
                : 'bg-slate-100 text-slate-700 hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700'
            }`}
          >
            <TrendingUp className="h-3.5 w-3.5" />
            Popular
          </button>
          <button
            onClick={() => setViewMode('all')}
            className={`rounded-lg px-3 py-2 text-xs font-medium transition-colors ${
              viewMode === 'all'
                ? 'bg-purple-600 text-white'
                : 'bg-slate-100 text-slate-700 hover:bg-slate-200 dark:bg-slate-800 dark:text-slate-300 dark:hover:bg-slate-700'
            }`}
          >
            All
          </button>
        </div>
      </div>

      {/* Plugin grid */}
      {filteredPlugins.length === 0 ? (
        <div className="rounded-lg border-2 border-dashed border-slate-200 p-12 text-center dark:border-slate-800">
          <Search className="mx-auto mb-4 h-12 w-12 text-slate-400 dark:text-slate-600" />
          <h3 className="mb-2 text-lg font-semibold text-slate-900 dark:text-slate-100">
            No plugins found
          </h3>
          <p className="text-sm text-slate-500 dark:text-slate-400">
            Try a different search term or browse all plugins
          </p>
        </div>
      ) : (
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          {filteredPlugins.map((plugin) => (
            <div
              key={plugin.manifest.id}
              className="group cursor-pointer rounded-lg border border-slate-200 bg-white p-4 transition-all hover:border-purple-300 hover:shadow-md dark:border-slate-800 dark:bg-slate-900 dark:hover:border-purple-700"
              onClick={() => setSelectedPlugin(plugin)}
            >
              <div className="flex items-start justify-between">
                <h3 className="text-sm font-semibold text-slate-900 dark:text-slate-100">
                  {plugin.manifest.name}
                </h3>
                {plugin.verified && (
                  <Award className="h-4 w-4 shrink-0 text-purple-600 dark:text-purple-400" />
                )}
              </div>
              <p className="mt-1 line-clamp-2 text-xs text-slate-600 dark:text-slate-400">
                {plugin.manifest.description}
              </p>
              <div className="mt-3 flex items-center gap-3 text-xs text-slate-500 dark:text-slate-400">
                <div className="flex items-center gap-1">
                  <Download className="h-3 w-3" />
                  {formatNumber(plugin.metrics.downloads)}
                </div>
                <div className="flex items-center gap-1">
                  <Star className="h-3 w-3 fill-yellow-400 text-yellow-400" />
                  {plugin.metrics.rating.toFixed(1)}
                </div>
              </div>
              <div className="mt-2 flex flex-wrap gap-1">
                {plugin.manifest.capabilities.slice(0, 2).map((cap) => (
                  <span
                    key={cap}
                    className="rounded bg-purple-100 px-1.5 py-0.5 text-xs font-medium text-purple-700 dark:bg-purple-900/40 dark:text-purple-300"
                  >
                    {cap}
                  </span>
                ))}
                {plugin.manifest.capabilities.length > 2 && (
                  <span className="rounded bg-slate-100 px-1.5 py-0.5 text-xs font-medium text-slate-600 dark:bg-slate-800 dark:text-slate-400">
                    +{plugin.manifest.capabilities.length - 2}
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
