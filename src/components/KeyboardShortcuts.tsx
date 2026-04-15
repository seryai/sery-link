// Keyboard shortcuts help overlay — shows when user presses `?`
// Displays all available keyboard shortcuts in a modal dialog.

import { useEffect, useState } from 'react';
import { X, Command, Keyboard } from 'lucide-react';

interface Shortcut {
  keys: string[];
  description: string;
  section: string;
}

const shortcuts: Shortcut[] = [
  // Navigation
  { keys: ['1'], description: 'Go to Folders', section: 'Navigation' },
  { keys: ['2'], description: 'Go to History', section: 'Navigation' },
  { keys: ['3'], description: 'Go to Privacy', section: 'Navigation' },
  { keys: ['4'], description: 'Go to Settings', section: 'Navigation' },

  // Actions
  { keys: ['Cmd', 'N'], description: 'Watch new folder', section: 'Actions' },
  { keys: ['Cmd', 'R'], description: 'Rescan current folder', section: 'Actions' },
  { keys: ['Cmd', 'W'], description: 'Close window', section: 'Actions' },
  { keys: ['Cmd', 'Q'], description: 'Quit application', section: 'Actions' },

  // UI
  { keys: ['Cmd', ','], description: 'Open Settings', section: 'UI' },
  { keys: ['?'], description: 'Show keyboard shortcuts', section: 'UI' },
  { keys: ['Esc'], description: 'Close dialog / Cancel', section: 'UI' },
];

export function KeyboardShortcuts() {
  const [isOpen, setIsOpen] = useState(false);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      // Open on '?' (Shift + /)
      if (e.key === '?' && !e.metaKey && !e.ctrlKey) {
        e.preventDefault();
        setIsOpen(true);
        return;
      }

      // Close on Escape
      if (e.key === 'Escape' && isOpen) {
        setIsOpen(false);
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [isOpen]);

  if (!isOpen) return null;

  const sections = Array.from(new Set(shortcuts.map((s) => s.section)));

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="relative mx-4 w-full max-w-2xl overflow-hidden rounded-2xl border border-slate-200 bg-white shadow-2xl dark:border-slate-800 dark:bg-slate-900">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-slate-200 px-6 py-4 dark:border-slate-800">
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-lg bg-purple-100 dark:bg-purple-900/40">
              <Keyboard className="h-5 w-5 text-purple-600 dark:text-purple-300" />
            </div>
            <div>
              <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
                Keyboard Shortcuts
              </h2>
              <p className="text-xs text-slate-500 dark:text-slate-400">
                All available shortcuts in Sery Link
              </p>
            </div>
          </div>
          <button
            onClick={() => setIsOpen(false)}
            className="rounded-lg p-2 text-slate-500 transition-colors hover:bg-slate-100 hover:text-slate-700 dark:text-slate-400 dark:hover:bg-slate-800 dark:hover:text-slate-200"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Content */}
        <div className="max-h-[60vh] overflow-y-auto p-6">
          {sections.map((section) => (
            <div key={section} className="mb-6 last:mb-0">
              <h3 className="mb-3 text-sm font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">
                {section}
              </h3>
              <div className="space-y-2">
                {shortcuts
                  .filter((s) => s.section === section)
                  .map((shortcut, idx) => (
                    <div
                      key={idx}
                      className="flex items-center justify-between rounded-lg px-3 py-2 hover:bg-slate-50 dark:hover:bg-slate-800/50"
                    >
                      <span className="text-sm text-slate-700 dark:text-slate-300">
                        {shortcut.description}
                      </span>
                      <div className="flex items-center gap-1">
                        {shortcut.keys.map((key, kidx) => (
                          <span key={kidx} className="flex items-center gap-1">
                            <kbd className="flex h-6 min-w-[24px] items-center justify-center rounded border border-slate-300 bg-slate-100 px-2 text-xs font-semibold text-slate-700 shadow-sm dark:border-slate-700 dark:bg-slate-800 dark:text-slate-300">
                              {key === 'Cmd' ? (
                                <Command className="h-3 w-3" />
                              ) : (
                                key
                              )}
                            </kbd>
                            {kidx < shortcut.keys.length - 1 && (
                              <span className="text-xs text-slate-400">+</span>
                            )}
                          </span>
                        ))}
                      </div>
                    </div>
                  ))}
              </div>
            </div>
          ))}
        </div>

        {/* Footer */}
        <div className="border-t border-slate-200 bg-slate-50 px-6 py-3 dark:border-slate-800 dark:bg-slate-800/50">
          <p className="text-xs text-slate-500 dark:text-slate-400">
            Press <kbd className="rounded bg-slate-200 px-1.5 py-0.5 font-mono text-xs dark:bg-slate-700">?</kbd> anytime to show this dialog
          </p>
        </div>
      </div>
    </div>
  );
}
