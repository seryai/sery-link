#!/usr/bin/env bash
# Usage: scripts/bump-version.sh <new-version>
# Bumps version across sery-link, homebrew-tap, and website repos,
# commits each, then tags sery-link with the new version.
set -e

NEW="$1"
if [ -z "$NEW" ]; then
  echo "Usage: $0 <new-version>  (e.g. 0.8.0)"
  exit 1
fi

# Validate semver-ish
if ! echo "$NEW" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Version must be x.y.z — got: $NEW"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SERY_LINK="$SCRIPT_DIR/.."
ROOT="$SERY_LINK/.."
HOMEBREW_TAP="$ROOT/homebrew-tap"
WEBSITE="$ROOT/website"

OLD=$(node -p "require('$SERY_LINK/package.json').version" 2>/dev/null || grep -m1 '"version"' "$SERY_LINK/package.json" | sed 's/.*"\([0-9.]*\)".*/\1/')

echo "Bumping $OLD → $NEW"

# ── sery-link ────────────────────────────────────────────────────────────────

sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$SERY_LINK/package.json"
sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$SERY_LINK/src-tauri/tauri.conf.json"
sed -i '' "s/^version = \"$OLD\"/version = \"$NEW\"/" "$SERY_LINK/src-tauri/Cargo.toml"

git -C "$SERY_LINK" add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml
git -C "$SERY_LINK" commit -m "chore: bump version to $NEW"

# ── homebrew-tap ─────────────────────────────────────────────────────────────

sed -i '' "s/VERSION=\"$OLD\"/VERSION=\"$NEW\"/" "$HOMEBREW_TAP/install.sh"
sed -i '' "s/version \"$OLD\"/version \"$NEW\"/" "$HOMEBREW_TAP/Casks/sery-link.rb"

git -C "$HOMEBREW_TAP" add install.sh Casks/sery-link.rb
git -C "$HOMEBREW_TAP" commit -m "chore: bump version to $NEW (sha256 TBD after release)"

# ── website ───────────────────────────────────────────────────────────────────

sed -i '' "s/VERSION=\"$OLD\"/VERSION=\"$NEW\"/" "$WEBSITE/public/install.sh"

git -C "$WEBSITE" add public/install.sh
git -C "$WEBSITE" commit -m "chore: bump install.sh to v$NEW"

# ── tag + push all ───────────────────────────────────────────────────────────

git -C "$SERY_LINK" push
git -C "$HOMEBREW_TAP" push
git -C "$WEBSITE" push

git -C "$SERY_LINK" tag "v$NEW"
git -C "$SERY_LINK" push origin "v$NEW"

echo ""
echo "✓ v$NEW tagged and pushed."
echo "  After the release builds, update sha256 in homebrew-tap/Casks/sery-link.rb"
echo "  and run: scripts/update-cask-sha.sh $NEW"
