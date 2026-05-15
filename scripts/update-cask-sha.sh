#!/usr/bin/env bash
# Usage: scripts/update-cask-sha.sh <version>
# Downloads release artifacts from GitHub, computes sha256, and updates
# homebrew-tap and scoop-bucket manifests. Run AFTER the release CI finishes.
set -e

VERSION="$1"
if [ -z "$VERSION" ]; then
  echo "Usage: $0 <version>  (e.g. 0.8.3)"
  exit 1
fi

if ! echo "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "Version must be x.y.z — got: $VERSION"
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$SCRIPT_DIR/../.."
HOMEBREW_TAP="$ROOT/homebrew-tap"
SCOOP_BUCKET="$ROOT/scoop-bucket"
GITHUB_BASE="https://github.com/seryai/sery-link/releases/download/v${VERSION}"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

sha256_of() {
  local url="$1"
  local file="$TMPDIR/$(basename "$url")"
  echo "  Downloading $(basename "$url") ..." >&2
  if ! curl -fsSL --retry 3 --retry-delay 5 -o "$file" "$url"; then
    echo "ERROR: failed to download $url" >&2
    echo "  Is the release published and artifacts uploaded?" >&2
    exit 1
  fi
  shasum -a 256 "$file" | awk '{print $1}'
}

echo "Fetching sha256 for v${VERSION} release artifacts..."

ARM_DMG="Sery.Link_${VERSION}_aarch64.dmg"
X64_DMG="Sery.Link_${VERSION}_x64.dmg"
X64_MSI="Sery.Link_${VERSION}_x64_en-US.msi"

SHA_ARM=$(sha256_of "${GITHUB_BASE}/${ARM_DMG}")
SHA_X64_DMG=$(sha256_of "${GITHUB_BASE}/${X64_DMG}")
SHA_X64_MSI=$(sha256_of "${GITHUB_BASE}/${X64_MSI}")

echo ""
echo "sha256 results:"
echo "  aarch64 DMG : $SHA_ARM"
echo "  x64 DMG     : $SHA_X64_DMG"
echo "  x64 MSI     : $SHA_X64_MSI"
echo ""

# ── homebrew-tap ─────────────────────────────────────────────────────────────

CASK="$HOMEBREW_TAP/Casks/sery-link.rb"

# Replace version
sed -i '' "s/version \".*\"/version \"${VERSION}\"/" "$CASK"

# Replace arm sha256 (first occurrence after "on_arm")
awk -v sha="$SHA_ARM" '
  /on_arm/ { in_arm=1 }
  in_arm && /sha256/ { sub(/sha256 ".*"/, "sha256 \"" sha "\""); in_arm=0 }
  { print }
' "$CASK" > "$TMPDIR/sery-link.rb" && mv "$TMPDIR/sery-link.rb" "$CASK"

# Replace intel sha256 (first occurrence after "on_intel")
awk -v sha="$SHA_X64_DMG" '
  /on_intel/ { in_intel=1 }
  in_intel && /sha256/ { sub(/sha256 ".*"/, "sha256 \"" sha "\""); in_intel=0 }
  { print }
' "$CASK" > "$TMPDIR/sery-link.rb" && mv "$TMPDIR/sery-link.rb" "$CASK"

echo "Updated: $CASK"

git -C "$HOMEBREW_TAP" add Casks/sery-link.rb
git -C "$HOMEBREW_TAP" commit -m "chore: update sha256 for v${VERSION}"
git -C "$HOMEBREW_TAP" push

# ── scoop-bucket ─────────────────────────────────────────────────────────────

MANIFEST="$SCOOP_BUCKET/bucket/sery-link.json"

# Use Python for clean JSON editing (no jq dependency required)
python3 - "$MANIFEST" "$VERSION" "$SHA_X64_MSI" << 'PYEOF'
import sys, json

path, version, sha = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path) as f:
    data = json.load(f)

data["version"] = version
data["architecture"]["64bit"]["url"] = (
    f"https://github.com/seryai/sery-link/releases/download/v{version}"
    f"/Sery.Link_{version}_x64_en-US.msi"
)
data["architecture"]["64bit"]["hash"] = sha

with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PYEOF

echo "Updated: $MANIFEST"

git -C "$SCOOP_BUCKET" add bucket/sery-link.json
git -C "$SCOOP_BUCKET" commit -m "chore: update sha256 for v${VERSION}"
git -C "$SCOOP_BUCKET" push

echo ""
echo "✓ homebrew-tap and scoop-bucket updated for v${VERSION}."
echo "  Users can now: brew upgrade --cask sery-link"
echo "                 scoop update sery-link"
