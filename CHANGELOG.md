# Changelog

All notable changes to Sery Link will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.12.2] — 2026-06-01

### Added

- **Enterprise DB sources (JDBC)** — Oracle, DB2, SAP HANA, Teradata, Vertica, Databricks,
  Trino, Hive, BigQuery, Cassandra, Neo4j, Firebird, Exasol, H2, and 15+ domestic Chinese
  databases (DM, KingBase, HighGo, OceanBase, GBase, YashanDB, TDengine, etc.) can now be
  added as sources via the Driver Store. Installed drivers appear as branded tiles in the
  Add Source → Databases tab.
- **Overview: Needs Attention section** — sources with scan errors, never-scanned sources,
  or sources not scanned in over 24 hours are surfaced in an amber-bordered list on the
  Overview page with one-click navigation to the source detail.
- **Overview: 7-day query activity chart** — bar chart showing query counts per day for
  the past week, with today highlighted, success rate, and average duration in the header.

### Changed

- **Add Source Databases tab** — installed JDBC driver tiles replace the generic
  "Enterprise DB" placeholder; Snowflake and MongoDB are excluded from the driver list
  (they have native implementations).
- **Source detail header icon** — source icon in folder and database detail page headers
  shrunk to `sm` size; the previous `lg` size was visually oversized.
- **Connect modal** — removed the redundant "Name this machine" input; the machine name
  from Settings → General is used automatically.
- **Rescan / Refresh** — after a database source rescan completes, stats are now synced
  to the cloud automatically.
- **Driver Store** — driver list rows now show the database brand icon instead of a
  two-letter text placeholder.
- **Overview page** — removed the redundant sources list (already visible in the sidebar);
  replaced with the Needs Attention and activity chart sections.

### Fixed

- **Local folder sources showed "Never scanned" on Overview** — `update_folder_scan_stats`
  only wrote to the legacy `watched_folders` array; the Dashboard reads `config.sources`,
  so local sources always appeared unseen. Now `update_source_scan_stats` is also called
  for the matching `DataSource` entry after each successful folder scan.

---

## [0.12.1] — 2026-05-31

### Added

- **DB table sample rows** — during metadata sync, Sery Link now fetches up to 5
  sample rows per database table (`SELECT * … LIMIT 5`) and includes them in the
  cloud dataset payload so the dashboard can display a preview without a live query.
- **DB sources in scan-all** — database sources are now included when "Scan all" is
  triggered; previously they were silently skipped with a "skipped" status.

### Changed

- **`ColumnSchema` extended** — `is_primary_key: bool` and `default_value: Option<String>`
  added to the struct and serialised to the cloud payload so dashboards can render
  accurate PK and default indicators alongside column definitions.

### Fixed

- **Storage icon backgrounds removed** — all 13 storage SVGs (S3, GDrive, Dropbox,
  Azure, OneDrive, B2, Wasabi, R2, GCS, local, SFTP, WebDAV, HTTPS) were rewritten
  to use brand-colored paths on a transparent background, matching the style of the
  database icons.
- **DB icons appeared smaller than storage icons** — database icon SVGs carry ~15–20 %
  built-in viewBox padding; a `transform: scale(1.25)` is now applied via inline
  style so their visual weight matches the storage icons at every size.

---

## [0.10.2] — 2026-05-27

### Added

- **Auto-scan interval setting** — users can now configure the auto-scan interval
  (in minutes) from the Settings panel. Set to 0 to disable. Defaults to 60 minutes.

### Fixed

- **Auto-scan disabled by default** — `auto_scan_interval_minutes` defaulted to `None`
  (disabled); now defaults to 60 minutes so sources scan automatically without any
  manual configuration.
- **No scan on source add** — adding a local or remote source now triggers an immediate
  background scan so the status dot turns green right away instead of waiting up to
  60 minutes for the first auto-scan tick.
- **`CLOUD_OFFLINE` never reset** — after a failed cloud sync the flag blocked all
  subsequent sync attempts for the entire session, even after connectivity recovered.
  Now clears on every successful WebSocket reconnect.

---

## [0.9.9] — 2026-05-19

### Fixed

- **v0.9.8 CI build failure** — `scanner.rs` rewrite (removing all direct `pdfium-render`
  calls) was not included in the v0.9.8 tag; `Cargo.toml` had the dep removed but the
  source still referenced it, causing `error[E0433]: cannot find module or crate pdfium_render`.
  This release commits the missing source changes alongside the already-merged dep removal.

---

## [0.9.8] — 2026-05-19

### Fixed

- **PDF extraction via dashboard always fails with `PdfiumLibraryBindingsAlreadyInitialized`** —
  root cause: pdfium-render 0.9 enforces a single global binding per process. mdkit's
  `PdfiumExtractor` claims it at engine init. The `max_pages` fast path called
  `Pdfium::bind_to_library` directly — a second claim — which always fails.
  The Sery Link "Extract Content" button worked because it routes through
  `reextract_file` → `extract_document_markdown` → `MDKIT_ENGINE.extract(path)` and
  never calls pdfium-render directly; the dashboard's `files.extract` with `max_pages`
  was the only path that hit this conflict.

  Fix: rewrite `extract_pdf_first_pages` to call `MDKIT_ENGINE.extract(path)` (same
  path as the UI button), then truncate the result to `max_pages` pages using form-feed
  splits (pdfium marks page boundaries with `\x0C`) or a character-count heuristic.
  Removed the direct `pdfium-render` dep from `Cargo.toml` — it remains available as
  a transitive dependency via mdkit. Also removed the now-unnecessary `PDFIUM_LIB_PATH`
  static and the `__scanned_pdf__` sentinel fallback (mdkit handles OCR automatically).

---

## [0.9.7] — 2026-05-19

### Fixed

- **`PDFIUM_LIB_PATH` empty when dashboard calls `files.extract` before any scan** —
  the `max_pages` fast path in `extract_pdf_first_pages` could run before anything had
  touched `MDKIT_ENGINE` (a `Lazy` static). The desktop "Extract Content" button
  goes through the full scan path (which initializes the engine), so it always worked;
  the dashboard API tunnel hits the fast path directly, leaving `PDFIUM_LIB_PATH`
  unpopulated and causing fallthrough to system-path pdfium search (fails — not
  installed system-wide). Fixed by touching `MDKIT_ENGINE` at the top of
  `extract_pdf_first_pages` to guarantee the init block runs before binding is attempted.

---

## [0.9.6] — 2026-05-19

### Fixed

- **Scanned PDF extraction fails with `PdfiumLibraryBindingsAlreadyInitialized`** —
  `extract_pdf_first_pages` tried to re-bind pdfium even though the mdkit engine
  already loaded it at startup. pdfium-render 0.9 only allows one binding per process;
  subsequent attempts return this error. Now handles the error by constructing `Pdfium {}`
  (the already-loaded binding), matching the pattern in pdfium-render's own test helper.

- **Scanned (image-only) PDFs return empty content** — after fixing the binding error,
  pdfium's text layer returns empty strings for every page because scanned PDFs contain
  images, not text. `extract_pdf_first_pages` now detects all-empty output and signals
  the caller to fall through to the full mdkit extraction path, which has Apple Vision /
  Windows OCR wired in via `with_ocr_fallback()`. Scanned PDFs now get OCR'd content
  instead of empty page stubs.

---

## [0.9.4] — 2026-05-19

### Fixed

- **PDF extraction broken in all release builds** — `bundled_resource_dir()` was
  returning `Contents/Resources/` on macOS, but Tauri v2 bundles array-glob resources
  one level deeper at `Contents/Resources/resources/`. The lookup for
  `libpdfium.dylib` silently missed on every GitHub-distributed build (works in
  `cargo tauri dev` because the debug fallback uses `CARGO_MANIFEST_DIR`). Now
  checks `Contents/Resources/resources/` first, falls back to flat layout.

---

## [0.9.3] — 2026-05-19

### Added

- **Partial PDF extraction (`max_pages`)** — `files.extract` now accepts an optional
  `max_pages` integer. When set, pdfium renders only the first N pages and returns
  markdown with `## Page N` headers plus a truncation notice, bypassing the full
  mdkit pipeline. Eliminates timeout failures on large PDFs; the dashboard uses
  `max_pages: 10` for files 5–10 MB so a first preview arrives in under 5 seconds.

### Fixed

- **PDF serial-thread panic no longer aborts the scan** — if pdfium crashes on a
  single file, the scan continues with a shallow record (name, size, format) instead
  of propagating the panic to the entire folder scan task.
- **Sources pushed to cloud on every WebSocket connect** — migrated local-folder
  sources that were created before v0.9.2 now get `AgentSource` records on the
  server immediately, so the dashboard can list and query them without a manual
  rescan.

---

## [0.9.2] — 2026-05-18

### Changed

- **Clean `query_path` schema** — datasets now store a plain absolute path
  (`/Users/foo/file.csv` or `s3://bucket/key`) in the `query_path` column.
  The `local://agent_id/` prefix has been removed from the DB; `agent_id`
  is already a dedicated FK column on every dataset row. The `local://`
  routing form is constructed on-the-fly where needed (WS tunnel, AI tools,
  SQL classifier). An Alembic migration (`040_clean_query_path`) back-fills
  existing rows automatically on next deploy.

### Fixed

