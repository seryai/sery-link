# Testing Guide: Three-Tier Strategy v0.4.0

## Overview

This document provides comprehensive testing instructions for the v0.4.0 release, which introduces the three-tier authentication strategy (LocalOnly, BYOK, WorkspaceKey).

## Test Environment Setup

### Prerequisites
- Clean macOS environment (or use a separate user profile)
- No existing Sery Link installation
- No stored credentials in keyring

### Clean Install Preparation

```bash
# Remove existing installation and config
rm -rf ~/.seryai
rm -rf ~/Library/Application\ Support/com.sery.link

# Clear keyring entries (if exists)
# This will be done manually during testing
```

## Test Suite

### Test 1: Fresh Install Flow (< 60 seconds)

**Objective:** Verify that a new user can complete their first query in under 60 seconds with zero authentication.

**Steps:**
1. **Install and Launch** (5 seconds)
   - Install Sery Link from DMG or build
   - Launch application
   - Expected: Onboarding wizard appears

2. **Welcome Step** (5 seconds)
   - Review welcome screen
   - Click "Get started"
   - Expected: Mode selection screen appears

3. **Mode Selection** (10 seconds)
   - Review two mode options:
     - Local Vault (FREE)
     - Sery Workspace (PRO)
   - Click "Local Vault" card
   - Expected: Skip to folder selection (no auth step)

4. **Folder Selection** (15 seconds)
   - Click "Click to browse"
   - Select a folder with CSV/Parquet files
   - Expected: Folder path displays
   - Click "Add & Scan" or "Next"
   - Expected: Scan starts in background

5. **Privacy & Done** (10 seconds)
   - Review privacy information
   - Click "Next"
   - Expected: Completion screen
   - Click "Get Started" or "Finish"
   - Expected: Main app interface loads

6. **First Query** (15 seconds)
   - Navigate to Folders tab (should be default)
   - See scanned datasets
   - Navigate to Recipes tab
   - Select a FREE recipe (e.g., "Time Series Aggregation")
   - Fill in minimal parameters
   - Click "Run Recipe"
   - Expected: Query executes successfully

**Success Criteria:**
- [ ] Total time < 60 seconds from launch to query result
- [ ] No authentication prompts
- [ ] No errors during onboarding
- [ ] At least one FREE recipe visible
- [ ] Query execution works without API key

**Expected Auth Mode:** `LocalOnly`

---

### Test 2: Tier Gating Enforcement

**Objective:** Verify that PRO recipes are properly blocked in LocalOnly mode with appropriate upgrade prompts.

#### Test 2.1: Recipe Visibility

**Steps:**
1. Complete fresh install (Test 1) in LocalOnly mode
2. Navigate to Recipes tab
3. Count visible recipes

**Success Criteria:**
- [ ] Exactly 5 recipes visible (all FREE tier):
  - CSV Time Series Aggregation
  - GA Traffic Sources
  - Shopify Churn Rate
  - Shopify Top Products
  - Stripe MRR
- [ ] Upgrade banner displayed at top of Recipes panel
- [ ] Banner shows "Unlock PRO Features" message

#### Test 2.2: Recipe Filtering

**Steps:**
1. In Recipes tab, check filter dropdown
2. Select "All Tiers" filter

**Success Criteria:**
- [ ] Filter shows only FREE recipes even with "All Tiers" selected
- [ ] No PRO recipes appear in results
- [ ] No errors in console

#### Test 2.3: Upgrade Prompt (Banner)

**Steps:**
1. Review upgrade banner at top of Recipes panel
2. Read feature comparison

**Success Criteria:**
- [ ] Banner clearly states "PRO Features" are locked
- [ ] Two upgrade buttons visible:
  - "Connect Workspace"
  - "Use My API Key"
- [ ] Feature list shows FREE vs PRO capabilities

#### Test 2.4: Recipe Execution Blocking

**Steps:**
1. Manually call a PRO recipe (via developer console or direct API)
2. Try to execute `ga-funnel-analysis` recipe

