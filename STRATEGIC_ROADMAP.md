# Sery Link - Strategic Roadmap (Post-MVP)

**Status**: v0.3.0 SQL Recipe Marketplace Complete ✅ (100% - 19/19 tasks)
**Launch Date**: January 2024
**Last Updated**: January 2025

This document merges insights from:
1. Gemini's Strategic Review (THE_OBSIDIAN_STRATEGY_FOR_SERY.md)
2. Obsidian-Inspired Improvements (OBSIDIAN_INSPIRED_IMPROVEMENTS.md)
3. Current shipped state (Phase 1-5 complete, Phase 6 backend ready)

---

## Strategic North Star

**Core Insight**: Sery Link is not a "data analytics tool" - it's a **data sovereignty bridge**. Like Obsidian did for notes, we do for data lakes: your data never leaves your machine, but you get enterprise-grade analytics.

### The Moat (Already Shipped ✅)

1. **Zero-cost scaling** - Tunnel mode executes queries locally with zero S3 egress fees
2. **Privacy-first architecture** - Raw data never uploaded, only metadata synced
3. **Local-first UX** - Command Palette (Cmd+K), offline search, instant response
4. **Plugin ecosystem** - WebAssembly sandboxing, 5 production plugins

### The Gap (Gemini's Key Insight 🔥)

**"Sell the Bridge, Not the Island"** - We built the infrastructure but haven't packaged the use cases.

**Missing piece**: SQL Recipe Marketplace
- Pre-built analysis templates for common data sources (Shopify, Stripe, Google Analytics, Snowflake exports)
- Copy-paste-run SQL recipes for common questions ("What's my churn rate?", "Who are my power users?")
- Community-contributed recipes with ratings and reviews
- Monetization layer: FREE recipes for common patterns, PRO recipes for advanced analysis

**Why this matters**: Users come to Sery Link because they have a question, not because they want to "set up a data analytics tool." Recipes answer the question immediately.

---

## Architectural Distinction

### MCP Plugins vs SQL Recipes (Complementary, Not Competing)

**MCP Plugins** (already shipped):
- **Purpose**: Extend file format support (CSV, JSON, HTML parsers)
- **Audience**: Technical users who need custom data transformations
- **Distribution**: Plugin marketplace (v0.2.0 UI coming)
- **Monetization**: Optional (plugin authors can charge)

**SQL Recipes** (v0.3.0):
- **Purpose**: Pre-built analysis templates for common data sources
- **Audience**: Business users who want answers, not infrastructure
- **Distribution**: Recipe marketplace (search by data source + question)
- **Monetization**: Core revenue driver (PRO tier unlocks advanced recipes)

**Example flow**:
1. User watches folder with Shopify CSV exports (MCP plugin parses CSV)
2. User searches "What's my best-selling product?" (SQL recipe generates query)
3. User clicks recipe → instant answer (no SQL knowledge required)

Both ecosystems feed each other: plugins expand data sources, recipes expand use cases.

---

## Pricing Model (Gemini + Existing Plan Merged)

### FREE Tier
- Unlimited local folders watched
- Tunnel mode (zero upload, local query execution)
- Basic metadata sync (schema + file stats)
- 5 built-in plugins
- 10 free SQL recipes (common patterns)
- Query history (local JSONL, last 1000 entries)