- **`files.extract` and `files.list` path parsing** — both commands now accept
  clean absolute paths (`/Users/foo/file.pdf`) in addition to the legacy
  `local://agent_id/path` form, so the dashboard works correctly after the
  `query_path` cleanup without requiring a coordinated client upgrade.

---

## [0.9.1] — 2026-05-18

### Fixed

- **httpfs DRY violation** — two `commands.rs` call sites that inline-executed
  `INSTALL httpfs; LOAD httpfs;` now delegate to `remote::install_httpfs()`,
  the single source of truth for that operation.

### Added (tests)

- **Tunnel SQL rewrite** — unit test verifying `local://agent_id/s3://...`
  references are stripped from SQL before DuckDB execution.
- **Creds key fallback** — unit test confirming that when no source matches
  the remote URL, `find_creds_key` falls back to the URL itself.
- **Creds key matching** — unit test confirming that when an S3 source covers
  the remote URL, `find_creds_key` returns the source root (e.g. `s3://bucket`).

---

## [0.9.0] — 2026-05-17

### Added

- **Agent RPC.** Dashboard can invoke any Sery Link capability over the
  WebSocket tunnel by name (`sources.*`, `files.*`, `sql.*`, `config.*`,
  `system.*`, `agent.*`) and receive streaming progress back.
- **Brand icons for cloud sources.** Drive, Dropbox, B2, Wasabi, R2, and
  GCS show official brand icons; S3, Azure, OneDrive, WebDAV use redrawn
  full-bleed SVGs.
- **S3-compatible preset detection.** Sidebar and Add Source modal pick the
  right icon automatically from the endpoint URL.
- **Remote config sync + dashboard-triggered scans** over the tunnel.
- **mtime-based smart re-scan** — unchanged source trees skip full re-index.

### Fixed

- **Launch crash on macOS (EXC_CRASH / SIGABRT)** — `tokio::spawn` in setup
  closure ran before the Tokio reactor existed; changed to
  `tauri::async_runtime::spawn`.
- **Add Source modal height overflow** on default window size.
- **Internal build paths in OAuth errors** — Drive/Dropbox/OneDrive "not
  configured" messages no longer leak `datalake/SETUP_*.md`.
- **API FK violation on `sync_metadata`** — `source_id` now validated before
  write; dangling references dropped gracefully instead of aborting the batch.

### Removed

- **Machines page** — redundant with the dashboard view.

---

## [0.8.10] — 2026-05-16 — One-click reconnect + machine rename

Auth quality-of-life release. Disconnect no longer clears the workspace
key, reconnect is one click, and machine names update immediately on the
dashboard without a reconnect cycle.

### Added

- **One-click reconnect.** The Connect button in the status bar now checks
  for a saved workspace key. If one exists, clicking Connect silently
  re-establishes the tunnel — no modal, no re-entry of the key.

- **Switch workspace link.** When a saved key is present, a secondary
  "Switch workspace" link appears next to Connect, opening the key modal
  for users who want to join a different workspace.

- **Machine rename syncs to cloud immediately.** Saving a new machine name
  in Settings → General now calls `PATCH /v1/agent/name` in the
  background. The Machines view on the dashboard reflects the new name
  without waiting for the next reconnect.

### Fixed

- **Disconnect kept showing online on dashboard.** The WebSocket reconnect
  loop was a detached tokio task; dropping `WebSocketClient` didn't stop
  it. The task handle is now stored in a static and aborted on disconnect,
  so the API `finally` block fires and Redis clears immediately.

- **Reconnect asked for workspace key again.** The Disconnect button was
  calling `logout` (which clears the keyring). It now calls
  `set_local_only_mode` instead, preserving the saved key for one-click
  reconnect.

## [0.8.9] — 2026-05-16 — Sync reliability: reconcile on connect + hash-based change detection

Sync reliability release. Cloud datasets now stay in sync with local
sources across all offline scenarios, and the fallback rescan runs every
minute with near-zero cost when nothing has changed.

### Added

- **Reconcile on connect.** Every time Sery Link connects to the cloud,
  it sends the current source list to a new `POST /v1/agent/reconcile`
  endpoint. The API deletes any cloud datasets whose source no longer
  exists locally — covering: folders removed while offline, failed
  delete calls, and first connect after migrating from an old install.

- **Hash-based change detection.** Before every fallback rescan, a
  SHA-256 is computed over `(path, size, mtime)` for every scannable
  file in the folder using the folder's configured exclude patterns.
  If the hash matches the last successful sync, the expensive DuckDB
  scan and cloud upload are skipped entirely.

- **1-minute fallback rescan interval** (down from 1 hour). With hash
  detection in place, missed events (iCloud, network drives) are caught
  within 60 seconds and unchanged folders cost only a fast directory
  walk.

- **Single-flight sync guard.** If a sync is already running for a
  folder (e.g. a file-change event fires mid-fallback-scan), the
  second invocation exits immediately rather than stacking work.

### Fixed

- **notify events missed for F42 sources.** File-change events for
  sources added via the new Sources sidebar (`config.sources`) were
  silently dropped — `handle_changes` only checked the legacy
  `watched_folders` list. Both lists are now checked.

- **PDF changes not triggering real-time sync.** `is_data_file` was
  missing `"pdf"`, so PDF modifications were only caught by the
  fallback timer.

- **Spurious re-syncs from excluded directories.** The hash walk
  previously had a hardcoded exclude list that ignored per-folder
  config patterns (e.g. `temp/`, `*.log`). Changes in excluded dirs
  would cause a hash mismatch and trigger a full sync that produced
  no new datasets. The hash now uses the same exclude patterns as the
  scanner.

- **Duplicate source roots on every sync.** After F42 migration, both
  `watched_folders` and `sources` list the same local paths, causing
  duplicate entries in every sync payload. Deduplicated with a HashSet.

- **Config loaded 3× per sync.** `Config::load()` (a TOML file read +
  parse) was called three times per sync cycle. Now loaded once and
  threaded through.

---

## [0.8.8] — 2026-05-16 — Stable machine identity + source detection fixes

Identity and sources reliability release. Introduces a persistent
machine ID so disconnect/reconnect always reuses the same network
record, and fixes source detection on the dashboard for local folders
and S3 resources.

### Added

- **Stable machine ID.** A UUID is generated on first install, stored
  in the OS keyring (survives config deletion), and sent on every auth
  request. The API now looks up agents by machine ID first, then by
  name — so renaming an agent or rotating a workspace key no longer
  creates a duplicate network record on the dashboard.

### Fixed

- **Source roots reported on every sync.** Sery Link now sends the full
  list of configured source roots (`source_roots`) with every cloud
  sync. The dashboard uses this list to show the correct sources
  immediately — no rescan needed.

- **Local folder paths embedded in query_path.** Each dataset's
  `relative_path` is now prefixed with its source folder path so the
  dashboard can map datasets back to individual watched folders
  (e.g. `/Users/foo/Documents/report.csv` instead of `report.csv`).

- **S3 sources no longer wrapped in local://.** Remote source URLs
  (`s3://`, `https://`, etc.) are no longer incorrectly prefixed with
  the local folder path, producing malformed query paths like
  `local://agent_id/s3://bucket/…`.

- **Session expired on API restart.** A single 401 during WebSocket
  reconnect (common while the API is restarting) no longer immediately
  shows "session expired". The reconnect loop now tolerates up to two
  transient auth errors before prompting re-authentication.

- **OAuth redirect goes to production dashboard.** The agent authorize
  endpoint now uses `PUBLIC_DASHBOARD_URL` (e.g. `https://app.sery.ai`)
  instead of defaulting to `http://localhost:3001`.

---

## [0.8.7] — 2026-05-16 — (yanked, superseded by 0.8.8)

---

## [0.8.6] — 2026-05-16 — CI: auto-rebuild website on release

CI patch. Adds a post-release job that triggers a sery.ai website
rebuild automatically so the About page and blog stay current without
a manual deploy step.

### Changed

- Release workflow now fires a `repository_dispatch` to `seryai/website`
  after all platform builds complete, keeping sery.ai in sync with each
  new release.

---

## [0.8.5] — 2026-05-15 — Sidebar restructure + Apple-signed releases

Sidebar polish and distribution fixes. Renames "Results" to "History",
promotes Settings to a top-level nav link, and moves Notifications and
Privacy into a More dropdown. Apple Developer ID signing is now fully
wired through CI.

### Changed

- **Results → History** in the sidebar nav label (route `/results` unchanged).
- **Settings** promoted from the More dropdown to a direct nav link.
- **Notifications** and **Privacy** moved into the More dropdown.
- Release builds are now signed and notarized with Apple Developer ID via
  `APPLE_SIGNING_IDENTITY` / `APPLE_CERTIFICATE` / `APPLE_CERTIFICATE_PASSWORD`
  secrets in GitHub Actions.

---

## [0.8.4] — 2026-05-15 — Tray menu polish + BYOK removal

Patch release. Cleans up the system-tray menu and removes the
long-deprecated BYOK (bring-your-own API key) code path.

### Fixed

- **Tray menu "Open Dashboard" now opens app.sery.ai.** The handler
  was reading `web_url` from the user's config file, which could hold
  a stale `https://sery.ai` from an older install. URL is now
  hardcoded in the handler.

