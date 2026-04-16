# Three-Tier Strategy Implementation Reference

## Quick Links

- **Strategy Document**: `docs/THREE_TIER_STRATEGY.md`
- **Testing Guide**: `TESTING_v0.4.0.md`
- **Recipe Files**: `examples/recipes/` (5 FREE, 4 PRO)

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Three Auth Modes                         │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │  LocalOnly   │  │     BYOK     │  │ WorkspaceKey │     │
│  │              │  │              │  │              │     │
│  │ • FREE only  │  │ • FREE+PRO   │  │ • FREE+PRO   │     │
│  │ • No auth    │  │ • User API   │  │ • Cloud sync │     │
│  │ • Local SQL  │  │ • Local SQL  │  │ • Team feat. │     │
│  └──────────────┘  └──────────────┘  └──────────────┘     │
│                                                               │
└─────────────────────────────────────────────────────────────┘
         ↓                    ↓                    ↓
┌─────────────────────────────────────────────────────────────┐
│                   Feature Gate System                         │
├─────────────────────────────────────────────────────────────┤
│                                                               │
│  feature_available(mode: AuthMode, feature: &str) → bool    │
│                                                               │
│  FREE:  free_recipes, local_sql                             │
│  PRO:   + pro_recipes, ai_queries                           │
│  TEAM:  + cloud_sync, team_sharing, performance_mode        │
│                                                               │
└─────────────────────────────────────────────────────────────┘
```

## File Modifications Summary

### Backend (Rust)

#### 1. **src-tauri/src/config.rs**
- Added `AuthMode` enum (3 variants)
- Added `selected_auth_mode: Option<AuthMode>` to `AppConfig`
- Added `migrate_if_needed()` method for existing users

#### 2. **src-tauri/src/auth.rs**
- Added `get_auth_mode(config: &Config) → AuthMode`
  - Detection order: keyring → env vars → LocalOnly
- Added `feature_available(mode: &AuthMode, feature: &str) → bool`
  - Feature matrix for all modes

#### 3. **src-tauri/src/commands.rs**
- Added `get_current_auth_mode() → Result<AuthMode, String>`
- Added `check_feature_available(feature: String) → Result<bool, String>`
- Added `set_auth_mode(mode: AuthMode) → Result<(), String>`
- Added `execute_recipe(recipe_id, params) → Result<String, String>`
  - Includes tier authorization check

#### 4. **src-tauri/src/lib.rs**
- Added migration call in setup: `config.migrate_if_needed()`
- Registered 4 new commands in `invoke_handler`

### Frontend (TypeScript/React)

#### 5. **src/App.tsx**
- Changed auth gate: `!authenticated` → `!config?.app?.first_run_completed`
- Allows LocalOnly mode access without sign-in

#### 6. **src/components/OnboardingWizard.tsx**
- Added `ModeSelectionStep` component
- Updated step flow: `welcome → mode → connect → folder → privacy → done`
- Added `authChoice: AuthChoice | null` state
- Local mode skips connect step
- Sets auth mode in `FolderStep` for local users

#### 7. **src/hooks/useFeatureGate.ts** (NEW)
- Custom React hook for feature availability
- Returns `{ available, mode, loading }`
- Used throughout UI for conditional rendering

#### 8. **src/components/UpgradePrompt.tsx** (NEW)
- Two variants: `banner` (inline) and `modal` (fullscreen)
- Two upgrade paths: "Connect Workspace" and "Use My API Key"
- Feature comparison (FREE vs PRO)

#### 9. **src/components/RecipePanel.tsx**
- Added `useFeatureGate('pro_recipes')` hook
- Filters recipes by tier in LocalOnly mode
- Displays upgrade banner when in LocalOnly
- Lock icons on PRO recipes when unavailable
- Upgrade modal on locked recipe click

#### 10. **src/components/RecipeExecutor.tsx**
- Updated to call `execute_recipe` command
- Detects tier restriction errors
- Shows upgrade modal for tier violations
- Graceful error handling

## Code Patterns

### Checking Auth Mode (Rust)

```rust
// In a command
let config = Config::load().map_err(|e| e.to_string())?;
let auth_mode = auth::get_auth_mode(&config);

match auth_mode {
    AuthMode::LocalOnly => {
        // Local-only logic
    }
    AuthMode::BYOK { provider, .. } => {
        // BYOK logic
    }
    AuthMode::WorkspaceKey { .. } => {
        // Workspace logic
    }
}
```

### Checking Feature Availability (Rust)

```rust
let config = Config::load()?;
let auth_mode = auth::get_auth_mode(&config);

