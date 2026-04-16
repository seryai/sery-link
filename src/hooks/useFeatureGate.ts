import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

export type AuthMode =
  | { type: 'LocalOnly' }
  | { type: 'BYOK'; provider: string }
  | { type: 'WorkspaceKey' };

export interface FeatureGateResult {
  available: boolean;
  mode: AuthMode | null;
  loading: boolean;
}

/**
 * Hook to check if a feature is available in the current auth mode.
 *
 * Features:
 * - free_recipes: Available in all modes
 * - pro_recipes: Requires BYOK or WorkspaceKey
 * - cloud_sync: Requires WorkspaceKey
 * - team_sharing: Requires WorkspaceKey
 *
 * @param feature The feature to check
 * @returns Feature availability, current auth mode, and loading state
 */
export function useFeatureGate(feature: string): FeatureGateResult {
  const [available, setAvailable] = useState(false);
  const [mode, setMode] = useState<AuthMode | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;

    const checkFeature = async () => {
      try {
        const [isAvailable, currentMode] = await Promise.all([
          invoke<boolean>('check_feature_available', { feature }),
          invoke<AuthMode>('get_current_auth_mode'),
        ]);

        if (!cancelled) {
          setAvailable(isAvailable);
          setMode(currentMode);
          setLoading(false);
        }
      } catch (err) {
        console.error('Failed to check feature availability:', err);
        if (!cancelled) {
          setAvailable(false);
          setLoading(false);
        }
      }
    };

    checkFeature();

    return () => {
      cancelled = true;
    };
  }, [feature]);

  return { available, mode, loading };
}
