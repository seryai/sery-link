// React hooks for Tauri tray-initiated events.
//
// The tray menu (src-tauri/src/tray.rs) emits events like
// "tray:show-add-machine" when users click menu items. These hooks let
// the React shell subscribe with a single line, without pulling the
// Tauri event API into every component.
//
// Usage in a top-level component:
//
//   useTrayAddMachineListener(() => setShowAddMachineModal(true));
//
// The hook takes care of Tauri's async listen() → unlisten() lifecycle
// and React's strict-mode double-mount correctness.

import { useEffect } from 'react';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** Event name constants — keep in sync with tray.rs. */
export const TRAY_EVENTS = {
  SHOW_ADD_MACHINE: 'tray:show-add-machine',
} as const;

/**
 * Subscribe to an arbitrary Tauri event for the lifetime of the
 * calling component. Handles the async listen/unlisten dance.
 *
 * Handler must be stable across renders (wrap in useCallback if it
 * captures changing state) to avoid unnecessary re-subscriptions.
 */
export function useTauriEventListener<T = unknown>(
  eventName: string,
  handler: (payload: T) => void,
): void {
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let cancelled = false;

    listen<T>(eventName, event => handler(event.payload)).then(off => {
      if (cancelled) {
        off();
      } else {
        unlisten = off;
      }
    });

    return () => {
      cancelled = true;
      if (unlisten) unlisten();
    };
  }, [eventName, handler]);
}

/**
 * Listen for the tray's "Add Another Machine…" menu click. Fires the
 * provided callback each time the user selects the menu item.
 *
 * Typical callback: open the <AddMachineModal /> or navigate to a
 * fleet/devices route.
 */
export function useTrayAddMachineListener(onTriggered: () => void): void {
  useTauriEventListener(TRAY_EVENTS.SHOW_ADD_MACHINE, onTriggered);
}
