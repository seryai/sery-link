# Changelog

All notable changes to Sery Link will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-04-XX

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