**Expected:**
- [ ] Rust command returns error: "Recipe 'Funnel Analysis' requires PRO tier..."
- [ ] Error message includes upgrade CTA
- [ ] No SQL execution occurs

---

### Test 3: Existing User Migration

**Objective:** Verify that existing users with workspace tokens automatically migrate to WorkspaceKey mode.

#### Test 3.1: Pre-Migration Setup

**Preparation:**
1. Install Sery Link v0.3.x (previous version)
2. Complete workspace authentication
3. Add watched folders
4. Verify workspace token in keyring:
   ```bash
   # Check keyring entry exists
   security find-generic-password -s "com.sery.link" -a "access_token"
   ```

#### Test 3.2: Upgrade and Migration

**Steps:**
1. Close Sery Link v0.3.x
2. Install Sery Link v0.4.0
3. Launch application
4. Observe startup behavior

**Success Criteria:**
- [ ] No onboarding wizard shown
- [ ] App loads directly to main interface
- [ ] All watched folders still present
- [ ] Config file (`~/.seryai/config.json`) contains:
   ```json
   {
     "app": {
       "selected_auth_mode": {
         "type": "WorkspaceKey",
         "key": "<from_keyring>"
       },
       "first_run_completed": true
     }
   }
   ```

#### Test 3.3: Post-Migration Feature Access

**Steps:**
1. Navigate to Recipes tab
2. Count visible recipes

**Success Criteria:**
- [ ] All 9 recipes visible (5 FREE + 4 PRO)
- [ ] No upgrade banner shown
- [ ] Can execute both FREE and PRO recipes
- [ ] Filter shows "All Tiers" by default
- [ ] No authentication prompts

**Expected Auth Mode:** `WorkspaceKey`

---

### Test 4: BYOK Mode (Bring Your Own Key)

**Objective:** Verify BYOK mode activation and PRO feature unlocking.

#### Test 4.1: BYOK Activation from LocalOnly

**Steps:**
1. Start in LocalOnly mode (fresh install)
2. Navigate to Recipes tab
3. Click "Use My API Key" on upgrade banner
4. Enter valid Anthropic API key when prompted
5. Observe UI changes

**Success Criteria:**
- [ ] Prompt accepts API key input
- [ ] Auth mode updates to BYOK
- [ ] Config file contains:
   ```json
   {
     "app": {
       "selected_auth_mode": {
         "type": "BYOK",
         "provider": "anthropic"
       }
     }
   }
   ```
- [ ] All 9 recipes now visible
- [ ] Upgrade banner disappears
- [ ] Can execute PRO recipes

**Expected Auth Mode:** `BYOK`

**Note:** API key is stored in config but NOT in keyring. It's marked with `#[serde(skip_serializing)]` so it won't appear in exported configs.

---

### Test 5: Mode Switching

**Objective:** Verify users can switch between auth modes.

#### Test 5.1: LocalOnly → WorkspaceKey

**Steps:**
1. Start in LocalOnly mode
2. Click "Connect Workspace" on upgrade banner
3. Enter valid workspace key
4. Complete authentication

**Success Criteria:**
- [ ] Auth mode switches to WorkspaceKey
- [ ] Token saved to keyring
- [ ] All PRO features unlocked
- [ ] WebSocket tunnel starts
- [ ] File watcher starts (if folders exist)

#### Test 5.2: BYOK → WorkspaceKey

**Steps:**
1. Start in BYOK mode (with API key configured)
2. Navigate to Settings
3. Connect workspace key
4. Observe mode change

**Success Criteria:**
- [ ] Auth mode switches to WorkspaceKey
- [ ] Previous BYOK API key no longer used
- [ ] Cloud sync features now available
- [ ] Team features visible (if applicable)

---

### Test 6: Feature Availability Matrix

**Objective:** Verify feature gating works correctly for all auth modes.

