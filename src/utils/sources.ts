// F42 Sources — typed Tauri invoke wrappers.
//
// Thin, typed bridges to the four mutation commands shipped in
// src-tauri/src/commands.rs (Day 4 — slice 1). The Rust side is
// authoritative on the contract; this file mirrors it so frontend
// callers don't sprinkle untyped invoke('rename_source', { ... })
// everywhere with the Tauri argName camelCase pitfalls.
//
// Note on Tauri's argument-naming convention: tauri's invoke
// auto-converts the Rust snake_case parameter names to camelCase on
// the JS side. So `rename_source(id, new_name)` in Rust is called
// from JS as `invoke('rename_source', { id, newName })` — NOT
// `new_name`. These wrappers handle that translation so callers
// don't have to remember it.

import { invoke } from '@tauri-apps/api/core';
import type { DataSource } from '../types/events';

export interface S3Credentials {
  access_key_id: string;
  secret_access_key: string;
  region: string;
  session_token?: string;
  /** F45: S3-compatible endpoint host (no scheme) for B2 / Wasabi /
   *  R2 / GCS / MinIO. Leave undefined for AWS S3. The Rust side
   *  strips http(s):// schemes automatically — pasting a full URL
   *  from the provider's docs page works. */
  endpoint_url?: string;
  /** F45: `path` for B2 / R2 / MinIO; `vhost` for AWS / Wasabi
   *  (default). Most providers' docs say which one. */
  url_style?: 'path' | 'vhost';
}

/** Rename a source by id. Throws if no source matches. */
export function renameSource(id: string, newName: string): Promise<void> {
  return invoke<void>('rename_source', { id, newName });
}

/** Move a source to a group, or pass `null` to move it back to
 *  the top-level (ungrouped) section. */
export function setSourceGroup(
  id: string,
  group: string | null,
): Promise<void> {
  return invoke<void>('set_source_group', { id, group });
}

/** Drop a source by id. Does NOT remove the corresponding entry in
 *  watched_folders (legacy field is read-only post-v0.7.0 and kept
 *  for one release for rollback safety). */
export function removeSource(id: string): Promise<void> {
  return invoke<void>('remove_source', { id });
}

/** Rewrite each source's sort_order based on the input id list.
 *  IDs missing from `orderedIds` get appended at the tail in their
 *  pre-call relative order — defensive against a partial list. */
export function reorderSources(orderedIds: string[]): Promise<void> {
  return invoke<void>('reorder_sources', { orderedIds });
}

/** Load existing S3 credentials for a URL. Returns null when no
 *  entry exists (source was added without creds, or creds were
 *  never saved). Used by the Edit credentials… flow to pre-populate
 *  the dialog. */
export function getS3CredentialsForUrl(
  url: string,
): Promise<S3Credentials | null> {
  return invoke<S3Credentials | null>('get_s3_credentials_for_url', { url });
}

/** Sort a sources list by sort_order for stable rendering. */
export function sortSources(sources: DataSource[]): DataSource[] {
  return [...sources].sort((a, b) => a.sort_order - b.sort_order);
}

/** Group sources by their `group` field. Top-level (ungrouped)
 *  sources land under the empty-string key. Each group's array is
 *  sorted by sort_order. */
export function groupSources(
  sources: DataSource[],
): Map<string, DataSource[]> {
  const groups = new Map<string, DataSource[]>();
  for (const source of sortSources(sources)) {
    const key = source.group ?? '';
    const existing = groups.get(key);
    if (existing) {
      existing.push(source);
    } else {
      groups.set(key, [source]);
    }
  }
  return groups;
}

/** Human-friendly label for the protocol of a source — used in the
 *  sidebar row and the Add Source modal's protocol picker. Mirrors
 *  the labels chosen by sourceKindLabel(WatchedFolder) but operates
 *  on the new SourceKind union. */
export function sourceKindLabel(source: DataSource): string {
  switch (source.kind.kind) {
    case 'local':
      return 'Local';
    case 'https':
      return 'HTTPS';
    case 's3':
      return 'S3';
    case 'google_drive':
      return 'Google Drive';
    case 'sftp':
      return 'SFTP';
    case 'web_dav':
      return 'WebDAV';
    case 'dropbox':
      return 'Dropbox';
    case 'azure_blob':
      return 'Azure Blob';
  }
}

/** F43: SFTP credential payload — discriminated union mirroring the
 *  Rust SftpAuth enum's serde(tag = "type", rename_all = snake_case)
 *  shape. Either a password OR a private key path (with optional
 *  passphrase). */
export type SftpAuth =
  | { type: 'password'; password: string }
  | {
      type: 'private_key';
      private_key_path: string;
      passphrase?: string;
    };

/** F44: WebDAV credential payload — discriminated union mirroring
 *  the Rust WebDavAuth enum's serde tag. Anonymous = no creds (rare
 *  but exists); Basic = typical Nextcloud / ownCloud; Digest =
 *  legacy servers. */
export type WebDavAuth =
  | { type: 'anonymous' }
  | { type: 'basic'; username: string; password: string }
  | { type: 'digest'; username: string; password: string };

/** Bridge from the new structured `SourceKind` to the legacy string
 *  enum used by `<SourceIcon kind=...>`. The legacy enum predates
 *  F42 and is still consumed by FolderList; we map across so the
 *  Sources sidebar can reuse the same icon set without duplicating
 *  the SVGs. SFTP doesn't have a brand mark in SourceIcon yet — it
 *  falls through to the local-folder icon which is the closest
 *  semantic match (a folder you can see files inside). Replace with
 *  a dedicated SSH-terminal mark in a polish slice. */
export function legacyKindStringOf(
  source: DataSource,
): 'local' | 's3' | 'gdrive' | 'http' {
  switch (source.kind.kind) {
    case 'local':
      return 'local';
    case 's3':
      return 's3';
    case 'google_drive':
      return 'gdrive';
    case 'https':
      return 'http';
    case 'sftp':
      // Fallback to 'local' icon visually — a remote folder you
      // browse like a local one. Future: dedicated SSH terminal mark.
      return 'local';
    case 'web_dav':
      // Fallback to 'http' (globe) since WebDAV is HTTP-based.
      // Future: dedicated WebDAV mark.
      return 'http';
    case 'dropbox':
      // Fallback to 'http' (globe). Future: dedicated Dropbox blue
      // box mark.
      return 'http';
    case 'azure_blob':
      // Fallback to 'http' (globe). Future: Azure cloud icon.
      return 'http';
  }
}

/** Return the path/url string used as the `scansInFlight` key for
 *  this source. Mirrors the Rust resolve_source_path semantics:
 *  Local → path, Https/S3 → url, Drive → null (Drive scans don't go
 *  through the path-keyed scanner), SFTP → null until the
 *  download-on-rescan flow lands. */
export function scanKeyOf(source: DataSource): string | null {
  switch (source.kind.kind) {
    case 'local':
      return source.kind.path;
    case 'https':
    case 's3':
      return source.kind.url;
    case 'google_drive':
    case 'sftp':
    case 'web_dav':
    case 'dropbox':
    case 'azure_blob':
      return null;
  }
}
