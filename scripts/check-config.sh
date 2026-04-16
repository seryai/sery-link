#!/bin/bash
# Check current Sery Link configuration

set -e

CONFIG_FILE="$HOME/.seryai/config.json"

echo "🔍 Sery Link Configuration Check"
echo "=================================="
echo ""

# Check if config exists
if [ ! -f "$CONFIG_FILE" ]; then
    echo "❌ Config file not found"
    echo "   Expected: $CONFIG_FILE"
    echo ""
    echo "This is normal for a fresh install."
    echo "Run the app once to create the config."
    exit 0
fi

echo "✅ Config file found"
echo "   Location: $CONFIG_FILE"
echo ""

# Check if jq is installed
if ! command -v jq &> /dev/null; then
    echo "⚠️  jq not installed (for pretty printing)"
    echo "   Install: brew install jq"
    echo ""
    echo "Raw config:"
    cat "$CONFIG_FILE"
    exit 0
fi

# Parse and display config
echo "📋 Configuration Details"
echo "------------------------"

# App settings
echo "🎨 App Settings:"
THEME=$(jq -r '.app.theme' "$CONFIG_FILE")
echo "  Theme: $THEME"

FIRST_RUN=$(jq -r '.app.first_run_completed' "$CONFIG_FILE")
if [ "$FIRST_RUN" = "true" ]; then
    echo "  First run: ✅ Completed"
else
    echo "  First run: ⏳ Not completed (onboarding pending)"
fi

LAUNCH_AT_LOGIN=$(jq -r '.app.launch_at_login' "$CONFIG_FILE")
if [ "$LAUNCH_AT_LOGIN" = "true" ]; then
    echo "  Launch at login: ✅ Enabled"
else
    echo "  Launch at login: ❌ Disabled"
fi
echo ""

# Auth mode
echo "🔐 Authentication Mode:"
AUTH_MODE=$(jq -r '.app.selected_auth_mode.type // "Not set"' "$CONFIG_FILE")

case "$AUTH_MODE" in
    "LocalOnly")
        echo "  Mode: 🏠 LocalOnly (FREE tier)"
        echo "  Features:"
        echo "    - ✅ Free recipes (5)"
        echo "    - ✅ Local SQL queries"
        echo "    - ❌ PRO recipes"
        echo "    - ❌ Cloud sync"
        ;;
    "BYOK")
        PROVIDER=$(jq -r '.app.selected_auth_mode.provider' "$CONFIG_FILE")
        echo "  Mode: 🔑 BYOK (PRO tier)"
        echo "  Provider: $PROVIDER"
        echo "  Features:"
        echo "    - ✅ Free recipes (5)"
        echo "    - ✅ PRO recipes (4)"
        echo "    - ✅ Local SQL queries"
        echo "    - ✅ AI-powered queries"
        echo "    - ❌ Cloud sync"
        ;;
    "WorkspaceKey")
        echo "  Mode: ☁️  WorkspaceKey (FULL tier)"
        echo "  Features:"
        echo "    - ✅ All FREE & PRO recipes (9)"
        echo "    - ✅ Local SQL queries"
        echo "    - ✅ AI-powered queries"
        echo "    - ✅ Cloud sync"
        echo "    - ✅ Team collaboration"
        echo "    - ✅ Performance mode"
        ;;
    "Not set")
        echo "  Mode: ⚠️  Not configured"
        echo "  Migration may be needed"
        ;;
    *)
        echo "  Mode: ❓ Unknown ($AUTH_MODE)"
        ;;
esac
echo ""

# Watched folders
echo "📁 Watched Folders:"
FOLDER_COUNT=$(jq '.watched_folders | length' "$CONFIG_FILE")
if [ "$FOLDER_COUNT" -eq 0 ]; then
    echo "  No folders configured"
else
    echo "  Total: $FOLDER_COUNT folder(s)"
    jq -r '.watched_folders[] | "  - \(.path) (recursive: \(.recursive))"' "$CONFIG_FILE"
fi
echo ""

# Agent info
echo "🤖 Agent Information:"
AGENT_NAME=$(jq -r '.agent.name' "$CONFIG_FILE")
PLATFORM=$(jq -r '.agent.platform' "$CONFIG_FILE")
AGENT_ID=$(jq -r '.agent.agent_id // "Not registered"' "$CONFIG_FILE")
echo "  Name: $AGENT_NAME"
echo "  Platform: $PLATFORM"
echo "  Agent ID: $AGENT_ID"
echo ""

# Cloud settings
echo "☁️  Cloud Configuration:"
API_URL=$(jq -r '.cloud.api_url' "$CONFIG_FILE")
WEB_URL=$(jq -r '.cloud.web_url' "$CONFIG_FILE")
echo "  API URL: $API_URL"
echo "  Web URL: $WEB_URL"
echo ""

# Check keyring
echo "🔑 Keyring Status:"
if command -v security &> /dev/null; then
    if security find-generic-password -s "com.sery.link" -a "access_token" &> /dev/null; then
        echo "  ✅ Workspace token found in keyring"
        if [ "$AUTH_MODE" != "WorkspaceKey" ]; then
            echo "  ⚠️  Warning: Token exists but mode is $AUTH_MODE"
            echo "  Consider running migration"
        fi
    else
        echo "  ❌ No workspace token in keyring"
        if [ "$AUTH_MODE" = "WorkspaceKey" ]; then
            echo "  ⚠️  Warning: WorkspaceKey mode but no token found"
        fi
    fi
else
    echo "  ℹ️  Keyring check skipped (not macOS)"
fi
echo ""

# Recommendations
echo "💡 Recommendations:"
if [ "$AUTH_MODE" = "Not set" ]; then
    echo "  • Run the app to trigger migration"
fi

if [ "$FOLDER_COUNT" -eq 0 ]; then
    echo "  • Add watched folders to start scanning"
fi

if [ "$AUTH_MODE" = "LocalOnly" ]; then
    echo "  • Upgrade to BYOK or Workspace for PRO features"
fi

echo ""
echo "✅ Configuration check complete"