| Feature | LocalOnly | BYOK | WorkspaceKey |
|---------|-----------|------|--------------|
| FREE recipes | ✅ | ✅ | ✅ |
| Local SQL queries | ✅ | ✅ | ✅ |
| PRO recipes | ❌ | ✅ | ✅ |
| AI-powered queries | ❌ | ✅ | ✅ |
| Cloud sync | ❌ | ❌ | ✅ |
| Team sharing | ❌ | ❌ | ✅ |
| Performance mode | ❌ | ❌ | ✅ |

**Testing Steps:**
1. For each auth mode:
   - Check `get_current_auth_mode()` returns correct mode
   - For each feature, call `check_feature_available(feature)`
   - Verify return value matches matrix

**Automated Test:**
```typescript
// Test script to run in browser console
const features = [
  'free_recipes',
  'local_sql',
  'pro_recipes',
  'ai_queries',
  'cloud_sync',
  'team_sharing',
  'performance_mode'
];

const testMode = async (mode) => {
  console.log(`\nTesting ${mode} mode:`);
  for (const feature of features) {
    const available = await invoke('check_feature_available', { feature });
    console.log(`  ${feature}: ${available ? '✅' : '❌'}`);
  }
};

// Run for current mode
const mode = await invoke('get_current_auth_mode');
testMode(mode.type);
```

---

### Test 7: UI/UX Validation

**Objective:** Verify all UI elements display correctly and provide good UX.

#### Test 7.1: Onboarding Wizard

**Steps:**
1. Fresh install
2. Review each onboarding step
3. Check mode selection cards

**Success Criteria:**
- [ ] Progress indicator shows 6 steps (not 5)
- [ ] Mode selection step is clear and attractive
- [ ] Local Vault card has "FREE" badge
- [ ] Workspace card has "PRO" badge
- [ ] Feature comparison is accurate
- [ ] Lock icons show for unavailable features
- [ ] Back button works on all steps

#### Test 7.2: Upgrade Prompts

**Steps:**
1. LocalOnly mode
2. Navigate to Recipes
3. Review upgrade banner

**Success Criteria:**
- [ ] Banner has purple/accent color scheme
- [ ] Sparkles icon displays
- [ ] Text is clear and compelling
- [ ] Two CTA buttons visible
- [ ] Banner doesn't block navigation
- [ ] Banner can be dismissed (if applicable)

#### Test 7.3: Recipe Cards (LocalOnly)

**Steps:**
1. LocalOnly mode
2. View recipe grid

**Success Criteria:**
- [ ] Only FREE recipes visible
- [ ] No lock icons on FREE recipes
- [ ] Cards are clickable
- [ ] Hover states work
- [ ] Recipe details modal opens
- [ ] Execution flow works

#### Test 7.4: Modal Upgrade Prompt

**Steps:**
1. LocalOnly mode
2. Somehow trigger PRO recipe execution
3. Observe modal

**Success Criteria:**
- [ ] Modal appears with dark overlay
- [ ] Alert icon displays
- [ ] Clear "PRO Feature Required" heading
- [ ] Feature explanation visible
- [ ] Two upgrade options clearly presented
- [ ] "Cancel" button works
- [ ] Modal dismisses on ESC key

---

### Test 8: Error Handling

**Objective:** Verify graceful error handling for edge cases.

#### Test 8.1: Invalid API Key (BYOK)

**Steps:**
1. Click "Use My API Key"
2. Enter invalid key (e.g., "invalid-key-123")
3. Attempt to execute PRO recipe

**Expected:**
- [ ] System accepts the key (no validation yet)
- [ ] Recipe execution may fail with API error
- [ ] Error message is user-friendly
- [ ] User can update key in settings

#### Test 8.2: Expired Workspace Token

**Steps:**
1. WorkspaceKey mode with expired token
2. Attempt to sync metadata
3. Observe behavior

**Expected:**
- [ ] Re-authentication modal appears
- [ ] User can enter new workspace key
- [ ] Or can switch to LocalOnly mode
- [ ] Error message is clear

