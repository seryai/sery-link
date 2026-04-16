# Developer Quick Start Guide

Get up and running with Sery Link development in 5 minutes.

## Prerequisites

- **Rust** 1.70+ (`rustup` recommended)
- **Node.js** 18+ and npm
- **macOS** 12+ (for Tauri development)
- **Xcode Command Line Tools**

## Initial Setup

### 1. Clone and Install

```bash
# Clone repository
git clone https://github.com/seryai/sery-link.git
cd sery-link

# Install frontend dependencies
npm install

# Install Rust dependencies (handled by cargo)
cd src-tauri
cargo build
cd ..
```

**Time:** ~3 minutes (first build is slower)

### 2. Run in Development Mode

```bash
# Start Tauri dev server (auto-reload on changes)
npm run tauri dev
```

**What happens:**
- Vite dev server starts (port 1420)
- Tauri app window opens
- Hot reload enabled for frontend
- Rust rebuilds on save

**Time:** ~30 seconds

### 3. Verify Installation

When the app opens:

1. **See onboarding wizard** ✅
2. **Select "Local Vault"** ✅
3. **Add a test folder** ✅
4. **See folder scanning** ✅

If all steps work → **Setup successful!**

## Development Workflow

### Making Changes

#### Frontend (TypeScript/React)

```bash
# Edit files in src/
# Changes auto-reload in dev mode

# Run type checker
npm run build  # Runs tsc + vite build

# Format code (if configured)
npm run format
```

#### Backend (Rust)

```bash
cd src-tauri

# Run tests
cargo test

# Run specific test
cargo test auth::tests::test_local_only_mode_defaults

# Check code
cargo check

# Format code
cargo fmt

# Lint code
cargo clippy
```

### Testing Your Changes

#### 1. Unit Tests (Rust)

```bash
cd src-tauri
cargo test --lib
```

**What's tested:**
- Auth mode logic (9 tests)
- Config management (10 tests)
- Recipe execution (3 tests)
- Plugin system (8 tests)
- And more...

#### 2. Integration Testing

See `TESTING_v0.4.0.md` for comprehensive test suites.

#### 3. Manual Testing

```bash
# Clean install test
rm -rf ~/.seryai
npm run tauri dev

# Test onboarding flow
# Test mode selection
# Test recipe execution
```

## Common Tasks

### Task 1: Add a New Feature Flag

```rust
// src-tauri/src/auth.rs

pub fn feature_available(mode: &AuthMode, feature: &str) -> bool {
    match (mode, feature) {
        // Add new feature
        (AuthMode::BYOK { .. }, "my_new_feature") => true,
        // ... rest of matches
    }
}
```

```typescript
// src/hooks/useFeatureGate.ts
const { available } = useFeatureGate('my_new_feature');
```

### Task 2: Add a New Recipe

```bash
# Create recipe file
cat > examples/recipes/my-recipe.json << 'EOF'
{
  "id": "com.sery.myrecipe",
  "name": "My Recipe",
  "description": "Does cool analysis",
  "data_source": "CSV",
  "tier": "FREE",
  "sql_template": "SELECT * FROM {{table}} LIMIT {{limit}}",
  "parameters": [
    {"name": "table", "type": "string"},
    {"name": "limit", "type": "int", "default": 100}
  ],
  "author": "Your Name",
  "version": "1.0.0"
}
EOF
```

Recipe loads automatically in dev mode.

### Task 3: Add a New Tauri Command

```rust
// src-tauri/src/commands.rs

#[tauri::command]
pub async fn my_new_command(param: String) -> Result<String, String> {
    Ok(format!("Received: {}", param))
}
```

```rust
// src-tauri/src/lib.rs

.invoke_handler(tauri::generate_handler![
    // ... existing commands
    commands::my_new_command,
])
```

```typescript
// src/MyComponent.tsx
import { invoke } from '@tauri-apps/api/core';

const result = await invoke('my_new_command', { param: 'test' });
```

### Task 4: Update Auth Mode Logic

```rust
// src-tauri/src/auth.rs

pub fn get_auth_mode(config: &Config) -> AuthMode {
    // Add new detection logic
    if let Ok(new_mode) = std::env::var("SERY_AUTH_MODE") {
        match new_mode.as_str() {
            "local" => return AuthMode::LocalOnly,
            "byok" => return AuthMode::BYOK { /* ... */ },
            _ => {}
        }
    }

    // Fall back to existing logic
    // ...
}
```

## Project Structure

```
sery-link/
├── src/                    # Frontend (React/TypeScript)
│   ├── components/         # React components
│   ├── hooks/              # Custom hooks (useFeatureGate, etc.)
│   ├── stores/             # Zustand state stores
│   └── App.tsx             # Main app component
│
├── src-tauri/              # Backend (Rust)
│   ├── src/
│   │   ├── auth.rs         # Auth mode logic ⭐
│   │   ├── commands.rs     # Tauri commands ⭐
│   │   ├── config.rs       # Config management ⭐
│   │   ├── recipe_executor.rs  # Recipe system
│   │   └── lib.rs          # App entry point
│   └── Cargo.toml
│
├── examples/
│   └── recipes/            # Recipe JSON files (5 FREE, 4 PRO)
│
├── TESTING_v0.4.0.md       # Test guide
├── IMPLEMENTATION_REFERENCE.md  # Code reference
└── CHANGELOG.md            # Version history
```

⭐ = Modified for v0.4.0 three-tier strategy

## Debugging

### Frontend Debugging

```bash
# Open app
npm run tauri dev

# Open DevTools in app
# Cmd+Option+I (macOS)

# Check logs
console.log('Debug info:', data);
```

### Backend Debugging

```bash
# Enable Rust logs
export RUST_LOG=debug
npm run tauri dev

# Or for specific modules
export RUST_LOG=sery_link::auth=debug
npm run tauri dev
```

