import { AlertCircle, Sparkles } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import { useState } from 'react';
import type { AuthMode } from '../hooks/useFeatureGate';

interface UpgradePromptProps {
  variant?: 'banner' | 'modal';
  feature: string;
  onClose?: () => void;
  onUpgrade?: () => void;
}

/**
 * UpgradePrompt — prompt users to upgrade from Local Vault to PRO features.
 *
 * Variants:
 * - banner: Inline banner for gated panels
 * - modal: Full modal dialog for blocked actions
 */
export function UpgradePrompt({
  variant = 'banner',
  feature,
  onClose,
  onUpgrade,
}: UpgradePromptProps) {
  const [loading, setLoading] = useState(false);

  const handleConnectWorkspace = async () => {
    setLoading(true);
    try {
      // Navigate to settings/auth page or trigger workspace key dialog
      // For now, we'll just call the onUpgrade callback
      onUpgrade?.();
    } finally {
      setLoading(false);
    }
  };

  const handleBYOK = async () => {
    setLoading(true);
    try {
      // Set BYOK mode with user's API key
      // This would typically open a dialog to input the key
      const apiKey = prompt('Enter your Anthropic API key:');
      if (apiKey) {
        await invoke('set_auth_mode', {
          mode: {
            type: 'BYOK',
            provider: 'anthropic',
            api_key: apiKey,
          } as AuthMode,
        });
        onUpgrade?.();
      }
    } catch (err) {
      console.error('Failed to set BYOK mode:', err);
      alert('Failed to configure API key. Please try again.');
    } finally {
      setLoading(false);
    }
  };

  if (variant === 'banner') {
    return (
      <div className="rounded-lg border border-purple-200 bg-purple-50 p-4 dark:border-purple-800 dark:bg-purple-900/20">
        <div className="flex items-start gap-3">
          <Sparkles className="h-5 w-5 text-purple-600 dark:text-purple-400 flex-shrink-0 mt-0.5" />
          <div className="flex-1">
            <h3 className="text-sm font-semibold text-purple-900 dark:text-purple-100">
              Unlock PRO Features
            </h3>
            <p className="mt-1 text-sm text-purple-700 dark:text-purple-300">
              This {feature} requires a PRO account. Connect your Sery workspace or
              bring your own API key to unlock advanced features.
            </p>
            <div className="mt-3 flex gap-2">
              <button
                onClick={handleConnectWorkspace}
                disabled={loading}
                className="rounded-md bg-purple-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-purple-700 disabled:opacity-50"
              >
                Connect Workspace
              </button>
              <button
                onClick={handleBYOK}
                disabled={loading}
                className="rounded-md border border-purple-300 bg-white px-3 py-1.5 text-sm font-medium text-purple-700 hover:bg-purple-50 dark:border-purple-700 dark:bg-purple-900/50 dark:text-purple-200 dark:hover:bg-purple-900/70 disabled:opacity-50"
              >
                Use My API Key
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  // Modal variant
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-md rounded-lg bg-white p-6 shadow-xl dark:bg-slate-800">
        <div className="flex items-start gap-3">
          <div className="rounded-full bg-purple-100 p-2 dark:bg-purple-900/30">
            <AlertCircle className="h-6 w-6 text-purple-600 dark:text-purple-400" />
          </div>
          <div className="flex-1">
            <h2 className="text-lg font-bold text-slate-900 dark:text-slate-50">
              PRO Feature Required
            </h2>
            <p className="mt-2 text-sm text-slate-600 dark:text-slate-400">
              This {feature} requires a PRO account. Choose an option below to continue:
            </p>
          </div>
        </div>

        <div className="mt-6 space-y-3">
          <button
            onClick={handleConnectWorkspace}
            disabled={loading}
            className="flex w-full items-center justify-center gap-2 rounded-md bg-purple-600 px-4 py-3 text-sm font-medium text-white hover:bg-purple-700 disabled:opacity-50"
          >
            <Sparkles className="h-4 w-4" />
            Connect Sery Workspace
          </button>
          <button
            onClick={handleBYOK}
            disabled={loading}
            className="flex w-full items-center justify-center gap-2 rounded-md border border-slate-300 bg-white px-4 py-3 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-600 dark:bg-slate-700 dark:text-slate-200 dark:hover:bg-slate-600 disabled:opacity-50"
          >
            Use My Own API Key (BYOK)
          </button>
        </div>

        <div className="mt-6 border-t border-slate-200 pt-4 dark:border-slate-700">
          <p className="text-xs text-slate-500 dark:text-slate-400">
            <strong>FREE:</strong> Column-aware search, schemas, and profiles — fully local
            <br />
            <strong>PRO:</strong> AI-powered analysis across every machine you own
          </p>
        </div>

        {onClose && (
          <button
            onClick={onClose}
            className="mt-4 w-full text-sm text-slate-600 hover:text-slate-900 dark:text-slate-400 dark:hover:text-slate-200"
          >
            Cancel
          </button>
        )}
      </div>
    </div>
  );
}