- **Tray menu status dot is colored.** The status line (first item in
  the right-click menu) now shows 🟢 / 🟡 / 🔴 / ⚪ matching the
  connection state. Previously all states rendered as a gray Unicode
  circle because the item was marked disabled.

- **Tray menu shows "Offline" immediately after disconnect.** Clicking
  Disconnect or enabling Local-Only mode now calls `tray::set_state`
  synchronously, so the menu reflects the new state right away instead
  of staying stale at "Connected".

- **"Open Sery in Browser" renamed to "Open Dashboard"** in the
  right-click menu.

- **"Open the dashboard to create one" link in the Connect dialog
  now actually opens the browser.** The `<a href>` tag was silently
  swallowed by Tauri's webview; replaced with `openUrl()` from
  `@tauri-apps/plugin-opener`.

- **Reconnecting… badge is now amber instead of gray** in the
  in-app status bar, making it visually distinct from the
  never-connected (Local only) state.

### Changed

- BYOK (bring-your-own Anthropic/OpenAI API key) support fully
  removed. AI runs cloud-side via the dashboard. The `AuthMode::BYOK`
  variant, deprecated config fields (`selected_byok_provider`,
  `byok_models`), and the "Use My Own API Key" upgrade prompt button
  are all gone. Old audit log entries (`byok_call`) still deserialize
  correctly for users inspecting historic privacy logs.

---

## [0.8.3] — 2026-05-15 — Launchpad fix + proper Apple Developer signing

Patch release. Fixes two macOS distribution issues introduced when
Sery Link first shipped as a signed app.

### Fixed

- **Sery Link now appears in Launchpad.** The app was visible in
  `/Applications` but missing from Launchpad entirely. Root cause:
  `tauri.conf.json` had no `category` field, so Tauri generated an
  `Info.plist` without `LSApplicationCategoryType`. macOS Launchpad
  uses this field to index apps — without it, the app is silently
  skipped. Added `"category": "Utility"` to the bundle config.

  Existing installs can force a Launchpad rescan:
  ```
  defaults write com.apple.dock ResetLaunchPad -bool true && killall Dock
  ```

- **Release builds are now properly signed with Apple Developer ID.**
  The hardcoded `"signingIdentity": "-"` (ad-hoc signing) has been
  removed. CI now reads `APPLE_SIGNING_IDENTITY` from GitHub Secrets
  and signs + notarizes the DMG automatically. Previously distributed
  builds were ad-hoc signed, which triggered Gatekeeper on first
  launch and could prevent LaunchServices from registering the app.

---

## [0.8.2] — 2026-05-14 — Auth + icon polish

### Fixed

- **Silent re-auth for workspace-key users on token expiry.** Users
  authenticating via a workspace key were kicked to the login screen
  when their token expired instead of silently refreshing. The auth
  refresh path now handles workspace-key sessions correctly.

- **App icon inset corrected.** Regenerated all icon variants from the
  rounded source with 9% padding so the mark sits within Apple's
  recommended safe area on all surfaces (Dock, Launchpad, Finder).

---

## [0.8.1] — 2026-05-13 — About panel cleanup

### Fixed

- **Removed internal API endpoint row from the About panel.** The
  About dialog was displaying the raw API base URL, which is an
  implementation detail not useful to end users.

- **App icon padding set to 9%** to match macOS system icon sizing.

---

## [0.8.0] — 2026-05-13 — Analytics + sidebar logo + tunnel fixes

Minor release. Adds privacy-respecting local analytics, replaces the
sidebar text brand with the SVG logo mark, and fixes several auth and
tunnel bugs found during the v0.7 rollout.

### Added

- **Local-first analytics.** Sery Link now sends a daily ping and
  install ID to `analytics.sery.ai` — nothing else. No file paths,
  no query content, no user data. Events queue locally and flush in
  the background; the app works identically if the endpoint is
  unreachable. Narrowed from a broader event system to just
  `install_id + daily_ping` to keep the surface minimal.

### Fixed

- **Sidebar shows the Sery logo instead of text branding.** The
  text "Sery Link" header has been replaced with the SVG mark,
  matching the website and dashboard.

- **`AuthMode` config round-trip no longer crashes.** Loading a
  config file that had `WorkspaceKey` auth mode caused a panic
  ("missing field key") due to a missing serde default. Fixed.

- **Tunnel queries on `local://` sources now resolve correctly.**
  Queries routed through the cloud tunnel to local sources were
  failing because the URL was forwarded un-resolved. The agent now
  resolves `local://` to an absolute path before executing.

- **Workspace-key auth mode persists across restarts.** After pairing
  with a workspace key, the auth mode was reverting to `Cloud` on the
  next launch. The pair flow now writes `WorkspaceKey` to the config
  and triggers a heartbeat immediately after pairing.

- **CI: Apple notarization now succeeds end-to-end.** Pre-signs
  bundled third-party binaries (pandoc, libpdfium) before tauri-action
  bundles them — notarization rejected the entire app when these
  arrived unsigned.

---

## [0.7.8] — 2026-05-10 — Crash + CI fixes, brand polish

Patch release. Fixes a recurring crash that killed the app during
folder rescans, restores macOS DuckDB extension loading, brings the
app icon onto Apple's standard size template, fixes red CI, and
brand-aligns purple to match the website + dashboard.

### Fixed

- **Rescan no longer crashes the app.** Concurrent `rescan_folder`
  calls (UI double-click, auto-scan + manual scan, watcher
  retrigger) raced two DuckDB transactions writing the same
  metadata row. The second commit hit DuckDB's index-validation
  path and threw a FatalException — a C++ exception that crossed
  the FFI boundary unhandled and `terminate()`'d the whole
  process. Tray, watcher, window all died.

  Three layered fixes:
  1. `metadata_cache::upsert_dataset` no longer mutates the
     PRIMARY KEY on conflict (which was triggering DuckDB's
     index-rebuild path)
  2. Switched from `ON CONFLICT(...) DO UPDATE` to explicit
     `DELETE + INSERT` inside a transaction (DuckDB 1.1 rejects
     `INSERT OR REPLACE` when the table has multiple unique
     constraints)
  3. Added a process-wide static `Mutex` around upsert calls
     so concurrent rescans serialize their commits instead of
     racing them
  4. `upsert_many` reuses a single transaction for the whole
     batch instead of opening one per row — fewer commits to
     contend, much faster bulk syncs

- **DuckDB extensions actually load on macOS.** Sync was crashing
  on first cloud-storage source with `code signature ... not valid
  for use in process: mapping process and mapped file (non-platform)
  have different Team IDs`. Hardened Runtime's library validation
  refuses any dylib whose Team ID differs from the host process,
  but DuckDB downloads its own httpfs / parquet extensions signed
  by DuckDB Labs. Added an `Entitlements.plist` that opts out:
  `cs.disable-library-validation`, `cs.allow-unsigned-executable-
  memory`, `cs.allow-dyld-environment-variables`,
  `network.client`, `files.user-selected.read-only`. Wired into
  `tauri.conf.json` `bundle.macOS.entitlements` so codesign picks
  it up for both ad-hoc dev and notarized release builds.

- **CI builds green.** Two fixes:
  1. `scan_cache` tests use a per-test `tempfile::TempDir` so
     concurrent test runs and stale `*.db.wal` files don't
     trip "Table already exists" during DuckDB WAL replay
  2. `actions/checkout`, `actions/setup-node`, `pnpm/action-setup`
     bumped from `@v4` → `@v5` (Node 20 deprecation in mid-2026)

### Changed

- **App icon follows Apple's macOS Icon Template.** The 1024×1024
  source PNG used to fill edge-to-edge, making the icon visibly
  outsize every other app in the Dock + Launchpad. Now: 824×824
  rounded body inset 100px on each side, matching the system
  template every Apple-shipped app uses. Mark scaled to 60% of
  the body (≈18.75% padding inside, HIG sweet spot). Regenerated
  the entire platform icon family via `cargo tauri icon` (icns +
  ico + 32/64/128/128@2x + iOS app-icon family + Android mipmap
  launchers + Microsoft Store / Windows tile family). Tray +
  titlebar icons unchanged (mark-only on transparent bg, no
  rounded body).

- **Brand purple aligned across the in-app UI.** Audit found 400+
  uses of Tailwind's default `purple-{500..900}` (`#9333ea`-ish
  bright violet) and zero uses of the canonical brand `#5b3ea3`.
  Override Tailwind's `purple` palette in `tailwind.config.js` so
  every existing `bg-purple-600` etc. resolves to brand-aligned
  values without changing JSX. Now matches the website + dashboard.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>

## [0.7.7] — 2026-05-10 — Brand alignment

Patch release. Visual refresh that aligns Sery Link's chrome with the
website + dashboard. No behavior changes.

### Changed

- **App icons regenerated** from the canonical 8-petal mark
  (`src/assets/sery-mark.svg`). White mark on a #5b3ea3 → #7c3aed
  purple gradient, with 18.75% inset matching Apple HIG / Material
  guidance. The full platform family — `icon.icns` (macOS),
  `icon.ico` (Windows), 32/64/128/128@2x PNGs (Linux), the iOS app
  icon family (15 files), Android launcher mipmaps (15 files), and
  the Microsoft Store / Windows tile family — all regenerated via
  `cargo tauri icon`.