if !auth::feature_available(&auth_mode, "pro_recipes") {
    return Err("This feature requires PRO tier".to_string());
}
```

### Using Feature Gate (React)

```typescript
import { useFeatureGate } from '../hooks/useFeatureGate';

function MyComponent() {
  const { available, mode, loading } = useFeatureGate('pro_recipes');

  if (loading) return <Spinner />;

  if (!available) {
    return <UpgradePrompt variant="banner" feature="pro_recipes" />;
  }

  // Render PRO feature
  return <ProFeature />;
}
```

### Setting Auth Mode (React)

```typescript
import { invoke } from '@tauri-apps/api/core';

// Set LocalOnly mode
await invoke('set_auth_mode', {
  mode: { type: 'LocalOnly' }
});

// Set BYOK mode
await invoke('set_auth_mode', {
  mode: {
    type: 'BYOK',
    provider: 'anthropic',
    api_key: userApiKey
  }
});

// Set WorkspaceKey mode (after authentication)
await invoke('set_auth_mode', {
  mode: {
    type: 'WorkspaceKey',
    key: '<from_keyring>'
  }
});
```

## Recipe Tier System

### FREE Recipes (5)

1. **CSV Time Series Aggregation**
   - Generic time series analysis
   - Works with any CSV/Parquet with dates
   - Path: `examples/recipes/csv-time-series.json`

2. **GA Traffic Sources**
   - Google Analytics traffic analysis
   - Source/medium breakdown
   - Path: `examples/recipes/ga-traffic-sources.json`

3. **Shopify Churn Rate**
   - Customer churn calculation
   - Cohort-based analysis
   - Path: `examples/recipes/shopify-churn-rate.json`

4. **Shopify Top Products**
   - Best-selling products
   - Revenue and quantity metrics
   - Path: `examples/recipes/shopify-top-products.json`

5. **Stripe MRR**
   - Monthly Recurring Revenue
   - Subscription metrics
   - Path: `examples/recipes/stripe-mrr.json`

### PRO Recipes (4)

1. **GA Funnel Analysis**
   - Conversion funnel tracking
   - Drop-off identification
   - Path: `examples/recipes/ga-funnel-analysis.json`

2. **Shopify Customer LTV**
   - Customer Lifetime Value calculation
   - Cohort analysis
   - Path: `examples/recipes/shopify-customer-ltv.json`

3. **Shopify Product Affinity**
   - Cross-sell recommendations
   - Product co-purchase patterns
   - Path: `examples/recipes/shopify-product-affinity.json`

4. **Stripe Cohort Retention**
   - Subscription retention by cohort
   - Churn analysis
   - Path: `examples/recipes/stripe-cohort-retention.json`

## Configuration Storage

### Config File: `~/.seryai/config.json`

```json
{
  "agent": {
    "name": "MacBook Pro",
    "platform": "macos",
    "hostname": "hostname",
    "agent_id": null
  },
  "watched_folders": [],
  "cloud": {
    "api_url": "http://localhost:8000",
    "websocket_url": "ws://localhost:8000",
    "web_url": "http://localhost:3000"
  },
  "sync": {
    "interval_seconds": 300,
    "auto_sync_on_change": true,
    "fallback_scan_interval_seconds": 3600
  },
  "app": {
    "theme": "system",
    "launch_at_login": true,
    "auto_update": true,
    "notifications_enabled": true,
    "first_run_completed": false,
    "window_hide_notified": false,
    "selected_auth_mode": {
      "type": "LocalOnly"
      // OR
      // "type": "BYOK",
      // "provider": "anthropic"
      // Note: api_key is NOT serialized
      // OR
      // "type": "WorkspaceKey"
      // Note: key is NOT serialized
    }
  }
}
```

### Keyring Storage (macOS)

```
Service: com.sery.link
Account: access_token
Value: <workspace_token>
```

## Build & Development

### Build Backend

```bash
cd src-tauri
cargo build          # Debug build
cargo build --release # Release build
cargo test           # Run tests
```

### Build Frontend

```bash
npm install          # Install dependencies
npm run dev          # Development server
npm run build        # Production build
npm run tauri dev    # Tauri dev mode
npm run tauri build  # Build app
```

### Run Tests

```bash
# Rust tests
cd src-tauri
cargo test

