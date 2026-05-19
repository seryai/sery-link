import { invoke } from '@tauri-apps/api/core';

/**
 * Call any registered agent RPC command locally.
 * Same shape as the cloud API: POST /v1/agent/{id}/invoke { command, args }.
 * Both Sery Link UI and Dashboard call the same underlying command registry —
 * Sery Link via this Tauri shim, Dashboard via the WebSocket tunnel.
 */
export async function agentInvoke<T = unknown>(
  command: string,
  args: Record<string, unknown> = {},
): Promise<T> {
  return invoke<T>('agent_invoke', { command, args });
}
