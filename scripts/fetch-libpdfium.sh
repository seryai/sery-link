#!/usr/bin/env bash
# Fetch libpdfium for the current platform / arch from
# bblanchon/pdfium-binaries on GitHub releases, extract the shared
# library (`libpdfium.dylib` / `libpdfium.so` / `pdfium.dll`), drop
# it into `src-tauri/resources/libpdfium/`, and codesign on macOS.
#
# Why a script and not a build.rs:
#   - build.rs adds a network dep to every cargo build, including
#     dev-mode incremental rebuilds. Painful.
#   - This script runs once per CI matrix target, before
#     tauri-action. Devs typically don't need it (the macOS arm64
#     binary is committed in the repo — see
#     `src-tauri/resources/libpdfium/README.md`).
#
# Usage (CI):
#   scripts/fetch-libpdfium.sh
#
# Usage (force a specific target on a dev machine):
#   TARGET=linux-x64 scripts/fetch-libpdfium.sh
#   TARGET=win-x64   scripts/fetch-libpdfium.sh
#
# Idempotent: re-runs are no-ops once the file exists, unless
# `--force` is passed as the first argument.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST_DIR="$REPO_ROOT/src-tauri/resources/libpdfium"
PDFIUM_RELEASE="${PDFIUM_RELEASE:-chromium/7811}"  # pin to a known-good release; bump deliberately

# bblanchon/pdfium-binaries publishes "moving" tags like `chromium/<rev>`
# for each Chromium-tracked release. We pin one explicit version
# rather than `latest` so reproducibility doesn't drift between CI
# runs months apart. Override via PDFIUM_RELEASE env var.
DOWNLOAD_BASE="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}"

# Auto-detect target triple if not explicitly set.
if [ -z "${TARGET:-}" ]; then
    case "$(uname -s)-$(uname -m)" in
        Darwin-arm64)        TARGET=mac-arm64;;
        Darwin-x86_64)       TARGET=mac-x64;;
        Linux-x86_64)        TARGET=linux-x64;;
        Linux-aarch64)       TARGET=linux-arm64;;
        Linux-armv7l)        TARGET=linux-arm;;
        # Windows runners under Git Bash report MINGW64_NT-...
        MINGW64*-x86_64|MSYS*-x86_64|CYGWIN*-x86_64)
                             TARGET=win-x64;;
        MINGW32*-i686|MSYS*-i686)
                             TARGET=win-x86;;
        *)
            echo "fetch-libpdfium: cannot auto-detect target ($(uname -s)-$(uname -m)); set TARGET explicitly" >&2
            echo "    valid TARGETs: mac-arm64, mac-x64, linux-x64, linux-arm64, linux-arm, win-x64, win-x86" >&2
            exit 1
            ;;
    esac
fi

# Map target → expected library filename.
case "$TARGET" in
    mac-*)       LIB_FILE=libpdfium.dylib; LIB_SUBDIR=lib;;
    linux-*)     LIB_FILE=libpdfium.so;    LIB_SUBDIR=lib;;
    win-*)       LIB_FILE=pdfium.dll;      LIB_SUBDIR=bin;;
    *)
        echo "fetch-libpdfium: unknown TARGET '$TARGET'" >&2
        exit 1
        ;;
esac

DEST_FILE="$DEST_DIR/$LIB_FILE"
mkdir -p "$DEST_DIR"

# Idempotent unless --force.
if [ "${1:-}" != "--force" ] && [ -f "$DEST_FILE" ]; then
    echo "fetch-libpdfium: $DEST_FILE already exists, skipping (re-run with --force to overwrite)"
    exit 0
fi

ARCHIVE="pdfium-${TARGET}.tgz"
URL="${DOWNLOAD_BASE}/${ARCHIVE}"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

echo "fetch-libpdfium: downloading $URL"
# Use curl rather than wget; curl is on every platform's runner image.
# --fail makes a 404 a hard error (default is silent). --location
# follows redirects (GitHub's release URLs redirect to S3).
# --retry-all-errors covers connection-level failures like
# "Empty reply from server" (curl error 52) which are surprisingly
# common on Windows runners and aren't covered by plain --retry.
curl --fail --silent --show-error --location \
     --retry 8 --retry-delay 2 --retry-all-errors --connect-timeout 30 \
     --output "$TMP_DIR/$ARCHIVE" \
     "$URL"

# Clear any pre-existing libpdfium binary for OTHER platforms.
# Without this, a Linux CI run would leave the committed
# `libpdfium.dylib` in place AND drop a fresh `libpdfium.so` next
# to it — both would end up in the bundle via
# `bundle.resources: ["resources/libpdfium/*"]`. We want exactly
# one file, the one matching the build target.
echo "fetch-libpdfium: clearing other-platform binaries from $DEST_DIR"
find "$DEST_DIR" -maxdepth 1 -type f \( \
        -name 'libpdfium.dylib' \
     -o -name 'libpdfium.so' \
     -o -name 'pdfium.dll' \
     \) ! -name "$LIB_FILE" -delete

echo "fetch-libpdfium: extracting $LIB_SUBDIR/$LIB_FILE"
# Extract only the library file we need; skip the headers, license,
# args.gn, etc. The tarballs use different layouts on different
# targets, but the lib lives at one of two paths.
tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR" "$LIB_SUBDIR/$LIB_FILE"
mv "$TMP_DIR/$LIB_SUBDIR/$LIB_FILE" "$DEST_FILE"

# macOS post-processing: strip the quarantine attribute the
# downloaded archive picks up, then ad-hoc codesign so Gatekeeper
# doesn't block load.
if [ "${TARGET#mac-}" != "$TARGET" ]; then
    echo "fetch-libpdfium: stripping quarantine + ad-hoc codesigning"
    xattr -cr "$DEST_FILE" || true
    codesign --force --sign - "$DEST_FILE"
fi

echo "fetch-libpdfium: ready at $DEST_FILE ($(du -h "$DEST_FILE" | cut -f1))"
