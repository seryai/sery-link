// Notifications view — renders schema-change events from the store.
//
// Stores: agentStore.schemaNotifications. Populated by the
// schema_changed event listener in useAgentEvents. One entry per
// dataset whose shape drifted since the last scan.
//
// For each notification we show:
//   - Dataset name + relative path
//   - Received timestamp (relative: "3 minutes ago")
//   - A one-line summary (N added, N removed, N type changed)
//   - Full diff: the list of ColumnChange entries with old/new types
//
// Clicking a row marks it read. Bulk actions at the top: "Mark all
// read", "Clear". Unread entries have a blue dot and slightly
// stronger contrast.

import { useMemo } from 'react';
import { Bell, Check, Trash2, Plus, Minus, ArrowRight } from 'lucide-react';
import { useAgentStore, type SchemaNotification } from '../stores/agentStore';
import type { ColumnChange } from '../types/events';

export function Notifications() {
  const schemaNotifications = useAgentStore((s) => s.schemaNotifications);
  const markAllRead = useAgentStore((s) => s.markAllSchemaNotificationsRead);
  const clearAll = useAgentStore((s) => s.clearSchemaNotifications);

  const unreadCount = useMemo(
    () => schemaNotifications.filter((n) => !n.read).length,
    [schemaNotifications],
  );

  if (schemaNotifications.length === 0) {
    return <EmptyState />;
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-slate-200 px-6 py-4 dark:border-slate-800">
        <div>
          <h1 className="text-lg font-semibold text-slate-900 dark:text-slate-50">
            Schema changes
          </h1>
          <p className="text-sm text-slate-500 dark:text-slate-400">
            {schemaNotifications.length} total
            {unreadCount > 0 ? ` · ${unreadCount} unread` : ''}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={markAllRead}
            disabled={unreadCount === 0}
            className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 disabled:cursor-not-allowed disabled:opacity-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <Check className="h-3.5 w-3.5" />
            Mark all read
          </button>
          <button
            onClick={clearAll}
            className="inline-flex items-center gap-1.5 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm font-medium text-slate-700 hover:bg-slate-50 dark:border-slate-700 dark:bg-slate-900 dark:text-slate-200 dark:hover:bg-slate-800"
          >
            <Trash2 className="h-3.5 w-3.5" />
            Clear
          </button>
        </div>
      </header>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        <ul className="space-y-3">
          {schemaNotifications.map((n) => (
            <NotificationCard key={n.id} notification={n} />
          ))}
        </ul>
      </div>
    </div>
  );
}

function NotificationCard({ notification }: { notification: SchemaNotification }) {
  const markRead = useAgentStore((s) => s.markSchemaNotificationRead);

  return (
    <li
      className={`rounded-lg border transition-colors ${
        notification.read
          ? 'border-slate-200 bg-white dark:border-slate-800 dark:bg-slate-900'
          : 'border-purple-200 bg-purple-50/40 dark:border-purple-900 dark:bg-purple-950/20'
      }`}
    >
      <button
        onClick={() => markRead(notification.id)}
        disabled={notification.read}
        className="block w-full text-left"
      >
        <div className="flex items-start gap-3 px-4 py-3">
          {!notification.read && (
            <span className="mt-2 h-2 w-2 flex-shrink-0 rounded-full bg-purple-500" />
          )}
          {notification.read && <span className="mt-2 h-2 w-2 flex-shrink-0" />}
          <div className="min-w-0 flex-1">
            <div className="flex items-baseline justify-between gap-2">
              <h3 className="truncate text-sm font-semibold text-slate-900 dark:text-slate-50">
                {notification.dataset_name}
              </h3>
              <time
                className="flex-shrink-0 text-xs text-slate-500 dark:text-slate-400"
                dateTime={notification.received_at}
              >
                {formatRelativeTime(notification.received_at)}
              </time>
            </div>
            <p className="mt-0.5 truncate text-xs text-slate-500 dark:text-slate-400">
              {notification.dataset_path}
            </p>
            <p className="mt-2 text-xs text-slate-600 dark:text-slate-300">
              {summarize(notification)}
            </p>
          </div>
        </div>
      </button>

      {notification.diff.changes.length > 0 && (
        <ul className="border-t border-slate-200 px-4 py-3 text-xs dark:border-slate-800">
          {notification.diff.changes.map((change, i) => (
            <li key={i} className="flex items-center gap-2 py-1">
              <ChangeIcon change={change} />
              <ChangeText change={change} />
            </li>
          ))}
        </ul>
      )}
    </li>
  );
}

function ChangeIcon({ change }: { change: ColumnChange }) {
  if (change.kind === 'Added') {
    return <Plus className="h-3.5 w-3.5 flex-shrink-0 text-emerald-500" />;
  }
  if (change.kind === 'Removed') {
    return <Minus className="h-3.5 w-3.5 flex-shrink-0 text-rose-500" />;
  }
  return <ArrowRight className="h-3.5 w-3.5 flex-shrink-0 text-amber-500" />;
}

function ChangeText({ change }: { change: ColumnChange }) {
  if (change.kind === 'Added') {
    return (
      <span className="min-w-0 truncate text-slate-700 dark:text-slate-300">
        <span className="font-medium text-slate-900 dark:text-slate-100">
          {change.name}
        </span>{' '}
        <span className="text-slate-500 dark:text-slate-400">
          ({change.column_type})
        </span>
      </span>
    );
  }
  if (change.kind === 'Removed') {
    return (
      <span className="min-w-0 truncate text-slate-700 dark:text-slate-300">
        <span className="font-medium text-slate-900 dark:text-slate-100">
          {change.name}
        </span>{' '}
        <span className="text-slate-500 dark:text-slate-400">
          was ({change.column_type})
        </span>
      </span>
    );
  }
  return (
    <span className="min-w-0 truncate text-slate-700 dark:text-slate-300">
      <span className="font-medium text-slate-900 dark:text-slate-100">
        {change.name}
      </span>{' '}
      <span className="text-slate-500 dark:text-slate-400">
        {change.old_type} → {change.new_type}
      </span>
    </span>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 px-6 py-12 text-center">
      <Bell className="h-10 w-10 text-slate-400 dark:text-slate-600" />
      <h2 className="text-lg font-semibold text-slate-900 dark:text-slate-100">
        No schema changes
      </h2>
      <p className="max-w-sm text-sm text-slate-500 dark:text-slate-400">
        Sery watches your folders for changes to file structure — a column
        that got renamed, a type that flipped, a new field that appeared.
        Those show up here.
      </p>
    </div>
  );
}

function summarize(n: SchemaNotification): string {
  const parts: string[] = [];
  if (n.added > 0) parts.push(`${n.added} added`);
  if (n.removed > 0) parts.push(`${n.removed} removed`);
  if (n.type_changed > 0) parts.push(`${n.type_changed} type changed`);
  return parts.length === 0 ? 'no detail' : parts.join(' · ');
}

// Short-form relative time — good enough for notification lists without
// pulling date-fns. Updates only when the component re-renders, which
// is fine because the store mutates on every new notification.
function formatRelativeTime(iso: string): string {
  const ms = Date.now() - new Date(iso).getTime();
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return 'just now';
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m ago`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h ago`;
  const day = Math.floor(hr / 24);
  if (day < 7) return `${day}d ago`;
  return new Date(iso).toLocaleDateString();
}