### PRO Tier ($15/month)
- Everything in FREE
- Unlimited SQL recipes (entire marketplace)
- Cloud query execution mode (for large datasets that don't fit locally)
- Advanced relationship graph (foreign key detection, join suggestions)
- Priority support
- Custom plugin marketplace (install community plugins)
- Export query results to Excel/CSV

### TEAM Tier ($50/month, up to 10 users)
- Everything in PRO
- Shared folders (team members see same datasets)
- Collaborative query history (team members share queries)
- Team recipe library (private recipes for the org)
- Role-based access control (viewer, analyst, admin)
- Audit logs (who ran what query when)
- SSO integration (Google Workspace, Okta)

**Key metric**: FREE users who discover recipes they need in PRO → conversion to paid

---

## Roadmap

### v0.2.0 - Marketplace UI (1-2 weeks) [NEXT]

**Goal**: Make the marketplace infrastructure usable

**Shipped in Phase 6**:
- ✅ MarketplaceRegistry (search, filter, sort)
- ✅ PluginInstaller (async install framework)
- ✅ 6 Tauri commands (load, search, featured, popular, get, install)
- ✅ 3 passing unit tests

**Remaining work**:
- [ ] Frontend marketplace browser component (React)
- [ ] Plugin detail pages (manifest display, screenshots, ratings)
- [ ] One-click install from UI (call existing Tauri commands)
- [ ] HTTP/GitHub download implementation (complete PluginInstaller)
- [ ] Seed marketplace.json with 3-5 community plugins

**Success criteria**: User can search for "CSV parser", see plugin details, click "Install", and use it immediately.

---

### v0.3.0 - SQL Recipe Marketplace (2-3 weeks) [FLAGSHIP] ✅ COMPLETE

**Goal**: "Sell the Bridge" - turn infrastructure into packaged value

**Phase A: Recipe Infrastructure** ✅
1. **Recipe schema** (JSON):
   ```json
   {
     "id": "shopify-churn-rate",
     "name": "Calculate Monthly Churn Rate",
     "description": "Analyzes customer purchase history to compute churn",
     "data_source": "Shopify",
     "required_tables": ["orders", "customers"],
     "sql_template": "SELECT ...",
     "parameters": [
       {"name": "start_date", "type": "date", "default": "30 days ago"},
       {"name": "min_orders", "type": "int", "default": 2}
     ],
     "tier": "FREE",
     "author": "Sery Team",
     "rating": 4.8,
     "download_count": 1234
   }
   ```

2. **Recipe executor** (Rust):
   - Parse recipe JSON
   - Render SQL template with user-provided parameters
   - Validate against detected schema (check required tables exist)
   - Execute via existing DuckDB executor
   - Return results with recipe metadata

3. **Recipe marketplace API** (FastAPI):
   - Search recipes by data source + keyword
   - Filter by tier (FREE/PRO)
   - Track downloads + ratings
   - User-contributed recipe submission (moderation queue)

**Phase B: Seed Recipes (9 total: 5 FREE, 4 PRO)** ✅

**FREE recipes** (common patterns, 5 shipped):
- Shopify: Top products by revenue, monthly sales trend
- Stripe: MRR calculation, failed payment summary
- Google Analytics: Top pages by traffic, bounce rate by source
- CSV: Duplicate detection, date range filtering

**PRO recipes** (advanced analysis, 4 shipped):
- Shopify: Cohort analysis, customer lifetime value, inventory turnover
- Stripe: Revenue retention curves, expansion MRR, payment method breakdown
- Google Analytics: Funnel conversion rates, segment comparison, attribution modeling
- Multi-source JOINs: Shopify + Stripe (revenue reconciliation), GA + Shopify (attribution to revenue)

**Phase C: Frontend Recipe UI** ✅
- Recipe search bar (autocomplete by data source + question)
- Recipe detail page (description, SQL preview, parameter inputs)
- Parameter form (date pickers, number inputs, text fields)
- "Run Recipe" button → displays results table
- "Save Recipe" → bookmark for later
- Rating/review system (5-star + optional comment)

**Success criteria**: User with Shopify CSVs can search "churn rate", find recipe, fill in dates, and get answer in <30 seconds.

---

### v0.4.0 - Mobile Apps (3-4 weeks) [EXPANSION]

**Goal**: Obsidian-style mobile companion apps

**iOS/Android apps** (Tauri supports both):
- Same folder watching logic (iCloud Drive, Google Drive sync)
- Same plugin system (WASM runs on mobile)
- Same recipe marketplace (search + execute)
- Mobile-optimized UI (bottom sheets, swipe gestures)
- Offline-first (local SQLite cache)

**Use case**: Analyst on the go can run saved recipes from phone, get instant charts.

**Success criteria**: Analyst can open app on phone, tap "MRR this month" recipe, see chart in <5 seconds.

---

### v0.5.0 - Self-Hosted Backend (4-6 weeks) [ENTERPRISE]

**Goal**: TEAM tier value prop

**Architecture**:
- Docker Compose for easy deployment (Postgres, Redis, FastAPI)
- Same API contract as cloud version (seamless migration)
- Shared folder sync (WebSocket tunnel to team members)
- Team recipe library (private recipes, not in public marketplace)
- Audit logs (query history, recipe usage, user actions)

**Use case**: Enterprise team with strict data residency requirements can deploy Sery backend on-prem, get all TEAM features without cloud dependency.

**Success criteria**: DevOps can run `docker-compose up` and get working Sery backend in <10 minutes.

---

## Metrics to Watch

### Leading Indicators (User Behavior)
- **Command Palette usage** - Are users discovering keyboard shortcuts?
- **Recipe search CTR** - Do users find recipes relevant to their data?
- **Plugin install rate** - Are plugins being discovered and used?
- **Query repeat rate** - Are users re-running saved queries?

### Lagging Indicators (Revenue)
- **FREE → PRO conversion** - What % of FREE users hit recipe paywall and convert?
- **PRO retention** - Do PRO users stick after month 1?
- **TEAM expansion** - Do TEAM accounts add more seats over time?

### Vanity Metrics (Don't Optimize For)
- Total signups (if they never run a query, doesn't matter)
- Total plugins downloaded (if they're not used, doesn't matter)
- Total recipes viewed (if they're not run, doesn't matter)

---

## Open Questions

### Product
1. **Recipe authorship model** - Allow community contributions from day 1, or curate seed set first?
2. **Recipe versioning** - If a user saves a recipe and we update the SQL, do they get the new version?
3. **Multi-dataset recipes** - Should recipes support JOINs across different data sources (Shopify + Stripe)?

### Business
1. **Recipe marketplace economics** - Should community authors get revenue share (like App Store)?
2. **Enterprise sales motion** - Do we need a TEAM trial (14 days all features) or start with self-serve?
3. **Pricing experiments** - Should we A/B test PRO at $10 vs $15 vs $20?

### Technical
1. **Recipe sandboxing** - Should recipes run in WASM sandbox (like plugins) for security?
2. **Recipe caching** - Should we cache recipe results (with TTL) to avoid re-running expensive queries?
3. **Mobile offline sync** - How much of the recipe marketplace should be cached locally on mobile?

---

## Next Actions (Immediate)

**Week 1-2: v0.2.0 Marketplace UI**
1. Build React marketplace browser component
2. Implement plugin detail pages
3. Wire up one-click install to existing Tauri commands
4. Seed marketplace.json with 3 community plugins
5. Ship and announce on Twitter/HN

**Week 3-4: v0.3.0 Recipe Infrastructure**
1. Design recipe JSON schema
2. Build RecipeExecutor in Rust
3. Build recipe marketplace API (FastAPI)
4. Create 10 FREE seed recipes (Shopify, Stripe, GA, CSV)

**Week 5-6: v0.3.0 Recipe UI + PRO Tier**
1. Build recipe search UI
2. Build recipe detail page with parameter inputs
3. Create 20 PRO recipes (cohort analysis, LTV, attribution)
4. Ship PRO tier paywall (Stripe integration)
5. Launch SQL Recipe Marketplace

**Week 7+: v0.4.0 Mobile Apps**
1. Tauri mobile build setup (iOS + Android)
2. Mobile-optimized UI components
3. App Store + Play Store submission

---

## Why This Wins

**Obsidian proved**: Users will pay for data sovereignty, even when free alternatives exist (Notion, Evernote).

**Sery Link's advantage**: We're the only local-first data analytics tool with:
1. Zero upload (tunnel mode) - no S3 egress fees
2. Plugin ecosystem (extend to any file format)
3. SQL recipes (answers, not infrastructure)
4. Professional UX (keyboard-first, command palette)

**The wedge**: Analyst with sensitive data (healthcare, finance, legal) tries Sery Link because "data never leaves my machine" → discovers recipes answer their questions instantly → converts to PRO for advanced recipes → brings their team (TEAM tier).

**The moat**: Recipe marketplace compounds over time (more recipes → more use cases → more users → more recipes). Plugins compound in parallel (more file formats → more data sources → more recipes).

**The business**: FREE tier is marketing (prove the value), PRO tier is revenue (unlock advanced use cases), TEAM tier is expansion (land-and-expand within orgs).

---

## Risks & Mitigations

### Risk 1: Recipe quality inconsistent
**Impact**: Users try a recipe, get wrong results, lose trust
**Mitigation**: Curate seed recipes ourselves, test against real data, require test suite for community recipes

### Risk 2: Pricing too high for solo users
**Impact**: FREE → PRO conversion fails
**Mitigation**: Start with 10 FREE recipes (prove value), make PRO $15/month (cheaper than ChatGPT Plus), offer annual discount (2 months free)

### Risk 3: Plugin ecosystem doesn't take off
**Impact**: Stuck with 5 built-in plugins forever
**Mitigation**: Make plugin authoring stupid easy (CLI scaffolding tool), showcase top plugins in app, revenue share with authors

### Risk 4: Mobile apps add too much complexity
**Impact**: Delay v0.3.0 SQL recipes (more important)
**Mitigation**: Ship mobile AFTER recipes prove product-market fit, use Tauri's mobile support (same codebase)

### Risk 5: Self-hosted backend maintenance burden
**Impact**: Team spends time on DevOps instead of product
**Mitigation**: Only ship self-hosted for TEAM tier (revenue justifies support cost), provide Docker Compose (easy updates)

---

## Success Looks Like

**6 months from now**:
- 1,000 active users (FREE tier)
- 100 PRO subscribers ($1,500 MRR)
- 5 TEAM accounts ($2,500 MRR)
- **Total MRR: $4,000**
- 50 SQL recipes (30 FREE, 20 PRO)
- 10 community plugins
- App Store + Play Store presence

**12 months from now**:
- 10,000 active users
- 500 PRO subscribers ($7,500 MRR)
- 20 TEAM accounts ($10,000 MRR)
- **Total MRR: $17,500**
- 100 SQL recipes (50 FREE, 50 PRO)
- 25 community plugins
- 1st enterprise deal ($50k ARR)

**The vision**: Analyst opens Sery Link, searches "What's my churn rate?", picks a recipe, gets the answer in 30 seconds. No SQL, no config, no upload. Just answers. Like Obsidian made note-taking frictionless, we make data analysis frictionless.

**Ship it.** 🚀
