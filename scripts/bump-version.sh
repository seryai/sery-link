#!/usr/bin/env bash
# Usage: scripts/bump-version.sh <new-version>
# Bumps version across sery-link, homebrew-tap, scoop-bucket, and website repos,
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
SCOOP_BUCKET="$ROOT/scoop-bucket"
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

sed -i '' "s/version \"$OLD\"/version \"$NEW\"/" "$HOMEBREW_TAP/Casks/sery-link.rb"

git -C "$HOMEBREW_TAP" add Casks/sery-link.rb
git -C "$HOMEBREW_TAP" commit -m "chore: bump version to $NEW (sha256 TBD after release)"

# ── scoop-bucket ─────────────────────────────────────────────────────────────

sed -i '' "s/\"version\": \"$OLD\"/\"version\": \"$NEW\"/" "$SCOOP_BUCKET/bucket/sery-link.json"
# Update URL version references (autoupdate uses $version but the static url has the literal)
sed -i '' "s|/v$OLD/|/v$NEW/|g" "$SCOOP_BUCKET/bucket/sery-link.json"
sed -i '' "s|_${OLD}_|_${NEW}_|g" "$SCOOP_BUCKET/bucket/sery-link.json"

git -C "$SCOOP_BUCKET" add bucket/sery-link.json
git -C "$SCOOP_BUCKET" commit -m "chore: bump version to $NEW (hash TBD after release)"

# ── tag + push all ───────────────────────────────────────────────────────────

git -C "$SERY_LINK" push
git -C "$HOMEBREW_TAP" push
git -C "$SCOOP_BUCKET" push

git -C "$SERY_LINK" tag "v$NEW"
git -C "$SERY_LINK" push origin "v$NEW"

echo ""
echo "✓ v$NEW tagged and pushed."
echo "  After the release builds, update sha256 hashes in:"
echo "    homebrew-tap/Casks/sery-link.rb  (aarch64 + x64 DMG)"
echo "    scoop-bucket/bucket/sery-link.json  (x64 MSI)"
echo "  and run: scripts/update-cask-sha.sh $NEW"