- **In-app logo** (`src/assets/sery-logo.svg`) — drop-in replacement.
  The OnboardingWizard renders the new mark in solid #5b3ea3 without
  any code change.
- **System tray icon** (`tray-{22,44,64}.png`) — monochrome black
  silhouette of the new mark, ready for macOS template treatment
  (`icon_as_template=true` auto-tints for dark/light mode).
- **Titlebar icon family** (`titlebar-{16,32,64,128}.png`) — solid
  #5b3ea3 mark on transparent. Used by OS surfaces that provide
  their own chrome (window title bar accents, notification badges).
- **Window icon** (`window-icon.png`) — full-color composite at
  256×256, matches the dock icon at smaller scales.
- **Dev-server preview** title and favicon updated. `index.html`
  title `Tauri + React + Typescript` → `Sery Link`; favicon now
  points at the Sery purple `/favicon.svg`.
- **Removed** vestigial `public/vite.svg` + `public/tauri.svg`
  (create-tauri-app template artifacts; never referenced by app
  code).
- **Reproducible build script.** `scripts/build-brand-assets.mjs`
  regenerates every hand-rendered icon (in-app logo + Tauri 1024
  source PNG + window-icon + tray-{22,44,64} + titlebar-{16,32,
  64,128}) from the single canonical mark SVG. After a brand-color
  shift, re-run with:
  ```
  node scripts/build-brand-assets.mjs
  cargo tauri icon src-tauri/icons/_source-1024.png
  ```
- `sharp` added as a devDependency for the build script.

### Fixed

- **Test build green again.** Four `WatchedFolder` literal-construction
  sites in test fixtures (`export_import.rs:222/298/325`,
  `sources.rs:375`) were missing the `last_synced_to_workspace_id`
  field that landed during the catch-up-sync work. Production
  construction sites had been updated; the test-only ones broke
  `cargo test` compilation on CI. Plugged with `None` defaults.

## [0.7.6] — 2026-05-06 — Catch-up sync visibility + privacy deep-link

Patch release. Two small follow-ups to the catch-up sync flow that
landed in v0.7.2/0.7.3.

### Added

- **Top-level "Syncing N of M…" pill in the StatusBar.** When
  `catch_up_sync` runs in the background (e.g. user clicked Sync
  in the catch-up dialog and dismissed it), the StatusBar shows
  the live folder-by-folder progress instead of the static "N to
  share" badge. Driven by a new `catch_up_progress` Tauri event
  emitted at the start of each folder + once at the end of the
  batch. Closes the gap where a user who closed the dialog had no
  top-level signal that work was still in flight.

### Fixed

- **Privacy "What gets uploaded?" deep-link.** The catch-up
  dialog's link previously dumped users at the top of /privacy.
  Now it deep-links to `#machines-and-datasets` — the section
  that actually answers what's stored and what isn't. Pairs with
  the matching anchor on the website side.

## [0.7.5] — 2026-05-06 — scan_status replay on reconnect

Patch release. Pairs with the api-side scan-error alerts shipped at
the same time.

### Fixed

- **scan_status replay on reconnect.** A WebSocket blip mid-scan
  (network hiccup, AuthExpired, server restart) used to leave the
  cloud dashboard pill blank until the next 30s keepalive tick.
  The scan_status snapshot now lives on a websocket-module-level
  `LAST_SCAN_STATUS` global, and `connect_and_run` replays the
  snapshot immediately after publishing the writer. Cleared on
  scan end (idle/error) so a stale "scanning" payload can never
  be replayed after the scan finishes.

## [0.7.4] — 2026-05-06 — scan_status keepalive on slow scans

Patch release. Fixes an edge case in the v0.7.1 scan_status pipeline:
during long extractions on a single file (e.g. a 10 GB CSV that
takes >60 s to read), no `progress_cb` fires, the cloud-side Redis
TTL (60 s) expires, and the dashboard pill blinks out mid-scan.

### Fixed

- **scan_status TTL keepalive.** `rescan_folder` now keeps a live
  snapshot of the latest reported state and a 30 s background tick
  that re-emits it. The keepalive runs alongside the existing 2/sec
  throttled emissions on `walk_progress` / `progress` callbacks, so
  the cloud pill stays accurate during fast and slow phases alike.
  RAII guard cancels the keepalive when `rescan_folder` returns
  (success, error, panic).

## [0.7.3] — 2026-05-06 — Catch-up state machine + reachable post-dismissal

Patch release that closes two holes in the v0.7.2 catch-up flow.

### Fixed

