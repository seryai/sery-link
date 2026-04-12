// Minimal toast provider. Gives the rest of the app a `useToast()` hook with
// success/error/info helpers. Toasts auto-dismiss after 4 seconds and stack
// in the bottom-right corner. Styled for both light and dark modes.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { CheckCircle2, AlertCircle, Info, X } from 'lucide-react';

type ToastKind = 'success' | 'error' | 'info';

interface ToastItem {
  id: number;
  kind: ToastKind;
  message: string;
}

interface ToastAPI {
  success: (message: string) => void;
  error: (message: string) => void;
  info: (message: string) => void;
  dismiss: (id: number) => void;
}

const ToastContext = createContext<ToastAPI | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastItem[]>([]);
  const idRef = useRef(0);

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, message: string) => {
      const id = ++idRef.current;
      setToasts((prev) => [...prev, { id, kind, message }]);
      setTimeout(() => dismiss(id), 4000);
    },
    [dismiss],
  );

  const api = useMemo<ToastAPI>(
    () => ({
      success: (m) => push('success', m),
      error: (m) => push('error', m),
      info: (m) => push('info', m),
      dismiss,
    }),
    [push, dismiss],
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-full max-w-sm flex-col gap-2">
        {toasts.map((t) => (
          <ToastCard key={t.id} toast={t} onDismiss={() => dismiss(t.id)} />
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastAPI {
  const ctx = useContext(ToastContext);
  if (!ctx) {
    // Graceful no-op so hooks that call toast before the provider mounts
    // don't crash the app (e.g. during early bootstrap).
    return {
      success: () => undefined,
      error: () => undefined,
      info: () => undefined,
      dismiss: () => undefined,
    };
  }
  return ctx;
}

function ToastCard({
  toast,
  onDismiss,
}: {
  toast: ToastItem;
  onDismiss: () => void;
}) {
  const [visible, setVisible] = useState(false);
  useEffect(() => {
    // Trigger the slide-up animation on mount
    const t = setTimeout(() => setVisible(true), 10);
    return () => clearTimeout(t);
  }, []);

  const styles = {
    success: {
      bg: 'bg-white dark:bg-slate-800',
      border: 'border-emerald-200 dark:border-emerald-900',
      icon: <CheckCircle2 className="h-5 w-5 text-emerald-500" />,
    },
    error: {
      bg: 'bg-white dark:bg-slate-800',
      border: 'border-rose-200 dark:border-rose-900',
      icon: <AlertCircle className="h-5 w-5 text-rose-500" />,
    },
    info: {
      bg: 'bg-white dark:bg-slate-800',
      border: 'border-sky-200 dark:border-sky-900',
      icon: <Info className="h-5 w-5 text-sky-500" />,
    },
  }[toast.kind];

  return (
    <div
      className={`pointer-events-auto flex items-start gap-3 rounded-lg border p-3 shadow-lg transition-all duration-200 ${styles.bg} ${styles.border} ${
        visible ? 'translate-y-0 opacity-100' : 'translate-y-2 opacity-0'
      }`}
    >
      <div className="mt-0.5 shrink-0">{styles.icon}</div>
      <div className="flex-1 text-sm text-slate-800 dark:text-slate-100">
        {toast.message}
      </div>
      <button
        onClick={onDismiss}
        className="shrink-0 rounded p-0.5 text-slate-400 hover:bg-slate-100 hover:text-slate-600 dark:hover:bg-slate-700 dark:hover:text-slate-200"
        aria-label="Dismiss"
      >
        <X className="h-4 w-4" />
      </button>
    </div>
  );
}
