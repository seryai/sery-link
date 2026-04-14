#!/bin/bash
# Test the sidecar integration with Sery Link

set -e

echo "🧪 Testing MarkItDown Sidecar Integration"
echo "=========================================="

# 1. Check sidecar binary exists
SIDECAR_DIST="dist/markitdown-sidecar-aarch64-apple-darwin"
if [ ! -f "$SIDECAR_DIST" ]; then
    echo "❌ Sidecar binary not found at $SIDECAR_DIST"
    echo "   Run: python3 build.py"
    exit 1
fi

echo "✅ Sidecar binary found: $SIDECAR_DIST"

# 2. Test sidecar directly
echo ""
echo "📄 Testing sidecar on a sample DOCX file..."
TEST_FILE="<redacted>/fixtures/sample.docx"

if [ ! -f "$TEST_FILE" ]; then
    echo "⚠️  Test file not found, skipping direct test"
else
    RESULT=$(echo "$TEST_FILE" | "$SIDECAR_DIST" | python3 -c "import sys, json; data=json.load(sys.stdin); print('✅ SUCCESS' if data['success'] else '❌ FAILED')")
    echo "   $RESULT"
fi

# 3. Check Tauri build
echo ""
echo "🦀 Checking Tauri build..."
TAURI_BIN="../src-tauri/target/release/sery-link"

if [ ! -f "$TAURI_BIN" ]; then
    echo "⚠️  Tauri binary not built yet"
    echo "   Run: cd ../src-tauri && cargo build --release"
else
    echo "✅ Tauri binary exists: $TAURI_BIN"

    # Check if the sidecar is bundled
    EXPECTED_SIDECAR="$(dirname "$TAURI_BIN")/markitdown-sidecar"
    if [ -f "$EXPECTED_SIDECAR" ]; then
        echo "✅ Sidecar is bundled with Tauri binary"
    else
        echo "⚠️  Sidecar not found next to Tauri binary"
        echo "   Expected at: $EXPECTED_SIDECAR"
    fi
fi

echo ""
echo "=========================================="
echo "✅ Integration test complete!"
echo ""
echo "Next steps:"
echo "1. Run Sery Link: ../src-tauri/target/release/sery-link"
echo "2. Add a watched folder containing DOCX files"
echo "3. Check logs for: [scanner] ✅ MarkItDown sidecar converted"