- **Catch-up dialog re-fired on every reconnect.** v0.7.2 listed
  every watched folder with a local scan baseline, so a machine
  reboot or auth blip would re-prompt the user to sync data already
  in the workspace. Now `WatchedFolder` carries
  `last_synced_to_workspace_id`, set after `sync_metadata_to_cloud`
  succeeds. `list_catch_up_folders` skips folders already synced to
  the *current* workspace; switching workspaces still surfaces them
  (correct — that's a different workspace).
- **No way back to catch-up after "Not now."** The auto-prompt was
  the only entry point; users who dismissed it had no path back
  short of disconnecting and reconnecting. Added a "N to share"
  pill in the StatusBar (visible whenever `list_catch_up_folders`
  returns non-empty) that opens the same dialog standalone.

### Refactored

- Phase-2 catch-up view extracted from `ConnectModal` into a
  standalone `CatchUpDialog` component so both entry points (post-
  connect prompt + StatusBar follow-up) reuse the same UI without a
  fork.

## [0.7.2] — 2026-05-06 — Catch-up sync after first connect

Patch release. Pairs with v0.7.1's live `scan_status` plumbing and
closes the workflow loop: when a user adds folders in local-only
mode and *later* enters a workspace key, ConnectModal now offers a
"Connected. One more step?" follow-up dialog listing the locally-
indexed folders with per-folder checkboxes (default all on) and a
single Sync button. The cloud dashboard's `scan_status` pill renders
the catch-up progress in real time.

### Added

- **`list_catch_up_folders` Tauri command** — returns watched
  folders that have a local scan baseline (`last_scan_stats`) for
  the connect-time UI to surface.
- **`catch_up_sync(paths)` Tauri command** — sequentially re-runs
  `rescan_folder` for each path. Sequential by design — desktops
  don't benefit from parallel folder scans.
- **Phase-2 view in ConnectModal** (frontend) — appears post-auth
  when the catch-up list is non-empty. Per-folder checkboxes,
  privacy reminder ("only metadata uploaded — file contents stay
  on this machine") with a link to /privacy, and Sync /
  Not now buttons. Sync fires `catch_up_sync` and dismisses; the
  user keeps using the app while the background catch-up runs.

## [0.7.1] — 2026-05-06 — Live scan progress in the cloud dashboard

Patch release. Sery Link now reports its scanner state to Sery.ai
over the existing tunnel, so the dashboard can replace its
"Open Sery Link" empty-state nag with a live "Indexing 1,243 / 8,000
files…" pill while a scan is in flight. Pairs with matching api
and dashboard updates that landed at the same time.

### Added

- **`scan_status` over the agent tunnel** — `rescan_folder` emits
  `scanning` → throttled progress → `idle` (or `error`) so the
  cloud dashboard's `/network/[id]` pill and chat empty-state
  banner can react in real time. Throttled to ~2 emissions/sec
  across walk + content-extraction phases.
- **Best-effort outbound channel** in `websocket.rs`
  (`send_outbound_json`) — non-WS code can publish status frames
  on the active tunnel. Sends are no-ops when the tunnel is
  offline (no buffering, by design — the next progress tick
  resyncs on reconnect).

## [0.7.0] — 2026-05-05 — Sources sidebar + 5 new storage protocols

A two-part release. **Part one (F42)** rebuilds the sidebar around a
unified `Sources` surface so every connected place — local folders,
S3 buckets, HTTPS URLs, Drive accounts, future protocols — appears
as one bookmark row with consistent right-click ops. The marketing-
page promise made real, and the foundation every protocol adapter
plugs into. **Part two (F43–F49)** ships five genuinely new storage
protocols on top of that foundation: SFTP, WebDAV, Dropbox, Azure
Blob, and OneDrive, plus four S3-compatible presets (B2 / Wasabi /
R2 / GCS).

Picker is **feature-complete**: **9 implemented** (Local · HTTPS ·
S3 · Drive · SFTP · WebDAV · Dropbox · Azure Blob · OneDrive) +
**4 S3-compatible presets** + **0 coming soon**.

See `datalake/SPEC_F42_SOURCES_SIDEBAR.md` for the foundation
design.

### F42 — Sources sidebar foundation

- **`/sources` route + Sources sidebar** (`SourcesSidebar`) with
  kind icon, name, protocol label, dataset count, and live status
  pill (scanning / online / pending — driven by `scansInFlight`).
  Coexists with the legacy Folders tab for v0.7.0; FolderList comes
  out in v0.7.1.
- **Right-click context menu**: Rescan now / Rename… / Edit
  credentials… (kinds with creds) / Move to group… / Show in
  Finder (Local) / Remove source. Rename is inline (Finder-style:
  Enter commits, Esc cancels, blur commits). Move-to-group opens
  a real picker with chip-style options for existing groups + an
  inline "Create new" input.
- **Drag-reorder via @dnd-kit/sortable**. Each visual bucket — the
  ungrouped section and each named group — is its own DndContext,
  so drag reorders within a bucket. Cross-bucket moves go through
  "Move to group…". Pointer + keyboard reorder both supported.
- **`+ Add source` button + `AddSourceModal`** unified entry. The
  protocol picker shows every kind Sery Link can register; clicking
  a tile transitions to the kind-specific form INLINE in the same
  modal — no jolt-handoff. Initial scan auto-kicks in the
  background after add.
- **"Scan all" button** in the sidebar header for bulk refresh.
- **`Config::load` migration**: existing v0.6.x users see their
  `watched_folders` auto-populated into the new `sources` Vec on
  first load. Source IDs survive subsequent loads (load-bearing for
  keychain key / cache prefix / deep links). Incremental migration
  picks up entries added via legacy `add_watched_folder` /
  `add_remote_source` so the sidebar stays in sync. `watched_folders`
  stays written for one release for rollback safety; v0.7.1 stops
  writing it.
- **`remove_source` dual cleanup**: drops the mirror watched_folders
  entry + keychain creds + scan cache + (for cache-and-scan kinds)
  the SFTP / WebDAV / Dropbox / Azure keychain entry too. Prevents
  the incremental migration from resurrecting the source with a
  fresh UUID on next load.

### F43 — SFTP

- `SourceKind::Sftp { host, port, username, base_path }` variant.
- Auth: password OR SSH private-key path (with optional passphrase).
- Backed by `ssh2` (libssh2) with vendored OpenSSL — no
  `brew install libssh2` needed for release builds.
- Pre-flight: real handshake + SFTP-channel-open before save —
  bad host / bad creds / SFTP-disabled-on-server all surface
  inline, not as silent empty rescan minutes later.
- Rescan: walks `base_path` (10k file cap), filters to scanner-
  supported extensions, downloads via streaming chunks to
  `~/.seryai/sftp-cache/<source_id>/`, then runs the existing local
  scanner against the cache. Per-file failures logged + skipped;
  the rest of the tree is still useful.

### F44 — WebDAV

- `SourceKind::WebDav { server_url, base_path }` variant.
- Auth: Anonymous, Basic (Nextcloud / ownCloud app passwords), or
  Digest (legacy servers).
- Backed by `reqwest_dav` — wraps reqwest with PROPFIND + multistatus
  XML parsing. Compatible with our existing reqwest 0.11.
- Pre-flight: PROPFIND Depth=0 against the server root.
- Rescan: PROPFIND Depth=Infinity, file-only filter, mirror-download
  to `~/.seryai/webdav-cache/<source_id>/`.

### F45 — S3-compatible endpoints

- `S3Credentials` gains optional `endpoint_url` + `url_style` fields
  (`serde(default)` + `skip_serializing_if = None` for back-compat).
- `duckdb_setters` emits `SET s3_endpoint='…'` and
  `SET s3_url_style='…'` when present. Endpoint is scheme-stripped
  before emit (DuckDB rejects values with `https://`).
- 4 new picker presets unlock 4 of the 7 previous "Coming soon"
  tiles via DuckDB's existing S3 client: **Backblaze B2**, **Wasabi**,
  **Cloudflare R2**, **Google Cloud Storage** (S3 interop).
- Manual endpoint disclosure in the AWS creds form for any other
  S3-compatible service (MinIO, SeaweedFS, etc.).
- Endpoint editor added to the existing "Edit credentials…" dialog
  for S3.

### F46 — Azure Blob Storage

- `SourceKind::AzureBlob { account_url, prefix }` variant.
- Auth: SAS token (Shared Access Signature). Long-lived, scopable,
  least-privilege. Storage account keys + Azure AD OAuth deferred.
- Backed by `roxmltree` for parsing the `EnumerationResults` XML.
  Tiny pure-rust parser, ~25 kB compiled.
- SAS token normalisation: leading `?` stripped so users can paste
  either the bare query string or the full token from the portal.
- Rescan: List Blobs + paginate via `<NextMarker>`, mirror-download
  to `~/.seryai/azure-cache/<source_id>/`.

### F48 — Dropbox

- `SourceKind::Dropbox { base_path }` variant.
- **Two auth shapes**, user-toggleable in the Add Source modal:
    - **OAuth (default)** — Connect-with-Dropbox via PKCE
      no-redirect flow. User clicks "Open Dropbox", signs in,
      pastes the code Dropbox shows back into the app. Tokens
      auto-refresh ~60s before expiry.
    - **Personal Access Token (fallback)** — user generates a
      token at `dropbox.com/developers/apps`. No expiry to
      manage; useful for self-hosters or users on builds that
      ship without a real `DROPBOX_APP_KEY` baked in.
- `DropboxCredentials` carries `access_token` + optional
  `refresh_token` + `expires_at`; PAT entries leave the optional
  fields `None`. Backward-compatible deserialization for
  pre-OAuth keychain entries.
- `ensure_fresh` helper rotates expiring OAuth tokens before
  every rescan and persists the rotated tokens. PAT entries are
  a no-op.
- Backed by direct Dropbox HTTP API calls via reqwest (no extra
  crate). Cursor pagination via `/files/list_folder/continue`;
  Dropbox quirks handled (root is empty string not "/", path_lower
  used as the stable per-file key).
- Rescan: list + mirror-download to
  `~/.seryai/dropbox-cache/<source_id>/`.
- **Manual ops step before OAuth works:** `DROPBOX_APP_KEY` env
  var must be set at `cargo build` time. Builds without it leave
  the OAuth tab functional but surface a clear "not configured"
  error when the user clicks "Open Dropbox" — PAT still works as
  the fallback. See `datalake/SETUP_DROPBOX_OAUTH.md` for app
  registration. Settings → About surfaces the configured/not-set
  state at runtime.

### F49 — OneDrive

- `SourceKind::OneDrive { base_path }` variant.
- Auth: **device code OAuth flow**. App requests a code from
  Microsoft; the modal shows it big-and-monospaced; user opens
  `microsoft.com/devicelogin` in any browser, enters the code,
  signs in. App polls until completion. Avoids the deep-link
  callback complexity of full PKCE while still being a first-class
  Microsoft auth grant.
- Backed by direct Microsoft Graph API calls via reqwest. BFS over
  folders via `/me/drive/root:/<path>:/children`, follows
  `@odata.nextLink` for pagination. Refresh-on-expiry runs
  automatically before each Graph call (~60s pre-expiry to dodge
  race conditions). Honors Microsoft's token rotation when a new
  refresh_token is issued.
- access_token + refresh_token + expires_at all live in the OS
  keychain via `onedrive_creds`, keyed on source_id.
- Rescan: walk + mirror-download to
  `~/.seryai/onedrive-cache/<source_id>/`. Manifest key = item id
  (stable across renames).
- **Manual ops step before this works:** `MICROSOFT_CLIENT_ID`
  must be set at `cargo build` time (option_env!). Same pattern
  as `GOOGLE_OAUTH_CLIENT_ID` and `DROPBOX_APP_KEY`. Builds
  without it return a clear configuration error from
  `start_device_code_flow`. See `datalake/SETUP_MICROSOFT_OAUTH.md`
  for Entra app registration. Settings → About surfaces the
  configured/not-set state at runtime.

### Incremental sync (all 5 cache-and-scan kinds)

- New `sync_manifest.rs` module owns a JSON file at
  `<cache_dir>/.sery-manifest.json` keyed on protocol-stable
  per-file ids → `{ size_bytes, mtime_marker }`.
- Every `walk_and_download` (SFTP / WebDAV / Dropbox / Azure /
  OneDrive):
    1. Loads the manifest (default empty on missing/corrupt — safer
       to redownload than serve stale data).
    2. Skips download when manifest entry matches AND local cached
       file still exists.
    3. Records every successful download.
    4. Drops manifest entries (and deletes their local cache files)
       for remote paths no longer present — removed-from-server
       files don't linger in scan results.
    5. Saves the updated manifest.
- Turns repeat rescans from "wait every time" into "wait once,
  ~instant for unchanged files."

### Edit credentials… for SFTP / WebDAV / Dropbox / Azure

- New unified `EditCredentialsDialog` switches form per kind.
- 8 new Tauri commands: `get_*_credentials_for_source` +
  `update_*_credentials` for each of the 4 kinds. Each
  `update_*` re-runs the protocol's pre-flight before persisting.
- Existing S3 dialog stays — its shape is different (works on URL
  not source_id) and predates the cache-and-scan generalisation.
- SFTP edit form: host/port/username read-only; only the auth
  payload (password or SSH key) is editable. Endpoint changes go
  through Remove + Re-add.
- **Dropbox OAuth re-auth** — when the loaded entry is OAuth-shaped
  the form opens on a "Sign in with Dropbox" tab that re-runs the
  PKCE flow against the existing source_id; tokens rotate without
  losing the source's name / group / sort_order / scan-cache. The
  user can also toggle to PAT mode to switch auth styles.
- **OneDrive Re-authorize…** is a separate context-menu entry
  (`ReauthOneDriveDialog`) that re-runs the device code flow.
  Auto-triggers on auth-shaped rescan errors so users don't have
  to discover the menu. Dropbox's auth-shaped errors auto-open
  the Edit credentials dialog on the OAuth tab — same pattern.

### Concurrent downloads (all 5 cache-and-scan kinds)

- All 5 cache-and-scan walks (SFTP / WebDAV / Dropbox / Azure /
  OneDrive) now run downloads concurrently — up to 4 in flight
  per walk. Pre-pass classifies files into needs-download vs
  skipped-by-manifest; skipped files emit progress upfront, the
  download queue runs concurrently behind a shared
  `Arc<Mutex<>>`-protected manifest + atomic counters.
- 4 async modules use `futures::stream::for_each_concurrent` over
  a single shared session (Dropbox / Azure / WebDAV reqwest /
  OneDrive Graph). `WalkProgressCb` is `Arc<dyn Fn>` so the
  callback clones cleanly across tasks.
- SFTP uses 4 OS threads with one libssh2 session each (libssh2
  channels can't multiplex). Worker connect failures are
  individually non-fatal (siblings carry the load); if all 4
  workers fail the rescan returns the last connect error rather
  than silently succeeding with 0 downloads.

### Per-byte progress for large files

- `download_*` functions across all 5 kinds accept an optional
  `ByteProgressCb`. The walker only wires it in for files larger
  than 10MB and throttles emissions to 5% boundaries — the
  per-file label sent to `scan_progress` becomes
  `"filename.parquet (45%)"` while the download is in flight,
  back to `"filename.parquet"` at the per-file post-callback.
- Frontend renders the new label text in the existing FolderList
  scan card without changes. Avoids the "did it freeze?" feel on
  multi-GB downloads.
- WebDAV-Digest still uses the buffered fallback (no streaming →
  no per-byte progress); a small minority of legacy WebDAV
  servers, considered acceptable.

### OAuth providers — build-time env var pattern

- All three OAuth providers now share the same `option_env!`
  build-time pattern with consistent error messages:
    - Google Drive: `GOOGLE_OAUTH_CLIENT_ID` (existing)
    - Dropbox: `DROPBOX_APP_KEY` (NEW)
    - OneDrive (Microsoft Entra): `MICROSOFT_CLIENT_ID` (NEW —
      drops the hardcoded const + "REPLACE_WITH_REAL_APP_ID"
      placeholder)
- Each rejects empty values AND historical placeholder strings,
  so a stale value in the env shows as "not configured" instead
  of failing later with a confusing 401 from the vendor.
- **Settings → About** surfaces a diagnostic block when at least
  one provider's env var is missing — names the provider + the
  env var the maintainer needs to set. Hidden when all three are
  configured (production builds show nothing extra).

### Fixed

- **`remove_source` dual cleanup** — see F42 above; was a real bug
  where Remove + the migration round-trip resurrected the source
  with a fresh UUID.

### Tests

- **273 sery-link Rust lib tests green** (up from 191 pre-F42).
  Per-protocol coverage:
    - `sftp::tests` — 8 (creds validation, password / SSH key,
      serde tagged enum, default port, tilde expansion).
    - `webdav::tests` — 5 (anonymous / basic / digest validation,
      tagged enum shape, missing url rejection).
    - `azure_blob::tests` — 10 (creds validation, SAS normalisation,
      list URL construction, blob URL encoding, full XML parser
      coverage including pagination + bad root rejection).
    - `dropbox::tests` — 4 (creds validation, list_folder JSON
      parse with file / folder / deleted entries).
    - `onedrive::tests` — 7 (creds validation, expiry detection
      with parseable/unparseable/past/future timestamps, Graph
      children response parse with files + folders + pagination
      link).
    - `sync_manifest::tests` — 9 (unknown / matched / size-changed
      / mtime-changed needs_download paths, drop_missing,
      save+load round-trip, missing-file + corrupt-file
      graceful-empty fallbacks).
    - `remote_creds::tests` — 5 added for F45 endpoint behavior.
    - `config::tests` — extended for migration + mutation +
      mirror-cleanup invariants.
- TypeScript + vite production build clean on every merge.

### Out of scope for v0.7.0

- **WebDAV Digest streaming** — Digest auth requires a manual
  challenge/response implementation; reqwest doesn't ship one.
  Falls back to the buffered path for the small minority of
  legacy WebDAV servers using Digest. No per-byte progress for
  those downloads.
- **Per-source-kind concurrency knob** — `MAX_CONCURRENT=4` is
  hardcoded across all 5 protocols. Settings UI to tune
  per-kind concurrency is deferred until users hit either the
  rate-limit ceiling or the home-bandwidth ceiling in practice.
- **Drive scan via DataSource** — still walks via `gdrive_walker`;
  `scan_source(GoogleDrive)` returns `Ok(vec![])` pending the
  adapter rewire.

## [0.6.3] — 2026-05-01

Audit-driven cleanup release. After the post-website-rewrite UI
audit (datalake/UI_AUDIT_2026_05.md) flagged a handful of places
where the desktop app contradicted or under-surfaced the new
marketing copy, this release closes 6 of 7 audit items.

### Fixed

- **BYOK render path stripped from Privacy → Activity.** Pre-v0.6
  audit logs still contained `byok_call` entries which rendered
  with their own row component + a "BYOK calls" totals card,
  contradicting the v0.6.0 BYOK-removal claim. The render path
  is gone (component deleted, totals card dropped); legacy
  entries on disk are filtered out at render time but preserved
  in the JSONL for users who want to inspect via "Reveal audit
  file." Net: -50 LoC in `Privacy.tsx`. (Audit B1.)
- **Privacy disclosure card now distinguishes local vs cloud
  scope.** "Results of queries you run" used to lump every
  query together — a false statement for local-only users
  whose preview / profile / search activity never leaves the
  machine. Cards rewritten to enumerate items by surface
  (catalog sync, AI chat, workspace events) and to explicitly
  mark local browse / preview / profile / search as
  device-only. (Audit B2.)

### Changed

- **Add Source modal shows the full protocol roadmap.** New
  ProtocolRoadmapGrid above the existing tabs lists all 11
  storage protocols with "Now" / "v0.7+" status badges. The 4
  shipped (Local, HTTPS, S3, Drive) get emerald badges; the 7
  coming (SFTP, WebDAV, B2, Azure, GCS, Dropbox, OneDrive) get
  muted "v0.7+" badges. Tiles are informational until F42 ships
  the unified protocol picker. (Audit I1.)
- **Onboarding wizard surfaces Convert-to-Parquet.** One-line
  tip on the welcome screen so users with piles of CSVs know
  the conversion feature exists before they discover it
  accidentally. (Audit I2.)
- **Folder list nudges MCP discoverability.** When ≥1 folder
  is being watched, the header subtitle now mentions exposing
  via Settings → MCP. Subtle, dismissable-by-removing-folders.
  (Audit I4.)
- **Search page surfaces the Cloud workspace upgrade.** New
  dismissable card above search results when local-only and
  ≥1 result returned. Explains the $19/mo workspace + AI chat
  upgrade and links to app.sery.ai/settings/workspace-keys.
  Dismissal persists via localStorage; doesn't nag. (Audit I5.)

### Deferred

- **Settings → Privacy → Stored Credentials inspection panel.**
  Tracked as audit I3. Needs cross-platform Rust work for
  keychain enumeration that's beyond a 1-day scope. Punted to
  the next sprint.

### Verification

- TypeScript `tsc --noEmit` clean.
- Rust `cargo check --lib` clean (no Rust changes in this
  release; type errors would have surfaced on dependent fields
  if Privacy.tsx's audit-entry shape changed, but the
  AuditEntry type itself is unchanged).
- Manual click-through QA: founder pass against a real install
  before tagging.

## [0.6.2] — 2026-05-01

Cleanup release — four pending branches merged after dogfooding.

### Fixed

- **CSV / Parquet preview rows rendered in the top-left corner**
  of the page instead of inside the Data preview card. The
  virtualizer was using `position: absolute` rows inside a
  `<tbody>` set to `position: relative`, but `display:
  table-row-group` doesn't reliably create a positioning context.
  Switched to top/bottom spacer rows (standard tanstack-virtual
  table pattern); column widths now stay in sync with `<thead>`
  for free.

### Added

- **S3 add modal pre-flights credentials** before persisting
  anything. Bad keys, wrong region, or wrong bucket surface as an
  inline error on the modal where the user can fix them, instead
  of as a silent or empty rescan minutes later. Failures bypass
  `remote_creds::save` so a bad attempt leaves no orphan keychain
  entry.

### Changed

- **Sidebar hides Machines and Recipes when not connected to a
  workspace.** Both pages were dead-end "Connect to see your
  machines" empty states in local-only mode. Find / Folders /
  Results / Notifications stay visible always (each has
  local-relevant content). Routes still resolve, so deep links
  keep working.
- **Keychain reads are cached in-process for the session.** macOS
  used to prompt twice at startup (`get_auth_mode` called
  `has_token()` then `get_token()`) and then re-prompt for
  Drive / S3 creds on every navigation. Now: one prompt per
  keychain item per launch, then silent. Save / delete invalidate
  the cache so it can never go stale.

## [0.6.1] — 2026-05-01

S3 listing now actually finds the user's files.

### Fixed

- **S3 prefix scans were missing nested data.** The default
  listing pattern was `<prefix>/*.{csv,parquet}` — one level deep
  and dependent on DuckDB-httpfs brace expansion that was observed
  silently returning empty even with matching keys present. A user
  added an S3 prefix that contained many CSV and Parquet files in
  sub-folders and got "added with nothing" because none of those
  paths matched. Two changes:
  - Default listing is now `<prefix>/**/*` (recursive, no brace).
  - Extension filtering (csv / tsv / parquet) happens Rust-side on
    the listed object URLs, sidestepping brace expansion entirely.
  - Capped at 10,000 listed objects per scan; explicit globs let
    the user narrow further.
- **Empty S3 listings now error instead of silently succeeding.**
  Previously the scanner returned `Ok(vec![])` for a zero-match
  glob and the UI showed "S3 source added" with no datasets and
  no diagnostic. Now the scan errors with the actual pattern and
  a hint about region / credentials / explicit glob, surfaced as
  a sync_failed toast.
- **PDF preview** rendered the row-preview error ("can't preview
  rows for pdf files") instead of the document text panel. The
  FileDetail document-format set was missing `pdf` while the
  scanner already treated PDFs as documents.

## [0.5.2] — 2026-04-30

### Fixed

- **macOS "Sery Link is damaged and can't be opened" alert.** v0.5.1
  shipped without any code-signing on macOS, which on Sequoia + a
  quarantined download triggers a stricter alert than the documented
  Gatekeeper override. Added `signingIdentity: "-"` to
  `tauri.conf.json` `bundle.macOS` so Tauri ad-hoc-signs the bundle
  during build. This shifts most users from "is damaged → dead end"
  to "cannot be opened → System Settings → Open Anyway", which IS
  documented and overridable.
- The `xattr -dr com.apple.quarantine /Applications/Sery\ Link.app`
  workaround still works for users who hit the stricter case
  (older download paths, certain Sequoia configurations). See the
  [Install section in RUNBOOK.md](https://github.com/seryai/sery-link/blob/main/RUNBOOK.md#install)
  for the full per-OS override flow.

The real fix (Apple notarization) lands when Developer enrollment
succeeds — both warnings disappear at that point. Tracked in
DECISIONS.md (2026-04-29 "Ship v0.5.1 macOS unsigned + un-notarized").

## [0.5.1] — 2026-04-30

The first public release. Functionally identical to the unreleased
v0.5.0 build; the version bump signals the clean-history reset.

### Changed

- Repository made public on GitHub under AGPL-3.0
  ([github.com/seryai/sery-link](https://github.com/seryai/sery-link)).
- Internal strategy + UX docs (`docs/internal/`) and icon-generation
  scripts (`docs/scripts/`) relocated to the private monorepo. Public
  repo now ships only docs intended for external readers (README,
  RUNBOOK, RELEASE, SECURITY, CONTRIBUTING, SUPPORT, CHANGELOG).
- Production URL defaults baked into binaries via `option_env!`
  (`api.sery.ai` / `wss://api.sery.ai` / `app.sery.ai` instead of
  localhost). Devs override with `SERY_API_URL` / `SERY_WEBSOCKET_URL`
  / `SERY_WEB_URL` env vars at build time.
- Release workflow updated: Apple code-signing is now optional (gated
  by `APPLE_CERTIFICATE` secret being non-empty); curl fetch scripts
  use `--retry-all-errors` for resilience to transient network blips.
- Minisign keypair rotated; new public key in `tauri.conf.json` updater
  config.

## [0.6.0] — 2026-05-01

The file-manager pivot. Sery Link is now a local file manager: it
indexes, previews, converts, and exposes data through the workspace
tunnel. AI lives in the cloud dashboard at app.sery.ai/chat — every
question runs server-side and fans out to your machines via the
existing tunnel, so there's a single answer surface across every
device you've connected.

### Removed

- **BYOK (paste-your-own LLM key) and the in-app `/ask` tab.** Three
  releases of trying to run a text-to-SQL agent on the desktop with
  user-supplied keys produced answers that were good enough to demo
  and unreliable enough to ship. Cloud `/chat` already had a working
  tool-use agent with cross-machine fan-out — running a parallel,
  weaker copy on every desktop install was net negative. ~1100 LoC
  stripped from `src-tauri/` and the Settings panel; the keychain
  entries from earlier installs are left in place (harmless), and
  `config.json` keeps `selected_byok_provider` / `byok_models` as
  deprecated fields slated for removal in v0.7.0.
- The `/ask` route in the desktop UI is now a placeholder that links
  to `${SERY_WEB_URL}/chat`. The Quick-Ask hotkey (⌘⇧S) routes to
  `/search` unconditionally — there is no longer a BYOK-vs-search
  branch.

### Added

- **Inline tabular preview** on the file detail view. Virtualized
  table via `@tanstack/react-virtual`, capped at 5000 rows, reads
  the file in-process via tabkit. No DuckDB round-trip, no network.
- **"Convert to Parquet"** button on CSV / TSV / Excel files. Writes
  the Parquet next to the source with collision-safe naming
  (`foo.parquet`, `foo (1).parquet`, …) via DuckDB COPY with a
  permissive read_csv_auto fallback ladder for malformed inputs.
- **Folder-level filter chips** — format (Parquet / CSV / Excel /
  Document), recency (24h / 7d / 30d / All), and sort (Name /
  Newest / Largest). Selections persist per-folder via Zustand.

### Changed

- **Settings → AI provider panel** removed entirely. Settings is now:
  Sync · Storage · Privacy · MCP.
- **`Ask.tsx`** rewritten as a cloud pointer. If the user is signed
  in to a workspace, the CTA opens `/chat`; otherwise it surfaces
  "Connect a workspace to use AI".

### Cargo

- Removed `byok` and `text_to_sql` modules. `sha2`, `base64`, `rand`,
  `futures`, `fs4` retained — still load-bearing for gdrive_oauth /
  gdrive streaming / disk_space pre-flight.
- `Cargo.lock` only changed the `SeryLink` package version line.

### Verification

- `cargo check --lib` clean.
- `pnpm build` clean (TypeScript + Vite).
- Manual: dashboard `/chat` already exercises the cross-machine
  tunnel-fanout path end-to-end against this build's tunnel client
  (no desktop-side changes needed for that path).

## [0.5.3] — 2026-05-01

Final release with on-device BYOK / text-to-SQL before the v0.6.0
pivot. Tagged for archival reference; subsequent releases route AI
through cloud `/chat`.

### Added

- **BYOK for OpenAI and Google Gemini** alongside the existing
  Anthropic provider, with per-provider model override in Settings.
- **Text-to-SQL agent loop on local DuckDB.** `/ask` tab uses the
  configured BYOK key to translate questions into SQL, executes
  against the local DuckDB instance, and grounds answers in local
  search hits. Includes a DESCRIBE-before-execute step to mitigate
  a duckdb-rs panic, trailing-semicolon trimming, and a meta-question
  branch in the prompt.
- **Google Drive end-to-end.** OAuth (PKCE + loopback redirect),
  recursive folder walker, hourly background refresh, per-row Watch
  button, checkbox-based folder selection, Sheets via /export →
  .xlsx, streaming downloads with size cap, single-flight watch,
  pre-flight free-space check, Storage tab (disk usage + Clear Drive
  cache), persisted skipped log surfaced in Search results with a
  "filename only" badge.
- **Public Google Sheets URLs** auto-rewrite to the CSV export endpoint.
- **`seryai://pair?key=…` deep link** wired into the Connect modal.
- **Source icons + per-kind labels** in the Folders sidebar.
- **F7 BYOK foundation** (originally drafted as v0.6.0 — see git
  history for the full pre-pivot intent): unit-tested provider
  isolation (`byok::anthropic::tests::anthropic_request_url_targets_anthropic_only`),
  audit log gains `kind` discriminator (`sync` | `byok_call`),
  Privacy header BYOK counters.

### Changed

- **Folder detail view** — replaced prominent scan panel with an
  inline progress indicator.
- **Scan cache** persists Shallow-tier files and surfaces cache write
  errors (was silently swallowing).
- **Ask draft / folder filter / history filter state** lifted into
  Zustand store so they survive tab switches.

### Notes

- All BYOK / text-to-SQL surfaces removed in v0.6.0 — install v0.5.3
  only if you specifically need on-device AI with paste-your-own keys.

## [0.5.0] - 2026-04-22

The local-first pivot. Sery Link is now a free, local desktop app that
indexes every CSV, spreadsheet, and document on your machines —
search by column name, inspect schemas and column stats in place, and
(on the paid AI tier) ask questions in plain English across every
machine you own. Open source under AGPL-3.0-or-later.

### Added

#### Column-aware search (new hero feature)
- Global search bar matches filenames, column names, and extracted
  document content across every folder and every remote source in one
  pass. Replaces the previous "which folder was that file in again?"
  hunt.
- Per-file column profile: open any file to see schema, sample rows,
  and per-column stats (null %, unique values, min/max/avg). Computed
  locally via DuckDB `SUMMARIZE`, merged with schema into a single
  auto-loading Columns table.

#### Remote sources
- Add public HTTPS URLs as data sources (Phase A).
- Add S3 URLs with credentials stored in the OS keychain (Phase B1).
- Add S3 bucket + prefix listings (Phase B2).
- All fetches happen on the local agent — credentials and raw data
  never transit Sery's cloud.

#### Schema-change notifications
- Cache-level schema diff computed at scan time.
- Toast UI surfaces changes as they're detected.
- Dedicated Notifications tab with persistent JSONL storage across
  restarts.
- Cross-machine broadcast: schema changes detected on one machine
  surface on every other machine in the workspace within seconds.
- Rapid-repeat dedup so file-watcher bounce doesn't spam the tab.
- Per-machine unread badge in the Machines view.
- Settings toggle to silence toasts while keeping persisted records.

#### Local-first onboarding
- No silent cloud contact on first launch. Pick a folder, search,
  profile files — all offline. Connecting to Sery.ai is an explicit
  opt-in with a workspace key.

#### Open source
- Sery Link is now open source under the **GNU Affero General Public
  License v3.0 or later** (AGPL-3.0-or-later). See `LICENSE`.
- `CONTRIBUTING.md`, `SUPPORT.md`, `SECURITY.md` for contributor and
  reporter guidance.
- Tauri auto-updater wired to GitHub Releases — existing installs
  auto-update on every tagged release.
- Release pipeline via `.github/workflows/release.yml` (tag-driven
  matrix builds for macOS arm64/x64, Windows, Linux).

#### Performance
- Persistent scan cache (`~/.sery/scan_cache.db`) with tiering so
  large folders don't re-read on every launch.
- Virtualized folder detail view handles folders with 10K+ files.
- Cache-warm folder detail views skip auto-rescan.
- CSV parser fallback ladder + graceful degradation for malformed
  files.

### Changed

- **"Fleet" renamed to "Machines"** across UI, code, and public copy.
  Internal types renamed (`FleetView` → `MachinesView`, `FleetAgent`
  → `Machine`, `list_fleet` → `list_machines`, route `/fleet` →
  `/machines`). Backend HTTP URL kept at `/v1/agent/workspace/fleet`
  for continuity with the api repo.
- Sidebar labels reorganized around the local-first flow.
- Every page now uses a consistent full-width shell.
- Source tree cleaned for open-source release: internal planning docs
  relocated to `docs/internal/`, personal paths scrubbed from history.

### Removed

- **Pair-code flow.** Machines now join a workspace via workspace
  keys (copy from machine A, paste on machine B). QR-code pair flow
  removed.
- **SQL Recipes feature.** The Analytics page, recipe execution
  surface, `recipe_executor` Rust module, and the 9 seed recipe JSONs
  have been removed. Users ask questions in plain English on the AI
  tier — the SQL pipeline is no longer exposed.
- **Dataset Relationship Graph.** The visualization and its
  "Show Relationships" button are gone; cross-file relationships are
  now surfaced implicitly through column-aware search.

### Fixed

- `CommandPalette` no longer loops on stale `useMetadataCache` return
  values.
- Per-file profile wraps `SUMMARIZE` in `SELECT` and catches DuckDB
  panics for malformed files.
- Scanner doesn't auto-rescan folder detail when the cache is warm.

### Security

- AGPL-3 license means the whole source of Sery Link is auditable.
  The central privacy claim — "your files never leave your machines"
  — is now verifiable by reading the code, not trust-us.
- `SECURITY.md` documents the private disclosure path
  (security@sery.ai, 72h acknowledgement target, safe-harbor clause).
- Auto-updater artifacts are cryptographically signed with minisign
  public-key verification. The pubkey is embedded in the app; the
  private key is held only by the release maintainer.
- Commit history rewritten to remove personal file paths and
  accidentally-committed build artifacts.

## [0.4.0] - 2026-04-15

### 🎉 Major Features

#### Three-Tier Authentication Strategy
- **LocalOnly Mode** - Zero authentication, local SQL queries only (NEW)
  - Query files with SQL immediately after install
  - 5 FREE analysis recipes included
  - No account required, no cloud sync
  - Complete first query in < 60 seconds

- **BYOK Mode** - Bring Your Own API Key (NEW)
  - Use your own Anthropic API key
  - Unlock PRO recipes and AI features
  - Data stays local, no cloud dependency
  - Full control over API usage and costs

- **WorkspaceKey Mode** - Full workspace integration (EXISTING)
  - All features from v0.3.x
  - Cloud sync and team collaboration
  - Performance mode with S3 upload
  - Managed API usage

### ✨ Added

#### Backend (Rust)
- Added `AuthMode` enum with three variants (`LocalOnly`, `BYOK`, `WorkspaceKey`)
- Added `get_auth_mode()` function for automatic mode detection
- Added `feature_available()` function for tier-based feature gating
- Added `get_current_auth_mode` Tauri command
- Added `check_feature_available` Tauri command
- Added `set_auth_mode` Tauri command
- Added `execute_recipe` Tauri command with tier enforcement
- Added automatic migration for existing users (`migrate_if_needed()`)
- Added comprehensive feature availability matrix

#### Frontend (React/TypeScript)
- Added `ModeSelectionStep` to onboarding wizard
- Added `useFeatureGate` custom React hook
- Added `UpgradePrompt` component (banner and modal variants)
- Added recipe tier filtering in `RecipePanel`
- Added tier error handling in `RecipeExecutor`
- Added visual lock icons on unavailable PRO recipes
- Added upgrade CTAs throughout UI

#### Documentation
- Added `TESTING_v0.4.0.md` - Comprehensive testing guide
- Added `IMPLEMENTATION_REFERENCE.md` - Developer reference
- Added `CHANGELOG.md` - This file

### 🔧 Changed

#### Breaking Changes
- **None!** All existing users auto-migrate to WorkspaceKey mode seamlessly

#### Non-Breaking Changes
- Modified onboarding flow: Welcome → **Mode Selection** → Connect → Folder → Privacy → Done
- Changed auth gate logic: `!authenticated` → `!config.app.first_run_completed`
- Updated `AppConfig` schema to include `selected_auth_mode` field
- Recipe execution now checks tier authorization before allowing execution

### 🐛 Fixed
- N/A (new feature release)

### 🔒 Security
- Workspace tokens remain securely stored in macOS Keychain
- BYOK API keys marked with `#[serde(skip_serializing)]` to prevent exposure
- LocalOnly mode makes zero network calls to Sery API
- Tier enforcement happens at Rust level (cannot bypass from UI)

### 📊 Metrics & Analytics
- Track percentage of users in each auth mode
- Monitor time-to-first-query for new users (target: < 60 seconds)
- Track upgrade conversion rate from LocalOnly to PRO tiers

### 🎯 Performance
- Startup time: < 2 seconds (LocalOnly), < 3 seconds (WorkspaceKey)
- Recipe loading: < 500ms
- Auth mode check: < 100ms
- No performance degradation vs v0.3.x

### 📦 Recipe Library

#### FREE Recipes (5)
1. CSV Time Series Aggregation - Generic time series analysis
2. GA Traffic Sources - Google Analytics traffic breakdown
3. Shopify Churn Rate - Customer churn calculation
4. Shopify Top Products - Best-selling products analysis
5. Stripe MRR - Monthly Recurring Revenue tracking

#### PRO Recipes (4)
1. GA Funnel Analysis - Conversion funnel tracking
2. Shopify Customer LTV - Customer Lifetime Value
3. Shopify Product Affinity - Cross-sell recommendations
4. Stripe Cohort Retention - Subscription retention analysis

### 🚀 Upgrade Path

#### From v0.3.x
1. Install v0.4.0
2. Launch app
3. Automatic migration to WorkspaceKey mode
4. All features continue working as before

#### Fresh Install
1. Install v0.4.0
2. Launch app → Onboarding wizard
3. Select "Local Vault (FREE)" or "Sery Workspace (PRO)"
4. Add folder → Start querying immediately

### 🔜 Next Release (v0.5.0)

Planned features for next release:
- BYOK: API key validation on entry
- BYOK: Direct Anthropic API calls (no backend required)
- BYOK: Local embeddings for semantic search
- BYOK: Rust-based agent loop for AI queries
- Performance improvements
- Additional FREE recipes

---

## [0.3.0] - 2026-04-10

### Added
- Desktop agent with Tauri
- File watcher for automatic sync
- Plugin system (WebAssembly)
- Recipe executor framework
- Metadata cache with DuckDB
- Relationship detector

### Changed
- Migrated from Electron to Tauri
- Improved scan performance
- Enhanced WebSocket tunnel

### Fixed
- Memory leaks in file watcher
- Sync race conditions
- Plugin isolation issues

---

## [0.2.0] - 2026-03-15

### Added
- WebSocket tunnel for remote queries
- Privacy controls
- Audit logging
- Query history

### Changed
- Improved authentication flow
- Enhanced error messages

### Fixed
- Token refresh issues
- Scan accuracy problems

---

## [0.1.0] - 2026-02-01

### Added
- Initial release
- S3 scanner
- DuckDB query engine
- Basic authentication
- Folder watching

---

## Version History

- **v0.4.0** (2026-04-XX) - Three-Tier Strategy
- **v0.3.0** (2026-04-10) - Desktop Agent + Plugins
- **v0.2.0** (2026-03-15) - WebSocket Tunnel + Privacy
- **v0.1.0** (2026-02-01) - Initial Release

---

## Links

- [GitHub Repository](https://github.com/seryai/sery-link)
- [Issue Tracker](https://github.com/seryai/sery-link/issues)
- [Documentation](https://sery.ai/docs)
- [Sery Dashboard](https://sery.ai)
