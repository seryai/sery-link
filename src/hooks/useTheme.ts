// Theme controller. Reads `config.app.theme` from the agent store and keeps
// the `html.dark` class in sync. When the user picks "system" we subscribe
// to the OS `prefers-color-scheme` media query so the UI follows the system.

import { useEffect } from 'react';
import { useAgentStore } from '../stores/agentStore';

type Theme = 'light' | 'dark' | 'system';

export function useTheme() {
  const theme = useAgentStore((s) => s.config?.app.theme ?? 'system') as Theme;

  useEffect(() => {
    const root = document.documentElement;

    const apply = (dark: boolean) => {
      if (dark) {
        root.classList.add('dark');
      } else {
        root.classList.remove('dark');
      }
    };

    if (theme === 'system') {
      const mql = window.matchMedia('(prefers-color-scheme: dark)');
      apply(mql.matches);
      const handler = (e: MediaQueryListEvent) => apply(e.matches);
      mql.addEventListener('change', handler);
      return () => mql.removeEventListener('change', handler);
    }

    apply(theme === 'dark');
  }, [theme]);
}