#### Test 8.3: Recipe Tier Mismatch

**Steps:**
1. Manually edit recipe JSON to change tier
2. Reload recipes
3. Verify tier enforcement still works

**Expected:**
- [ ] Tier check happens in Rust command
- [ ] Cannot bypass with UI manipulation
- [ ] Error message matches actual tier

---

### Test 9: Performance & Regression

**Objective:** Verify no performance degradation or regressions.

#### Test 9.1: Startup Time

**Steps:**
1. Measure app launch time
2. Compare across auth modes

**Success Criteria:**
- [ ] LocalOnly startup: < 2 seconds
- [ ] WorkspaceKey startup: < 3 seconds (includes token check)
- [ ] No blocking operations on UI thread
- [ ] Config migration runs asynchronously

#### Test 9.2: Recipe Loading

**Steps:**
1. Navigate to Recipes tab
2. Measure time to display recipes

**Success Criteria:**
- [ ] Recipes load in < 500ms
- [ ] No flickering during tier filtering
- [ ] Search/filter is instant
- [ ] No console errors

#### Test 9.3: Query Execution

**Steps:**
1. Execute FREE recipe in LocalOnly mode
2. Execute same recipe in WorkspaceKey mode
3. Compare execution times

**Success Criteria:**
- [ ] No significant difference in execution time
- [ ] DuckDB query performance unchanged
- [ ] Results display correctly in both modes

---

### Test 10: Data Privacy & Security

**Objective:** Verify security promises are upheld.

#### Test 10.1: LocalOnly Data Isolation

**Steps:**
1. LocalOnly mode
2. Scan sensitive folder
3. Check network requests

**Success Criteria:**
- [ ] No network calls to Sery API
- [ ] No schema metadata uploaded
- [ ] All queries execute locally
- [ ] No data leaves device

#### Test 10.2: Keyring Security

**Steps:**
1. WorkspaceKey mode
2. Check keyring storage
3. Verify encryption

**Success Criteria:**
- [ ] Token stored securely in macOS Keychain
- [ ] Config file does NOT contain plain-text token
- [ ] API key (BYOK) is NOT serialized to disk

#### Test 10.3: Config Export

**Steps:**
1. Export configuration (if feature exists)
2. Review exported JSON
3. Check for sensitive data

**Success Criteria:**
- [ ] Workspace token not in export
- [ ] BYOK API key not in export
- [ ] Only public config values exported
- [ ] `#[serde(skip_serializing)]` works correctly

---

## Automated Test Suite

### Unit Tests (Rust)

Create tests in `src-tauri/src/auth.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_local_only_default() {
        let config = Config::default();
        let mode = get_auth_mode(&config);
        assert!(matches!(mode, AuthMode::LocalOnly));
    }

    #[test]
    fn test_feature_availability_local_only() {
        let mode = AuthMode::LocalOnly;
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "local_sql"));
        assert!(!feature_available(&mode, "pro_recipes"));
        assert!(!feature_available(&mode, "ai_queries"));
        assert!(!feature_available(&mode, "cloud_sync"));
    }

    #[test]
    fn test_feature_availability_byok() {
        let mode = AuthMode::BYOK {
            provider: "anthropic".to_string(),
            api_key: "sk-ant-test".to_string(),
        };
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "pro_recipes"));
        assert!(feature_available(&mode, "ai_queries"));
        assert!(!feature_available(&mode, "cloud_sync"));
        assert!(!feature_available(&mode, "team_sharing"));
    }

    #[test]
    fn test_feature_availability_workspace() {
        let mode = AuthMode::WorkspaceKey {
            key: "sery_k_test".to_string(),
        };
        assert!(feature_available(&mode, "free_recipes"));
        assert!(feature_available(&mode, "pro_recipes"));
        assert!(feature_available(&mode, "cloud_sync"));
        assert!(feature_available(&mode, "team_sharing"));
        assert!(feature_available(&mode, "performance_mode"));
    }
}
```

