// Hook for interacting with the local metadata cache
// Enables offline fuzzy search over all dataset metadata

import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';

export interface CachedDataset {
  id: string;
  workspace_id: string;
  name: string;
  path: string;
  file_format: string;
  size_bytes: number;
  schema_json?: string;
  tags: string[];
  description?: string;
  last_synced: string; // ISO 8601 datetime
}

export interface SearchResult {
  dataset: CachedDataset;
  score: number;
}

export interface CacheStats {
  dataset_count: number;
  total_size_bytes: number;
}

export function useMetadataCache(workspaceId: string | null) {
  const [stats, setStats] = useState<CacheStats | null>(null);

  useEffect(() => {
    if (!workspaceId) return;

    // Load cache stats on mount
    invoke<CacheStats>('get_cache_stats')
      .then(setStats)
      .catch((err) => console.error('Failed to load cache stats:', err));
  }, [workspaceId]);

  const search = async (query: string, limit: number = 20): Promise<SearchResult[]> => {
    if (!workspaceId) return [];

    try {
      return await invoke<SearchResult[]>('search_cached_datasets', {
        workspaceId,
        query,
        limit,
      });
    } catch (err) {
      console.error('Cache search failed:', err);
      return [];
    }
  };

  const getAll = async (): Promise<CachedDataset[]> => {
    if (!workspaceId) return [];

    try {
      return await invoke<CachedDataset[]>('get_all_cached_datasets', {
        workspaceId,
      });
    } catch (err) {
      console.error('Failed to get all cached datasets:', err);
      return [];
    }
  };

  const getById = async (id: string): Promise<CachedDataset | null> => {
    try {
      return await invoke<CachedDataset | null>('get_cached_dataset', { id });
    } catch (err) {
      console.error('Failed to get cached dataset:', err);
      return null;
    }
  };

  const upsert = async (dataset: CachedDataset): Promise<void> => {
    try {
      await invoke('upsert_cached_dataset', { dataset });
      // Refresh stats after upsert
      const newStats = await invoke<CacheStats>('get_cache_stats');
      setStats(newStats);
    } catch (err) {
      console.error('Failed to upsert dataset:', err);
      throw err;
    }
  };

  const upsertMany = async (datasets: CachedDataset[]): Promise<void> => {
    try {
      await invoke('upsert_cached_datasets', { datasets });
      // Refresh stats after bulk upsert
      const newStats = await invoke<CacheStats>('get_cache_stats');
      setStats(newStats);
    } catch (err) {
      console.error('Failed to upsert datasets:', err);
      throw err;
    }
  };

  const clearWorkspace = async (): Promise<void> => {
    if (!workspaceId) return;

    try {
      await invoke('clear_cached_workspace', { workspaceId });
      setStats({ dataset_count: 0, total_size_bytes: 0 });
    } catch (err) {
      console.error('Failed to clear workspace cache:', err);
      throw err;
    }
  };

  return {
    stats,
    search,
    getAll,
    getById,
    upsert,
    upsertMany,
    clearWorkspace,
  };
}
