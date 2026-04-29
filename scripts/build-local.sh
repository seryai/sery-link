#!/usr/bin/env bash
#
# Build Sery Link locally for both macOS targets (arm64 + Intel).
# Designed to run on a Mac (Intel or Apple Silicon) — typically the
# maintainer's own machine or a Mac mini.
#
# Why this exists:
#   GitHub Actions on a private repo is metered. Running the same
#   build pipeline locally is free + works on hardware you already
#   own. This script mirrors what `.github/workflows/release.yml`
#   does in CI, minus Apple notarization (we ship unsigned macOS
#   builds — see /docs Gatekeeper override flow).
#
# Usage:
#   ./scripts/build-local.sh                # builds both arm64 + Intel
#   ./scripts/build-local.sh arm64          # arm64 only (faster)
#   ./scripts/build-local.sh intel          # Intel only
#
# Outputs:
#   dist-builds/Sery-Link_<ver>_<arch>.zip          (user-facing download)
#   src-tauri/target/<triple>/release/bundle/macos/
#     Sery Link.app                                  (the bundle itself)
#     Sery Link.app.tar.gz + .sig                    (auto-updater payload)
#
# DMG note:
#   Tauri's create-dmg wrapper is flaky on macOS 15+ (Sequoia / Tahoe).
#   We deliberately skip the DMG step (--bundles app --bundles updater)
#   and ship a `ditto`-produced zip instead. Slack, Zoom, Postman, and
#   plenty of other major macOS apps distribute as zips for the same
#   reason — codesigning and Gatekeeper handling are identical.
#
# Requires (one-time setup — see README at bottom of this script):
#   - rustup with both apple-darwin targets installed
#   - pnpm + Node 20+
#   - Tauri minisign key at ~/.tauri/sery-link.key (+ .password)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# ---- Sanity checks ---------------------------------------------------

if [ ! -f "package.json" ] || ! grep -q '"name": "sery-link"' package.json; then
  echo "✗ Not in the sery-link repo. Run from the repo root." >&2
  exit 1
fi

for cmd in rustup cargo pnpm node; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "✗ Missing required tool: $cmd. See setup notes at the bottom of $0." >&2
    exit 1
  fi
done

KEY="$HOME/.tauri/sery-link.key"
PWD_FILE="$HOME/.tauri/sery-link.password"
if [ ! -f "$KEY" ] || [ ! -f "$PWD_FILE" ]; then
  echo "✗ Minisign key not found at $KEY (or password at $PWD_FILE)." >&2
  echo "  Auto-updater artifacts won't sign without these. See setup notes." >&2
  exit 1
fi

# Tauri reads these env vars to sign the updater bundle.
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$KEY")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(cat "$PWD_FILE")"

# ---- Decide which target(s) to build ---------------------------------

TARGETS=()
case "${1:-both}" in
  arm64)  TARGETS=("aarch64-apple-darwin") ;;
  intel)  TARGETS=("x86_64-apple-darwin") ;;
  both|"") TARGETS=("aarch64-apple-darwin" "x86_64-apple-darwin") ;;
  *)
    echo "✗ Unknown target '$1'. Use: arm64 | intel | both" >&2
    exit 1
    ;;
esac

echo "→ Building for: ${TARGETS[*]}"

# ---- Install JS deps once (fast no-op if already installed) -----------

echo "→ Installing JS dependencies (pnpm)..."
pnpm install --frozen-lockfile

# ---- Per-target build loop -------------------------------------------

for TARGET in "${TARGETS[@]}"; do
  echo ""
  echo "================================================================"
  echo "  Building $TARGET"
  echo "================================================================"

  # Make sure rustup has the target installed.
  if ! rustup target list --installed | grep -q "^$TARGET$"; then
    echo "→ Adding rust target $TARGET..."
    rustup target add "$TARGET"
  fi

  # Map Tauri's target triple to the names the fetch scripts expect.
  case "$TARGET" in
    aarch64-apple-darwin) PDFIUM_TARGET="mac-arm64";  PANDOC_TARGET="arm64-macOS" ;;
    x86_64-apple-darwin)  PDFIUM_TARGET="mac-x64";    PANDOC_TARGET="x86_64-macOS" ;;
  esac

  echo "→ Fetching libpdfium for $PDFIUM_TARGET..."
  TARGET="$PDFIUM_TARGET" "$SCRIPT_DIR/fetch-libpdfium.sh" --force

  echo "→ Fetching pandoc for $PANDOC_TARGET..."
  TARGET="$PANDOC_TARGET" "$SCRIPT_DIR/fetch-pandoc.sh" --force

  # We deliberately skip Tauri's DMG bundler (--bundles app --bundles
  # updater). Tauri's create-dmg wrapper has been flaky on macOS 15+ —
  # AppleScript / hdiutil edge cases that aren't worth debugging when
  # `ditto` produces a perfectly distributable zip below. Slack, Zoom,
  # Postman ship as zips for the same reason. The `app` bundle gives us
  # `Sery Link.app`; the `updater` bundle gives us the .app.tar.gz +
  # .sig pair the auto-updater needs.
  echo "→ pnpm tauri build --target $TARGET --bundles app --bundles updater..."
  pnpm tauri build --target "$TARGET" --bundles app --bundles updater
