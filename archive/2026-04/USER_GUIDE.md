# Sery Link User Guide

**Version 0.4.0 - Three-Tier Strategy**

Welcome to Sery Link! This guide will help you get started with local-first data analysis in under 60 seconds.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Understanding the Three Tiers](#understanding-the-three-tiers)
3. [Installation](#installation)
4. [First-Time Setup](#first-time-setup)
5. [Adding Your Data](#adding-your-data)
6. [Running Recipes](#running-recipes)
7. [Upgrading Your Tier](#upgrading-your-tier)
8. [Troubleshooting](#troubleshooting)

---

## Quick Start

**Goal:** Run your first data query in under 60 seconds.

1. **Install** - Double-click `Sery Link_0.4.0_aarch64.dmg` and drag to Applications
2. **Launch** - Open Sery Link from your Applications folder
3. **Choose Mode** - Select "Local Vault (FREE)" for zero authentication
4. **Add Folder** - Click "Add Folder" and select a folder with data files
5. **Run Recipe** - Pick a FREE recipe and execute it

Done! You just analyzed data without creating an account.

---

## Understanding the Three Tiers

Sery Link offers three authentication modes, each unlocking different features:

### 🏠 **Tier 1: Local Vault (FREE)**

**No account required. Your data never leaves your machine.**

✅ What you get:
- **5 FREE recipes** (pre-built SQL queries for common analytics)
- **Local SQL queries** - Run DuckDB queries on your files
- **File watching** - Auto-detect new data files
- **Query history** - Track what you've analyzed
- **Zero authentication** - No login, no signup, no tracking

❌ What you don't get:
- PRO recipes (4 advanced analytics recipes)
- AI-powered natural language queries
- Cloud sync across devices
- Team collaboration

**Perfect for:** Privacy-focused users, local data exploration, trying Sery Link

---

### 🔑 **Tier 2: BYOK (Bring Your Own Key)**

**Use your own Anthropic API key for AI features.**

Everything from Local Vault, **plus:**
- ✅ **All 9 recipes** (5 FREE + 4 PRO)
- ✅ **AI-powered queries** - Ask questions in plain English
- ✅ **Advanced analytics** - LTV, cohort analysis, funnel analysis
- ✅ **Your own AI credits** - Use your Anthropic API key directly

❌ Still missing:
- Cloud sync across devices
- Team collaboration
- Performance mode (cloud-accelerated queries)

**Perfect for:** Power users, developers with API keys, teams wanting AI without centralized accounts

**Cost:** You pay Anthropic directly for AI usage (~$0.10-$1 per 100 queries, depending on complexity)

---

### ☁️ **Tier 3: Workspace (FULL)**

**Full Sery.ai integration with cloud sync and team features.**

Everything from BYOK, **plus:**
- ✅ **Cloud sync** - Access your data from any device
- ✅ **Team sharing** - Collaborate with teammates
- ✅ **Performance mode** - Cloud-accelerated queries for large datasets
- ✅ **Managed AI credits** - No need to manage API keys
- ✅ **Priority support** - Faster response times

**Perfect for:** Teams, multi-device users, production workflows

**Cost:** Sery.ai pricing (includes AI credits, storage, and team features)

---

## Installation

### macOS (Apple Silicon)

1. **Download** the DMG installer:
   ```
   Sery Link_0.4.0_aarch64.dmg
   ```

2. **Mount** - Double-click the DMG file

3. **Install** - Drag "Sery Link.app" to your Applications folder

4. **Launch** - Open Sery Link from Applications

5. **macOS Security** - If you see "Sery Link cannot be opened":
   - Open **System Settings** > **Privacy & Security**
   - Scroll down and click **"Open Anyway"**
   - Confirm by clicking **"Open"**

---

## First-Time Setup

### Step 1: Welcome Screen

You'll see a welcome screen explaining Sery Link's three tiers. Click **"Get Started"**.

### Step 2: Choose Your Mode

You have three options:

#### Option A: **Local Vault (FREE)** - Recommended for first-time users

1. Click the **"Local Vault (FREE)"** card
2. Click **"Continue"**
3. Skip to Step 3 (no authentication needed)

#### Option B: **Bring Your Own API Key (BYOK)**

1. Click the **"Use My API Key"** card
2. Enter your Anthropic API key
   - Get one at: https://console.anthropic.com/
   - Format: `sk-ant-...`
3. Click **"Connect"**

#### Option C: **Sery Workspace (FULL)**

1. Click the **"Sery Workspace (PRO)"** card
2. Click **"Sign in with Sery.ai"**
3. A browser window will open for authentication
4. Sign in with your Sery.ai account
5. Copy the workspace key shown
6. Paste it into Sery Link
7. Click **"Connect"**

### Step 3: Add Your First Folder (Optional)

1. Click **"Add Folder"** or **"Skip for now"**
2. If adding, select a folder containing:
   - **Parquet files** (`.parquet`, `.pq`)
   - **CSV files** (`.csv`)
   - **Excel files** (`.xlsx`, `.xls`)
   - **Documents** (`.pdf`, `.docx`, `.pptx`, `.html`)

3. Sery Link will scan the folder and detect all data files
4. Click **"Continue"**

### Step 4: Privacy Overview

Review what data leaves your device (spoiler: almost nothing in Local Vault mode).

Click **"Continue"**.

### Step 5: You're All Set!

Choose:
- **"Open Sery"** - Opens sery.ai in your browser (if using Workspace mode)
- **"I'll explore the app first"** - Starts using Sery Link immediately

**The app is now running in your menu bar!**

---

## Adding Your Data

### From the Main Window

1. Click the **Folders** tab (left sidebar)
2. Click **"+ Add Folder"**
3. Select a folder with data files
4. Choose settings:
   - **Watch for changes** - Auto-detect new files (recommended)
   - **Scan subfolders** - Include nested folders
5. Click **"Add"**

Sery Link will:
- Scan all files in the folder
- Detect schemas automatically
- Index metadata for fast searching
- Watch for changes (if enabled)

### Supported File Types

| Type | Extensions | Notes |
|------|-----------|-------|
| Parquet | `.parquet`, `.pq` | Best performance |
| CSV | `.csv`, `.tsv` | Auto-detects delimiters |
| Excel | `.xlsx`, `.xls` | Reads all sheets |
| Documents | `.pdf`, `.docx`, `.pptx`, `.html` | Converted to markdown |

### Folder Scan Progress

- **Small folders** (< 100 files): Instant
- **Medium folders** (100-1000 files): 5-30 seconds
- **Large folders** (1000+ files): 1-5 minutes

You'll see a progress indicator in the status bar.

---

## Running Recipes

**Recipes** are pre-built SQL queries for common analytics tasks.

### Available FREE Recipes (5)

1. **CSV Time Series Aggregation** - Group time-series data by date
2. **GA Traffic Sources** - Analyze Google Analytics traffic
3. **Shopify Churn Rate** - Calculate customer churn
4. **Shopify Top Products** - Find best-selling products
5. **Stripe MRR** - Calculate Monthly Recurring Revenue

### Available PRO Recipes (4)

*Requires BYOK or Workspace mode*

6. **GA Funnel Analysis** - Track user conversion funnels
7. **Shopify Customer LTV** - Calculate Customer Lifetime Value
8. **Shopify Product Affinity** - Find products often bought together
9. **Stripe Cohort Retention** - Analyze customer retention by cohort

### How to Run a Recipe

1. Click the **Recipes** icon in the left sidebar (looks like a book)
2. Browse available recipes
3. Click a recipe card to see details
4. Click **"Execute Recipe"**
5. Fill in parameters (if required):
   - **Dataset** - Select which file/folder to analyze
   - **Date range** - Filter by time period
   - **Filters** - Additional constraints
6. Click **"Run"**
7. Results appear in the main panel
8. Export results as CSV or copy to clipboard

### Recipe Parameters

Each recipe requires different inputs:

**Example: Shopify Churn Rate**
- **Customer data file** - Select your customers CSV/Parquet
- **Time period** - Last 30 days, 90 days, or custom
- **Active threshold** - Define "active customer" (e.g., 1+ purchase)

**Example: Stripe MRR**
- **Subscription data** - Select subscriptions file
- **Currency** - USD, EUR, GBP, etc.
- **Group by** - Daily, weekly, monthly

### Recipe Results

Results include:
- **Summary stats** - Key metrics at a glance
- **Data table** - Full query results (up to 10,000 rows)
- **Visualizations** - Charts and graphs (if recipe includes them)
- **SQL query** - The actual query that ran (for learning/debugging)

---

## Upgrading Your Tier

### From Local Vault → BYOK

1. Click your profile icon (top right)
2. Select **"Upgrade to PRO"**
3. Choose **"Use My API Key"**
4. Enter your Anthropic API key
5. Click **"Upgrade"**

You now have access to all 9 recipes and AI-powered queries!

### From Local Vault → Workspace

1. Click your profile icon (top right)
2. Select **"Upgrade to Workspace"**
3. Click **"Sign in with Sery.ai"**
4. Complete authentication in your browser
5. Paste workspace key into Sery Link
6. Click **"Connect"**

You now have cloud sync, team features, and performance mode!

### From BYOK → Workspace

1. Click your profile icon (top right)
2. Select **"Connect Workspace"**
3. Follow the authentication flow
4. Your existing folders and history will sync to the cloud

**Note:** You can switch between modes at any time in Settings.

---

## Menu Bar Features

Sery Link runs in your macOS menu bar (top-right corner).

### Menu Bar Icon States

- **Green checkmark** - Connected and syncing
- **Yellow sync icon** - Syncing data
- **Red X** - Connection error
- **Gray** - Offline mode

### Menu Bar Actions

Click the icon to:
- **Show Main Window** - Open the full app
- **Quick Actions** - Run common tasks
- **Pause Syncing** - Temporarily stop file watching
- **Quit** - Close Sery Link

---

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Cmd+K` | Open Command Palette |
| `Cmd+N` | Add new folder |
| `Cmd+F` | Search recipes/datasets |
| `Cmd+R` | Refresh current view |
| `Cmd+,` | Open Settings |
| `Cmd+Q` | Quit Sery Link |

**Command Palette** (`Cmd+K`) gives you fuzzy search for:
- Recipes
- Datasets
- Folders
- Settings
- Actions

---

## Settings

### General

- **Theme** - Light, Dark, or System
- **Launch at login** - Start Sery Link when macOS boots
- **Minimize to menu bar** - Hide window instead of quitting

### Authentication

- **Current mode** - Shows your current tier (LocalOnly, BYOK, Workspace)
- **Switch mode** - Change authentication method
- **Sign out** - Disconnect workspace (returns to LocalOnly)

### Folders

- **Watch interval** - How often to check for new files (default: 30s)
- **Exclude patterns** - Files/folders to ignore (e.g., `node_modules`, `.git`)
- **Max file size** - Skip files larger than this (default: 1 GB)

### Privacy

- **Data collection** - What data Sery Link sends (if any)
- **Analytics** - Anonymous usage statistics (opt-in)
- **Crash reports** - Help us fix bugs (opt-in)

### Advanced

- **DuckDB memory limit** - Max RAM for query execution (default: 4 GB)
- **Query timeout** - Auto-cancel slow queries (default: 60s)
- **Cache size** - Query result cache (default: 100 MB)

---

## Troubleshooting

### App won't open - "Sery Link cannot be opened"

**Cause:** macOS Gatekeeper blocking unsigned app

**Solution:**
1. Open **System Settings** > **Privacy & Security**
2. Scroll down to **Security**
3. Click **"Open Anyway"** next to the Sery Link message
4. Click **"Open"** in the confirmation dialog

### Onboarding stuck on "You're all set"

**Cause:** Fixed in v0.4.0, but if using older version:

**Solution:**
1. Force quit Sery Link (`Cmd+Q` or right-click menu bar icon → Quit)
2. Reinstall from the latest DMG
3. Restart onboarding

### "Generic exec icon" in Login Items

**Cause:** LaunchAgent not associated with app bundle (development artifact)

**Solution:**
1. Open **System Settings** > **General** > **Login Items & Extensions**
2. Remove "SeryLink" from the list
3. In Sery Link, go to **Settings** > **General**
4. Toggle **"Launch at login"** off and back on
5. The icon should now appear correctly

### Folder scan stuck at 0%

**Possible causes:**
1. **Very large folder** - 10,000+ files can take 5-10 minutes
2. **Permission denied** - Sery Link can't read the folder
3. **Network drive** - Mounted drives are slower

**Solution:**
1. Wait 2-3 minutes for large folders
2. Check folder permissions (right-click folder → Get Info)
3. For network drives, copy files locally first

### Recipe shows "requires PRO tier"

**Cause:** You're in Local Vault mode trying to run a PRO recipe

**Solution:**
1. Upgrade to BYOK or Workspace mode
2. Or use one of the 5 FREE recipes instead

### Query returns no results

**Possible causes:**
1. **Empty dataset** - File has no rows
2. **Filters too strict** - Date range or WHERE clause excludes all rows
3. **Wrong file selected** - Selected file doesn't match recipe expectations

**Solution:**
1. Check file preview - does it have data?
2. Remove filters and try again
3. Verify file schema matches recipe (e.g., GA recipes need GA export format)

### "Out of memory" error

**Cause:** Query processing too much data for available RAM

**Solution:**
1. Add filters to reduce data size (e.g., date range)
2. Increase DuckDB memory limit in **Settings** > **Advanced**
3. Close other apps to free RAM
4. For very large datasets, use Parquet instead of CSV (10x faster)

### App slow or unresponsive

**Common causes:**
1. **Large query running** - Check status bar for "Executing query..."
2. **Folder scan in progress** - Check status bar for "Scanning folder..."
3. **Too many folders watched** - Each folder adds overhead

**Solution:**
1. Wait for current operation to complete
2. Reduce number of watched folders
3. Exclude large subdirectories (e.g., `node_modules`)
4. Restart Sery Link

### Cannot connect to Sery.ai (Workspace mode)

**Possible causes:**
1. **No internet connection**
2. **Firewall blocking WebSocket**
3. **Workspace key expired**

**Solution:**
1. Check internet connection
2. Allow Sery Link in macOS Firewall (**System Settings** > **Network** > **Firewall**)
3. Re-authenticate in **Settings** > **Authentication**

---

## Getting Help

### Documentation

- **This User Guide** - `USER_GUIDE.md`
- **Developer Quickstart** - `DEVELOPER_QUICKSTART.md`
- **Testing Guide** - `TESTING_v0.4.0.md`
- **Changelog** - `CHANGELOG.md`

### Support

- **Email:** support@sery.ai
- **GitHub Issues:** https://github.com/seryai/sery-link/issues
- **Documentation:** https://sery.ai/docs

### FAQ

**Q: Is my data uploaded to the cloud?**
A: In **Local Vault** mode, NO. Your data never leaves your machine. In **Workspace** mode, you can optionally enable "Performance Mode" which uploads data for faster queries, but this is OFF by default.

**Q: What's the difference between BYOK and Workspace?**
A: **BYOK** uses your own Anthropic API key (you pay Anthropic directly). **Workspace** uses managed AI credits from Sery.ai (included in subscription). Workspace also adds cloud sync and team features.

**Q: Can I use Sery Link offline?**
A: **Local Vault** and **BYOK** modes work completely offline for SQL queries. AI-powered queries require internet (to call Anthropic API). **Workspace** mode requires internet for syncing but caches data locally.

**Q: How much does it cost?**
A: **Local Vault** is FREE forever. **BYOK** costs whatever you pay Anthropic for API usage (~$0.10-$1 per 100 queries). **Workspace** pricing: https://sery.ai/pricing

**Q: Can I switch modes later?**
A: Yes! You can switch between modes at any time in **Settings** > **Authentication**. Your folders and history are preserved.

**Q: What happens to my data if I downgrade from Workspace to Local Vault?**
A: All data stays in your local folders. Cloud-synced query history and settings will no longer sync, but remain accessible on each device independently.

**Q: Can I use multiple API keys (BYOK)?**
A: Currently, only one API key is supported at a time. You can change it in Settings.

**Q: Does Sery Link support Windows or Linux?**
A: Not yet. Currently macOS Apple Silicon only. Windows and Linux support is planned for v0.5.0.

---

## Tips & Best Practices

### For Local Vault Users

1. **Start small** - Add one folder at a time to learn the interface
2. **Use Parquet** - 10x faster than CSV for large datasets
3. **FREE recipes first** - Explore all 5 FREE recipes before upgrading
4. **Export results** - Save query results as CSV for further analysis

### For BYOK Users

1. **Monitor API usage** - Check Anthropic console for usage/costs
2. **Cache queries** - Sery Link caches results to save API calls
3. **Use PRO recipes** - They're optimized and tested (better than ad-hoc AI queries)
4. **Start simple** - Test with small datasets before running complex queries

### For Workspace Users

1. **Enable Performance Mode** - For datasets > 1 GB (Settings > Advanced)
2. **Share wisely** - Only share folders you want teammates to access
3. **Use team recipes** - Create custom recipes your team can reuse
4. **Set up alerts** - Get notified when queries fail or data changes

### General Tips

1. **Name folders clearly** - "Shopify Orders 2024" is better than "orders"
2. **Use consistent schemas** - Easier to write recipes if files match
3. **Archive old data** - Move historical files to separate folders
4. **Check query history** - Learn from past queries (History tab)
5. **Learn SQL basics** - Helps debug recipe parameters
6. **Use Command Palette** - `Cmd+K` is the fastest way to do anything

---

## What's Next?

### Recommended Learning Path

1. ✅ **Complete onboarding** (you're here!)
2. ✅ **Add your first folder** (< 5 minutes)
3. ✅ **Run a FREE recipe** (< 1 minute)
4. ⏭️ **Explore query history** - See what SQL ran
5. ⏭️ **Try different recipes** - Learn what each does
6. ⏭️ **Add more folders** - Analyze different data sources
7. ⏭️ **Upgrade to BYOK or Workspace** - Unlock PRO features

### v0.5.0 Preview (Coming Soon)

- **More FREE recipes** (10 total)
- **Custom recipe builder** - Create your own recipes with AI
- **Data visualizations** - Charts and graphs in-app
- **Windows & Linux support** - Cross-platform
- **Team workspaces** - Collaborate with teammates
- **Scheduled queries** - Run recipes automatically

---

## Version History

**v0.4.0 (Current)** - Three-Tier Strategy
- Added Local Vault mode (zero authentication)
- Added BYOK mode (bring your own API key)
- 5 FREE recipes, 4 PRO recipes
- < 60 second onboarding
- Fixed onboarding stuck issue
- Fixed menu bar icon display

**v0.3.0** - Workspace mode only (previous version)

---

**Thank you for using Sery Link!**

If you have feedback, questions, or feature requests, we'd love to hear from you: support@sery.ai