Run tests:
```bash
cd src-tauri
cargo test auth::tests
```

### Integration Tests (TypeScript)

Create `src/tests/authMode.test.ts`:

```typescript
import { describe, it, expect, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

describe('Auth Mode System', () => {
  beforeEach(async () => {
    // Reset to LocalOnly mode
    await invoke('set_auth_mode', { mode: { type: 'LocalOnly' } });
  });

  it('should default to LocalOnly mode', async () => {
    const mode = await invoke('get_current_auth_mode');
    expect(mode.type).toBe('LocalOnly');
  });

  it('should block PRO features in LocalOnly', async () => {
    const available = await invoke('check_feature_available', {
      feature: 'pro_recipes'
    });
    expect(available).toBe(false);
  });

  it('should allow FREE features in LocalOnly', async () => {
    const available = await invoke('check_feature_available', {
      feature: 'free_recipes'
    });
    expect(available).toBe(true);
  });

  it('should unlock PRO features with BYOK', async () => {
    await invoke('set_auth_mode', {
      mode: {
        type: 'BYOK',
        provider: 'anthropic',
        api_key: 'test-key'
      }
    });

    const available = await invoke('check_feature_available', {
      feature: 'pro_recipes'
    });
    expect(available).toBe(true);
  });
});
```

Run tests:
```bash
npm run test
```

---

## Checklist Summary

### Test 1: Fresh Install
- [ ] Complete install in < 60 seconds
- [ ] Select Local Vault mode
- [ ] Add folder successfully
- [ ] Execute FREE recipe

### Test 2: Tier Gating
- [ ] Only 5 FREE recipes visible
- [ ] Upgrade banner shows
- [ ] PRO recipes blocked at Rust level
- [ ] Error messages are clear

### Test 3: Migration
- [ ] Existing users auto-migrate
- [ ] WorkspaceKey mode detected
- [ ] All features work post-migration
- [ ] No data loss

### Test 4: BYOK
- [ ] API key input works
- [ ] PRO features unlock
- [ ] Key stored correctly
- [ ] Can execute PRO recipes

### Test 5: Mode Switching
- [ ] LocalOnly → WorkspaceKey works
- [ ] BYOK → WorkspaceKey works
- [ ] No errors during switch

### Test 6: Feature Matrix
- [ ] All features gated correctly
- [ ] No bypasses possible
- [ ] Automated test passes

### Test 7: UI/UX
- [ ] Onboarding is clear
- [ ] Upgrade prompts are attractive
- [ ] Lock icons display correctly
- [ ] No visual glitches

### Test 8: Error Handling
- [ ] Invalid keys handled gracefully
- [ ] Expired tokens prompt re-auth
- [ ] Tier checks cannot be bypassed

### Test 9: Performance
- [ ] Fast startup times
- [ ] Quick recipe loading
- [ ] No regressions

### Test 10: Security
- [ ] LocalOnly truly offline
- [ ] Tokens encrypted in keyring
- [ ] No sensitive data in exports

---

## Success Metrics

After testing, verify these metrics:

✅ **< 60 sec from install to first query** (Test 1)
✅ **0 breaking changes for existing users** (Test 3)
✅ **100% of FREE recipes work without auth** (Test 1, 2)
📊 **Track: % of users in local mode** (Analytics)
📊 **Track: % upgrade within 30 days** (Analytics)

---

## Known Issues & Limitations

### v0.4.0 Limitations:
1. **BYOK mode** - Basic implementation, no API validation yet
2. **No recipe execution** - execute_recipe returns SQL only (DuckDB integration pending)
3. **No team features** - TEAM tier exists but not fully implemented
4. **Upgrade CTAs** - Link to settings, not inline auth flow

### Future Improvements (v0.5.0+):
- BYOK: Validate API key on entry
- Recipe execution: Full DuckDB query execution
- Upgrade flow: Inline workspace key entry
- Analytics: Track mode adoption rates