done

# ---- Package the .app as a distributable zip -------------------------

# `ditto -c -k --keepParent` produces a Finder-style zip that preserves
# extended attributes + symlinks (a regular `zip -r` corrupts code-
# signing on macOS). The user-facing flow is: download → double-click
# unzip → drag .app to /Applications → first launch via Gatekeeper
# override.

VERSION="$(node -p "require('./package.json').version")"
DIST_DIR="$REPO_ROOT/dist-builds"
mkdir -p "$DIST_DIR"

for TARGET in "${TARGETS[@]}"; do
  APP_DIR="src-tauri/target/$TARGET/release/bundle/macos"
  if [ ! -d "$APP_DIR/Sery Link.app" ]; then
    echo "✗ Expected $APP_DIR/Sery Link.app but it's missing — skipping zip" >&2
    continue
  fi

  case "$TARGET" in
    aarch64-apple-darwin) ARCH_LABEL="aarch64" ;;
    x86_64-apple-darwin)  ARCH_LABEL="x64" ;;
  esac

  ZIP_PATH="$DIST_DIR/Sery-Link_${VERSION}_${ARCH_LABEL}.zip"
  echo "→ Packaging $ARCH_LABEL distributable zip..."
  ditto -c -k --keepParent "$APP_DIR/Sery Link.app" "$ZIP_PATH"
  echo "  → $ZIP_PATH ($(du -h "$ZIP_PATH" | cut -f1))"
done

# ---- Summary ---------------------------------------------------------

echo ""
echo "✓ Build complete. Artifacts:"
for TARGET in "${TARGETS[@]}"; do
  BUNDLE_DIR="src-tauri/target/$TARGET/release/bundle"
  if [ -d "$BUNDLE_DIR" ]; then
    echo "  $TARGET:"
    find "$BUNDLE_DIR/macos" -maxdepth 1 -type f \
      \( -name "*.app.tar.gz" -o -name "*.app.tar.gz.sig" \) \
      -exec echo "    {}" \; 2>/dev/null
  fi
done
echo "  user-facing zips:"
ls -1 "$DIST_DIR"/*.zip 2>/dev/null | sed 's/^/    /'

echo ""
echo "Next steps (manual upload to GitHub Release):"
echo "  1. Create / find the release: gh release create vX.Y.Z --draft --title 'Sery Link vX.Y.Z'"
echo "  2. Upload artifacts: gh release upload vX.Y.Z dist-builds/*.zip <each .app.tar.gz + .sig>"
echo "  3. Build latest.json updater manifest (see RELEASE.md §Auto-update manifest)"
echo "  4. Publish the release in the GitHub UI when ready."

# ---- First-time setup notes (read me if a check above failed) -------
#
# Install rustup + the macOS targets:
#   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
#   source "$HOME/.cargo/env"
#   rustup target add aarch64-apple-darwin x86_64-apple-darwin
#
# Install Node 20 + pnpm (via Homebrew or fnm):
#   brew install node@20 pnpm
#   # or: fnm install 20 && fnm default 20 && npm install -g pnpm
#
# Generate or restore the minisign key (see memory file
# `sery_link_release_signing.md`):
#   mkdir -p ~/.tauri
#   # If you already have the key in a password manager, paste:
#   #   ~/.tauri/sery-link.key
#   #   ~/.tauri/sery-link.password
#   # Otherwise, generate a fresh one (you'll need to update the
#   # pubkey in src-tauri/tauri.conf.json — see scripts/rotate-key.sh
#   # if it exists, otherwise see RELEASE.md):
#   #   PASSWORD=$(openssl rand -base64 32)
#   #   echo "$PASSWORD" > ~/.tauri/sery-link.password
#   #   chmod 600 ~/.tauri/sery-link.password
#   #   npx tauri signer generate \
#   #     --ci --password "$PASSWORD" \
#   #     --write-keys ~/.tauri/sery-link.key
#
# Pandoc + libpdfium are auto-fetched by the script; no manual setup.
