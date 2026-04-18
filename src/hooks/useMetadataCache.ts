// Hook for interacting with the local metadata cache
// Enables offline fuzzy search over all dataset metadata

import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useMemo, useState } from 'react';

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

  // Each method is wrapped in useCallback so its identity stays stable
  // across renders while workspaceId is unchanged. Consumers that pass
  // the returned `cache` object into useEffect/useMemo deps (notably
  // CommandPalette) otherwise see it change every render and re-fire
  // effects that call setState — a classic infinite-loop shape.
  const search = useCallback(
    async (query: string, limit: number = 20): Promise<SearchResult[]> => {
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
    },
    [workspaceId],
  );

  const getAll = useCallback(async (): Promise<CachedDataset[]> => {
    if (!workspaceId) return [];
    try {
      return await invoke<CachedDataset[]>('get_all_cached_datasets', {
        workspaceId,
      });
    } catch (err) {
      console.error('Failed to get all cached datasets:', err);
      return [];
    }
  }, [workspaceId]);

  const getById = useCallback(
    async (id: string): Promise<CachedDataset | null> => {
      try {
        return await invoke<CachedDataset | null>('get_cached_dataset', { id });
      } catch (err) {
        console.error('Failed to get cached dataset:', err);
        return null;
      }
    },
    [],
  );

  const upsert = useCallback(async (dataset: CachedDataset): Promise<void> => {
    try {
      await invoke('upsert_cached_dataset', { dataset });
      const newStats = await invoke<CacheStats>('get_cache_stats');
      setStats(newStats);
    } catch (err) {
      console.error('Failed to upsert dataset:', err);
      throw err;
    }
  }, []);

  const upsertMany = useCallback(
    async (datasets: CachedDataset[]): Promise<void> => {
      try {
        await invoke('upsert_cached_datasets', { datasets });
        const newStats = await invoke<CacheStats>('get_cache_stats');
        setStats(newStats);
      } catch (err) {
        console.error('Failed to upsert datasets:', err);
        throw err;
      }
    },
    [],
  );

  const clearWorkspace = useCallback(async (): Promise<void> => {
    if (!workspaceId) return;
    try {
      await invoke('clear_cached_workspace', { workspaceId });
      setStats({ dataset_count: 0, total_size_bytes: 0 });
    } catch (err) {
      console.error('Failed to clear workspace cache:', err);
      throw err;
    }
  }, [workspaceId]);

  // Memoize the returned object so its identity only changes when the
  // underlying values/callbacks actually change. Without this, every
  // render returns a fresh object, which poisons deps arrays in every
  // consumer.
  return useMemo(
    () => ({
      stats,
      search,
      getAll,
      getById,
      upsert,
      upsertMany,
      clearWorkspace,
    }),
    [stats, search, getAll, getById, upsert, upsertMany, clearWorkspace],
  );
}