# TypeScript tests (if configured)
npm run test
```

## Debugging

### Check Current Auth Mode

```typescript
// In browser console
const mode = await invoke('get_current_auth_mode');
console.log('Current mode:', mode);
```

### Check Feature Availability

```typescript
// In browser console
const features = [
  'free_recipes',
  'pro_recipes',
  'ai_queries',
  'cloud_sync',
  'team_sharing'
];

for (const feature of features) {
  const available = await invoke('check_feature_available', { feature });
  console.log(`${feature}: ${available ? '✅' : '❌'}`);
}
```

### View Config File

```bash
cat ~/.seryai/config.json | jq
```

### Check Keyring

```bash
# macOS
security find-generic-password -s "com.sery.link" -a "access_token" -w

# Display (may prompt for keychain password)
security find-generic-password -s "com.sery.link" -a "access_token"
```

### View Rust Logs

```bash
# Set log level
export RUST_LOG=debug

# Run app
npm run tauri dev
```

## Common Issues & Solutions

### Issue: Onboarding Shows Despite Having Token

**Cause:** `first_run_completed` is false in config
**Solution:** Migration should set this to true. Check:
```bash
jq '.app.first_run_completed' ~/.seryai/config.json
```

### Issue: PRO Recipes Visible in LocalOnly Mode

**Cause:** Feature gate not working
**Solution:** Check auth mode:
```typescript
const mode = await invoke('get_current_auth_mode');
console.log(mode); // Should be { type: 'LocalOnly' }
```

### Issue: Recipe Execution Blocked with "requires PRO tier"

**Cause:** Expected behavior in LocalOnly mode
**Solution:** This is correct! Upgrade prompt should appear.

### Issue: Migration Not Running

**Cause:** `migrate_if_needed()` only runs if `selected_auth_mode` is None
**Solution:** Delete mode from config to test:
```bash
jq 'del(.app.selected_auth_mode)' ~/.seryai/config.json > /tmp/config.json
mv /tmp/config.json ~/.seryai/config.json
```

## Performance Monitoring

### Startup Time

```typescript
// In main.tsx
console.time('app-startup');

// ... app initialization

console.timeEnd('app-startup');
// Target: < 2 seconds
```

### Recipe Load Time

```typescript
// In RecipePanel.tsx
console.time('recipe-load');
const recipes = await invoke('list_recipes');
console.timeEnd('recipe-load');
// Target: < 500ms
```

### Auth Mode Check Time

```typescript
console.time('auth-mode-check');
const mode = await invoke('get_current_auth_mode');
console.timeEnd('auth-mode-check');
// Target: < 100ms
```

## Security Checklist

✅ **Workspace token** stored in keyring, NOT in config file
✅ **BYOK API key** marked with `#[serde(skip_serializing)]`
✅ **LocalOnly mode** makes zero network calls
✅ **Tier enforcement** happens in Rust (cannot bypass from UI)
✅ **Config export** excludes sensitive fields

## Deployment Checklist

Before releasing v0.4.0:

- [ ] All tests pass (see TESTING_v0.4.0.md)
- [ ] Rust backend compiles without warnings
- [ ] TypeScript frontend builds without errors
- [ ] Example recipes load correctly (5 FREE + 4 PRO)
- [ ] Migration works for existing users
- [ ] Fresh install completes in < 60 seconds
- [ ] PRO recipes blocked in LocalOnly mode
- [ ] Upgrade prompts display correctly
- [ ] No console errors in LocalOnly mode
- [ ] BYOK mode unlocks PRO features
- [ ] WorkspaceKey mode works as before
- [ ] Updated version in `Cargo.toml` and `package.json`
- [ ] Changelog updated with v0.4.0 changes
- [ ] Documentation reviewed and accurate

## Version Information

**Version:** 0.4.0
**Release Date:** TBD
**Code Name:** "Local Vault"
**Key Feature:** Zero-login local-first usage

## Future Roadmap

### v0.5.0 - BYOK Full Implementation
- API key validation on entry
- Direct Anthropic API calls
- Local embeddings for semantic search
- Rust agent loop for AI queries

### v0.6.0 - Performance Mode
- S3 upload for large datasets
- Cloud-based DuckDB queries
- Optimistic UI updates
- Result streaming

### v0.7.0 - Team Workspace
- User invitations
- Role-based permissions
- Shared query history
- Team recipe library

---

**Last Updated:** 2026-04-15
**Maintainer:** Sery Team
**Support:** https://github.com/seryai/sery-link/issues