**Log output:**
```rust
use log::{info, debug, error};

debug!("Auth mode: {:?}", mode);
info!("Recipe executed successfully");
error!("Failed to load config: {}", err);
```

### Check Current State

```typescript
// In browser DevTools console

// Check auth mode
const mode = await invoke('get_current_auth_mode');
console.log('Current mode:', mode);

// Check feature availability
const available = await invoke('check_feature_available', {
  feature: 'pro_recipes'
});
console.log('PRO recipes available:', available);

// Check config
const config = await invoke('get_config');
console.log('Config:', config);
```

### Inspect Database

```bash
# View config file
cat ~/.seryai/config.json | jq

# Check keyring
security find-generic-password -s "com.sery.link" -a "access_token"

# View metadata cache
sqlite3 ~/.seryai/metadata.db
.tables
.schema datasets
SELECT COUNT(*) FROM datasets;
```

## Quick Reference

### Run Commands

| Command | Purpose |
|---------|---------|
| `npm run tauri dev` | Start dev server |
| `npm run tauri build` | Build production app |
| `npm run build` | Build frontend only |
| `cargo test` | Run Rust tests |
| `cargo build --release` | Build Rust release |
| `cargo clippy` | Lint Rust code |
| `cargo fmt` | Format Rust code |

### Important Files

| File | Purpose |
|------|---------|
| `src/App.tsx` | Main app, routing, auth gate |
| `src/components/OnboardingWizard.tsx` | First-run experience |
| `src/hooks/useFeatureGate.ts` | Feature availability hook |
| `src-tauri/src/auth.rs` | Auth mode detection & gating |
| `src-tauri/src/commands.rs` | Tauri RPC commands |
| `src-tauri/src/config.rs` | Config schema & persistence |
| `examples/recipes/*.json` | Recipe definitions |

### Environment Variables

```bash
# Rust logging
export RUST_LOG=debug

# BYOK mode (for testing)
export ANTHROPIC_API_KEY=sk-ant-xxx

# Custom config location
export SERY_CONFIG_DIR=/custom/path
```

### Build Modes

```bash
# Development (fast, debug symbols)
npm run tauri dev

# Release (optimized, no debug)
npm run tauri build

# Check only (no binary)
cd src-tauri && cargo check
```

## Troubleshooting

### Issue: "App won't start"

```bash
# Clean everything
rm -rf node_modules
rm -rf src-tauri/target
npm install
cargo clean
npm run tauri dev
```

### Issue: "TypeScript errors"

```bash
npm run build  # Shows all TS errors
# Fix errors, then retry
```

### Issue: "Rust compilation fails"

```bash
cd src-tauri
cargo clean
cargo build  # See detailed error
# Fix error, then retry
```

### Issue: "Config not loading"

```bash
# Reset config
rm -rf ~/.seryai
# Restart app - creates fresh config
```

### Issue: "Tests failing"

```bash
# Run specific test
cargo test test_name -- --nocapture

# See full output
RUST_BACKTRACE=1 cargo test
```

## Performance Tips

### Fast Iteration

1. **Frontend only changes** → Use Vite hot reload (instant)
2. **Rust-only changes** → Tauri rebuilds automatically (~5-15 sec)
3. **Both** → Tauri dev restarts (~15-30 sec)

### Speed Up Rust Builds

```bash
# Use sccache (Rust build cache)
cargo install sccache
export RUSTC_WRAPPER=sccache

# Use mold linker (macOS: zld)
# Add to src-tauri/.cargo/config.toml
```

### Reduce Build Time

```bash
# Build only what you need
cargo build --bin sery-link  # Skip tests

# Use release profile in dev (slower compile, faster runtime)
cargo build --profile release-with-debug
```

## Next Steps

1. ✅ **Read** `IMPLEMENTATION_REFERENCE.md` - Code patterns
2. ✅ **Review** `TESTING_v0.4.0.md` - Test scenarios
3. ✅ **Explore** `examples/recipes/` - Recipe structure
4. ✅ **Check** `CHANGELOG.md` - What's new in v0.4.0
5. ✅ **Try** making a small change and running tests

## Getting Help

- **Code Reference**: `IMPLEMENTATION_REFERENCE.md`
- **Testing Guide**: `TESTING_v0.4.0.md`
- **Issues**: https://github.com/seryai/sery-link/issues
- **Docs**: https://sery.ai/docs

## Useful Snippets

### Test Auth Modes

```typescript
// In browser console
async function testAllModes() {
  const modes = ['LocalOnly', 'BYOK', 'WorkspaceKey'];
  const features = ['free_recipes', 'pro_recipes', 'cloud_sync'];

  for (const mode of modes) {
    console.log(`\nMode: ${mode}`);
    for (const feature of features) {
      const available = await invoke('check_feature_available', { feature });
      console.log(`  ${feature}: ${available ? '✅' : '❌'}`);
    }
  }
}

testAllModes();
```

### Reset to Fresh State

```bash
#!/bin/bash
# save as: scripts/reset-dev.sh

echo "Resetting Sery Link to fresh state..."

# Remove config
rm -rf ~/.seryai

# Remove keyring entries
security delete-generic-password -s "com.sery.link" 2>/dev/null || true

# Clean build artifacts
rm -rf node_modules/.vite
rm -rf src-tauri/target/debug

echo "✅ Reset complete. Restart app for fresh onboarding."
```

### Profile Performance

```rust
// In any Rust function
use std::time::Instant;

let start = Instant::now();
// ... your code ...
let duration = start.elapsed();
println!("Operation took: {:?}", duration);
```

---

**Ready to code!** 🚀

Start with: `npm run tauri dev`

**Last Updated:** 2026-04-15
