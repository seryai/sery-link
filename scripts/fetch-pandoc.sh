#!/usr/bin/env bash
# Fetch the pandoc binary for the current platform / arch from
# jgm/pandoc on GitHub releases, extract just the executable, drop
# it into `src-tauri/resources/pandoc/`, and codesign on macOS.
#
# Why we bundle pandoc
#
#   mdkit's PandocExtractor handles DOCX / PPTX / EPUB / RTF / ODT /
#   LaTeX. It needs the `pandoc` binary at runtime — either on
#   PATH or via PandocExtractor::with_binary(path). Dev machines
#   typically have it via Homebrew; consumer machines don't.
#   Bundling closes the gap so user-facing builds don't silently
#   fall through to anytomd (which has lower fidelity than Pandoc
#   for these formats).
#
# Why a script and not a build.rs
#
#   Same reasoning as fetch-libpdfium.sh — build.rs adds a network
#   dep to every cargo build, including dev-mode incremental
#   rebuilds. This script runs once per CI matrix target before
#   tauri-action.
#
# Usage (CI):
#   scripts/fetch-pandoc.sh
#
# Usage (force a specific target on a dev machine):
#   TARGET=linux-amd64 scripts/fetch-pandoc.sh
#   TARGET=windows-x86_64 scripts/fetch-pandoc.sh
#
# Idempotent: re-runs are no-ops once the file exists, unless
# `--force` is passed as the first argument.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src-tauri/resources/pandoc"
PANDOC_VERSION="${PANDOC_VERSION:-3.9.0.2}"  # pin to a known-good release; bump deliberately

DOWNLOAD_BASE="https://github.com/jgm/pandoc/releases/download/${PANDOC_VERSION}"

# Auto-detect target if not explicitly set.
if [ -z "${TARGET:-}" ]; then
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)        TARGET=arm64-macOS;;
        Darwin-x86_64)       TARGET=x86_64-macOS;;
        Linux-x86_64)        TARGET=linux-amd64;;
        Linux-aarch64)       TARGET=linux-arm64;;
        MINGW64*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64)
                             TARGET=windows-x86_64;;
        *)
            echo "fetch-pandoc: cannot auto-detect target ($(uname -s)-$(uname -m)); set TARGET explicitly" >&2
            echo "    valid TARGETs: arm64-macOS, x86_64-macOS, linux-amd64, linux-arm64, windows-x86_64" >&2
            exit 1
            ;;
    esac
fi

# Map TARGET → archive filename + extracted binary path.
case "$TARGET" in
    *-macOS)
        ARCHIVE="pandoc-${PANDOC_VERSION}-${TARGET}.zip"
        ARCHIVE_TYPE=zip
        # Inside the macOS .zip: pandoc-VERSION-arm64/bin/pandoc
        ARCHIVE_INNER="pandoc-${PANDOC_VERSION}-${TARGET%-macOS}/bin/pandoc"
        BIN_FILE=pandoc
        ;;
    linux-*)
        ARCHIVE="pandoc-${PANDOC_VERSION}-${TARGET}.tar.gz"
        ARCHIVE_TYPE=tar
        ARCHIVE_INNER="pandoc-${PANDOC_VERSION}/bin/pandoc"
        BIN_FILE=pandoc
        ;;
    windows-x86_64)
        ARCHIVE="pandoc-${PANDOC_VERSION}-${TARGET}.zip"
        ARCHIVE_TYPE=zip
        ARCHIVE_INNER="pandoc-${PANDOC_VERSION}/pandoc.exe"
        BIN_FILE=pandoc.exe
        ;;
    *)
        echo "fetch-pandoc: unknown TARGET '$TARGET'" >&2
        exit 1
        ;;
esac

DEST_FILE="$DEST_DIR/$BIN_FILE"
mkdir -p "$DEST_DIR"

# Idempotent unless --force.
if [ "${1:-}" != "--force" ] && [ -f "$DEST_FILE" ]; then
    echo "fetch-pandoc: $DEST_FILE already exists, skipping (re-run with --force to overwrite)"
    exit 0
fi

URL="${DOWNLOAD_BASE}/${ARCHIVE}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

# Clear other-platform binaries from the resources dir before
# extracting. Same rationale as fetch-libpdfium.sh — without this,
# Linux CI would leave the committed mac arm64 pandoc in place AND
# drop a fresh linux pandoc next to it, both ending up bundled.
echo "fetch-pandoc: clearing other-platform binaries from $DEST_DIR"
find "$DEST_DIR" -maxdepth 1 -type f \( \
        -name 'pandoc' \
     -o -name 'pandoc.exe' \
     \) ! -name "$BIN_FILE" -delete 2>/dev/null || true

echo "fetch-pandoc: downloading $URL"
# --retry-all-errors covers connection-level failures like
# "Empty reply from server" (curl error 52) which plain --retry skips.
curl --fail --silent --show-error --location \
     --retry 8 --retry-delay 2 --retry-all-errors --connect-timeout 30 \
     --output "$TMP_DIR/$ARCHIVE" \
     "$URL"

echo "fetch-pandoc: extracting $ARCHIVE_INNER"
case "$ARCHIVE_TYPE" in
    zip)
        # `unzip` is on every CI image. -j flattens directories;
        # we want the original layout temporarily so the inner
        # path resolves predictably, hence no -j.
        unzip -q -o "$TMP_DIR/$ARCHIVE" -d "$TMP_DIR/extract"
        cp "$TMP_DIR/extract/$ARCHIVE_INNER" "$DEST_FILE"
        ;;
    tar)
        tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR" "$ARCHIVE_INNER"
        cp "$TMP_DIR/$ARCHIVE_INNER" "$DEST_FILE"
        ;;
esac

# Ensure the extracted binary is executable. tar preserves perms;
# zip on some platforms does not.
chmod +x "$DEST_FILE"

# macOS post-processing: strip quarantine, ad-hoc codesign so
# Gatekeeper doesn't block the binary.
if [ "${TARGET#*-}" = "macOS" ]; then
    echo "fetch-pandoc: stripping quarantine + ad-hoc codesigning"
    xattr -cr "$DEST_FILE" || true
    codesign --force --sign - "$DEST_FILE"
fi

echo "fetch-pandoc: ready at $DEST_FILE ($(du -h "$DEST_FILE" | cut -f1))"
