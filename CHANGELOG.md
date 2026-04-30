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

## [0.6.0] — Unreleased

The data-network release. v0.5.0 made Sery Link local-first; v0.6.0
turns "your machines + Sery cloud" into a real network primitive —
catalog + fan-out + cross-machine queries with structural privacy
guarantees that ship as code, not promises.

### Added

#### Bring Your Own Anthropic key (F7)
- **Settings → Sync → AI provider** — paste your Anthropic API key,
  hit "Test & save". The key is validated against `api.anthropic.com`
  before being saved to the OS keychain; bad keys never persist.
- **New /ask page** — single-question conversations with your own key.
  The prompt goes from this app straight to Anthropic, never via
  Sery's servers. Each turn shows a green *"Direct to Anthropic"*
  badge so the privacy guarantee is visible per message.
- **Privacy guarantee enforced by code, not policy.** A unit test
  (`byok::anthropic::tests::anthropic_request_url_targets_anthropic_only`)
  fails if any URL the BYOK code path constructs targets anything
  other than `api.anthropic.com`.
- OpenAI BYOK lands in v0.6.x.

#### Quick-Ask hotkey is now BYOK-aware (F9)
- ⌘⇧S (macOS) / Ctrl+Shift+S (others) routes to `/ask` when a BYOK
  key is configured, falling back to `/search` when it isn't. The
  hotkey finally lives up to its name.

#### Cross-machine recipes (F11)
- The dashboard's chat page now has a **"Save as recipe"** button on
  assistant messages.
- Sery Link's Recipes view shows the workspace's saved recipes;
  click Run to open the question in your browser. Sery Link
  notifies the API so `run_count` and `last_run_at` increment per
  machine, with a per-agent F14 audit event.

#### Outbound deep links (F3 / F1 follow-up)
- Sery Link registers the **`seryai://` URL scheme** on macOS,
  Windows, and Linux. Two verbs:
  - `seryai://reveal?path=<absolute-path>` — opens the file's
    parent folder in your file manager with the file selected.
    Used by the dashboard's "Open" button on cross-machine search
    results, closing the gap where clicking a hit used to do
    nothing.
  - `seryai://pair?key=<workspace-key>` — placeholder routing for
    the future deep-link pairing alternative; surfaces the main
    window and emits a frontend event. Receiver UI ships in v0.6.x.

#### Privacy: BYOK calls in the local audit file (F5)
- `~/.seryai/sync_audit.jsonl` now records BYOK calls alongside
  the existing sync events. Each entry carries the host the
  request actually targeted (always `api.anthropic.com` — the
  load-bearing privacy proof) plus prompt / response character
  counts and round-trip duration. **No prompt text is logged**,
  only metadata.
- **"Reveal audit file"** button in Privacy opens the folder in
  Finder/Explorer and surfaces the absolute path so you can
  `tail -f` it from a terminal.
- Privacy header gains separate counters for Syncs, BYOK calls,
  and errors. BYOK rows render with an emerald host pill.

### Changed

- **Quick-Ask hotkey routing** is now BYOK-aware (see Added).
  Pre-v0.6 always landed on `/search`.
- **Privacy view section title** "Sync activity" → "Outbound
  activity" — broader scope now that BYOK calls are also logged.
- **Per-machine fan-out ranking in chat answers** — when an AI
  question fans out across multiple machines, the result table
  shows a per-machine row-count + ms breakdown with share-percentage
  ("Machine A: 1,234 rows / 87 ms · 92%") instead of a flat list of
  machine names. The streaming pipeline also gained the missing
  `result_table` event for fan-out queries — previously the
  FanoutBanner UI never rendered in streamed responses because the
  event was only emitted for single-agent queries.
- **Search results in the dashboard** are now expandable: full
  file path, all matched columns with types, "Open" button (via
  `seryai://reveal`), Copy-path button.

### Breaking

- ⚠️ **Document text upload is now opt-in (default off)** (F2).
  Settings → Sync → "Include document text in workspace catalog"
  defaults to **off**, resolving an audit finding that contradicted
  the "raw files never leave your devices" promise. Cross-machine
  document search degrades to filename-only for documents
  (DOCX/PPTX/HTML/IPYNB) until the toggle is turned on. Local
  document search (within Sery Link itself) is unaffected.
  Existing rows in the cloud catalog were cleared by Alembic
  migration `025_clear_dataset_document_text` on the server side.
  **If you were relying on cross-machine document search:** flip
  the toggle on and re-sync. Tabular files (CSV/Parquet/Excel)
  are untouched.

### Account & data control

- **Real "Delete my account" endpoint** wired through the dashboard's
  `/settings/workspace` Danger Zone. Type-the-workspace-name
  confirmation; modal pre-fetches a per-table count snapshot
  ("Datasets: 42 · Conversations: 17 · Agents: 3 · Recipes: 8")
  so you see what's about to vanish. Cascades through every
  workspace relationship; closes GDPR Article 17 + the Privacy
  Policy promise that previously linked to a settings page with
  no delete affordance.

### Infrastructure

- Added `tauri-plugin-deep-link` and `tauri-plugin-global-shortcut`
  to the Cargo manifest.
- New Rust modules: `byok` (Anthropic client + keyring helpers),
  `deep_link` (URL scheme dispatcher), `hotkey` (Quick-Ask global
  shortcut).
- Audit log struct extended with a `kind` discriminator
  (`sync` | `byok_call`) — backwards-compatible with v0.5 audit
  files via `#[serde(default)]`.
- New Tauri commands: `save_byok_key`, `clear_byok_key`,
  `validate_byok_key`, `ask_byok`, `get_byok_status`,
  `mark_recipe_run`, `reveal_audit_file_in_finder`.
- `tauri.conf.json` gains a `plugins.deep-link.desktop.schemes`
  entry for `seryai`. `capabilities/default.json` grants
  `deep-link:default`.

### Known gaps

- **Claude Desktop MCP support** (separate stdio shim) is on the
  v0.6.x near-term roadmap; v0.6.0 does not ship MCP yet. See
  `SPEC_MCP.md` in the docs repo for the full design.
- **OpenAI BYOK** lands in v0.6.x.
- **Right-click "Ask Sery" in Finder/Explorer** (F10) is explicitly
  deferred to Year 2 — large per-OS native-extension work.
- **OS-native open of search results** uses the `seryai://reveal`
  deep link, which requires Sery Link to be installed on the
  machine running the browser. Without Sery Link, the Copy path
  button is the fallback.

### Verification

- `cargo build --lib` clean across every commit.
- `cargo test --lib byok` — 4/4 passing, including the load-bearing
  privacy URL assertion.
- TypeScript `tsc --noEmit` clean across desktop + dashboard.
- ⚠️ Backend CI is currently red at infrastructure level (runner
  allocation failure, not code) — needs to be unblocked before
  this release tag is cut.
- **Pre-release manual verification (Week 11/12 dogfood):**
  install the v0.6.0 build, exercise (a) BYOK end-to-end with
  network capture confirming zero `*.sery.ai` traffic for the LLM
  call, (b) cross-machine search → click "Open" → confirm Finder
  reveals the file, (c) account deletion against a staging
  workspace, (d) F2 document-text toggle off→on→off cycle.

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
