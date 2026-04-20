# Sery Link: Fluent UX & Local-First Workflow Optimization

**Vision:** Transform Sery Link from a "Cloud-First SaaS" into a "Fluent, Local-First Data Vault" (The Obsidian for Data).

---

## 1. The Three-Tier Onboarding (Progressive Value)

To remove the "Login Wall" and build instant trust, we adopt a tiered approach to identity and features.

### Tier 1: Guest Mode (Zero Login / Pure Privacy)
*   **Identity:** Anonymous `LocalDeviceID` generated on first launch.
*   **Action:** Install → Open → "Select Folder" → **Instant Indexing**.
*   **Capabilities:**
    *   **Local Vault:** Browse file tree, see schemas, and preview data (first 100 rows).
    *   **Zero-ETL Indexer:** Background worker converts CSV/Parquet/Excel to local DuckDB views.
    *   **MarkItDown:** Parse PDFs, DOCX, and PPTX into searchable Markdown/Text in DuckDB.
    *   **Local Search:** Instant fuzzy search across filenames and column names (powered by local DuckDB).
    *   **SQL Workbench:** Run raw SQL queries against local files without any cloud roundtrip.

### Tier 2: The AI Bridge (Sign In / Trial / BYOK)
*   **Trigger:** User clicks "Ask AI" or "Sync to Web."
*   **Experience:** "Sign in for 50 free AI queries or add your own OpenAI/Anthropic key."
*   **Value Add:**
    *   **Remote Metadata:** Push file tree/schemas (not data) to `app.sery.ai` so the user can browse their local files from any browser.
    *   **AI Chat:** Natural language queries via Sery Proxy or User's own API Key.
    *   **Dashboard Sync:** Save "Recipes" and "Queries" to the cloud workspace.

### Tier 3: Sery Cloud & Teams (The Paid Service)
*   **Value Add:**
    *   **Performance Mode:** One-click sync to Sery S3 for high-speed cloud compute on massive datasets.
    *   **Team Collaboration:** Share local metadata hubs with team members.
    *   **Proactive Alerts:** Server-side anomaly detection on synced datasets.

---

## 2. The "Fluent" Interaction Model (Cloud App Workflows)

We replace "Heavy Destination" UX with "Fluid Tool" UX inspired by Dropbox, iCloud, and Raycast.

### A. The "Magic Folder" (Set it and Forget it)
*   **Concept:** Sery creates a `~/Sery Vault` folder on the user's machine.
*   **Workflow:** Drop any data file into this folder → System Tray icon pulses → File is instantly queryable.
*   **UX:** No "Add Folder" button required; the filesystem is the interface.

### B. The "Spotlight" Moment (Cmd+K)
*   **Interaction:** A global hotkey (`Cmd+K`) opens a lightweight search bar (Raycast-style).
*   **Action:** Type "Revenue by month" → Sery runs a local DuckDB query → Result shows in a small overlay.
*   **Result:** Insights in < 2 seconds without ever opening the main Dashboard window.

### C. Drag-to-Insight
*   **Workflow:** Drag a CSV from your Desktop onto the Sery Link Dock/Menu Bar icon.
*   **Action:** App opens directly to a Chat view with that file pre-selected: *"I've analyzed 'sales.csv'. What should we look for?"*

---

## 3. Technical Architecture: "Native-Core, Web-Shell"

To achieve this without a full Swift/C# rewrite, we leverage the **Tauri + Rust** stack more effectively.

### The Background Agent (Rust)
*   **Role:** A standalone process (System Tray app) that runs 24/7.
*   **Responsibility:**
    *   Watch folders for changes.
    *   Manage the persistent **Local DuckDB Index**.
    *   Run `MarkItDown` workers for unstructured data.
    *   Maintain the WebSocket Tunnel for Tier 2/3 users.
*   **Benefit:** Indexing happens even when the UI window is closed.

### The Spotlight UI (Tauri / React)
*   **Role:** A secondary, ultra-lightweight Tauri window.
*   **Responsibility:** Fast command input and "Quick Result" rendering.
*   **Benefit:** Zero-latency feeling for simple questions.

### The Main Dashboard (Next.js / React)
*   **Role:** The full analysis suite.
*   **Responsibility:** Complex charts, long-form chat, and Workspace management.
*   **Benefit:** High iteration speed for complex features.

---

## 4. Implementation Roadmap

### Phase 1: The Trust Phase (1-2 Weeks)
- [ ] **Guest Mode Bypass:** Modify Login screen to allow "Continue as Guest."
- [ ] **Local DuckDB Setup:** Initialize a local `.db` file in `Application Support`.
- [ ] **Menu Bar Identity:** Create the System Tray icon showing "Indexing Status."

### Phase 2: The Speed Phase (3-4 Weeks)
- [ ] **Cmd+K Palette:** Build the lightweight search overlay in Tauri.
- [ ] **Fuzzy Search:** Connect the palette to DuckDB's `PRAGMA word_similarity`.
- [ ] **MarkItDown Worker:** Auto-convert dropped PDFs to searchable text.

### Phase 3: The Ecosystem Phase (2+ Months)
- [ ] **MCP Server Support:** Allow local plugins (Postgres, Slack) via MCP.
- [ ] **BYOK Settings:** UI for users to input their own LLM keys.
- [ ] **Cloud Sync Toggle:** Clear "Offline/Online" indicator in the UI.

---

**Status:** Ready for Review
**Goal:** Make Sery Link the fastest way to talk to local data.
