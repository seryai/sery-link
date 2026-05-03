# Changelog

All notable changes to Sery Link will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
