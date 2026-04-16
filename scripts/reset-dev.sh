#!/bin/bash
# Reset Sery Link to fresh state for testing

set -e

echo "🔄 Resetting Sery Link to fresh state..."

# Remove config directory
if [ -d "$HOME/.seryai" ]; then
    echo "📁 Removing config directory..."
    rm -rf "$HOME/.seryai"
    echo "✅ Config directory removed"
else
    echo "ℹ️  No config directory found"
fi

# Remove keyring entries (macOS)
if command -v security &> /dev/null; then
    echo "🔑 Removing keyring entries..."
    security delete-generic-password -s "com.sery.link" -a "access_token" 2>/dev/null && echo "✅ Keyring entry removed" || echo "ℹ️  No keyring entry found"
else
    echo "⚠️  Keyring removal skipped (not macOS)"
fi

# Clean build cache (optional)
read -p "Clean build cache? (y/N) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "🧹 Cleaning build cache..."
    rm -rf node_modules/.vite
    rm -rf src-tauri/target/debug
    echo "✅ Build cache cleaned"
fi

echo ""
echo "✅ Reset complete!"
echo ""
echo "Next steps:"
echo "  1. Run: npm run tauri dev"
echo "  2. Select 'Local Vault' mode in onboarding"
echo "  3. Test fresh install flow"
echo ""
