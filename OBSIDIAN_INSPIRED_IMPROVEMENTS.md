# Obsidian-Inspired Improvement Plan for Sery Link

**Vision:** Make Sery Link the "Obsidian for Data" - the local-first analytics tool that respects user data ownership while providing optional cloud scale.

**Created:** April 14, 2026
**Status:** Planning
**Priority:** Strategic positioning + UX enhancements

---

## Executive Summary

Obsidian proved that local-first desktop apps can build $millions ARR businesses by:
1. Making the core product free and fully-functional
2. Charging for optional cloud services (sync, publish, teams)
3. Building extensibility through plugins
4. Positioning privacy as a feature, not a limitation
5. Creating exceptional user experience for power users

**Sery Link is Obsidian for data instead of notes.** This document outlines how we adopt Obsidian's winning patterns while staying true to our analytics mission.

---

## Core Philosophy

### What Obsidian Teaches Us

| Principle | Obsidian | Sery Link Application |
|-----------|----------|----------------------|
| **Local-first** | Notes stored locally, sync optional | Data stays on user's machine (Tunnel mode), cloud optional |
| **Data ownership** | Plain markdown files, readable without app | Standard formats (Parquet/CSV/Excel/DOCX), never locked in |
| **Sacred folder** | Vault is user's folder, not app's database | Watched folders are read-only, non-invasive |
| **Free core** | Desktop app free forever | Desktop agent should be permanently free |
| **Paid services** | Charge for sync ($10/mo) and publish ($20/mo) | Charge for cloud compute, storage, team features |
| **Extensibility** | 1,000+ community plugins | MCP servers as our plugin system |
| **Speed matters** | Instant search across 10K notes | Instant dataset search, offline mode |
| **Power users** | Command palette, keyboard shortcuts | Add Cmd+K palette, keyboard-first UX |

---

## Improvement Roadmap

### Phase 1: Immediate Wins (Next Sprint - 1-2 weeks)

#### 1.1 Marketing & Messaging Refresh

**Current problem:** We position as "AI-powered BI tool" which sounds like every other SaaS
**Obsidian lesson:** Lead with "Your notes, your data, forever"

**Action items:**
- [ ] Update app tagline: "Local-first data analytics. Your data never leaves your machine."
- [ ] Add "Privacy-first" badge to app header and website hero
- [ ] Homepage hero section emphasizes local-first, not AI-first
- [ ] Update README.md to lead with privacy positioning

**Copy examples:**
```markdown
Before: "AI-powered data analyst for S3 and local files"
After:  "Your data, your machine, your insights. Cloud-optional analytics."

Before: "Connect to S3 buckets and local files"
After:  "Analyze local files without uploading. Add cloud when you need scale."
```

**Files to update:**
- `sery-link/README.md`
- `website/src/app/page.tsx` (hero section)
- `app-dashboard/src/components/Landing.tsx`

---

#### 1.2 Clarify Data Ownership in UI

**Current problem:** "Add folder" could imply we're importing/moving data
**Obsidian lesson:** "Open vault" makes clear the folder is yours, not theirs

**Action items:**
- [ ] Change button text: "Add Folder" → "Watch Folder (Read-Only)"
- [ ] Add tooltip: "Sery Link never modifies your files. We only read and index."
- [ ] First-time setup wizard: "Your data stays in your folders. We're just watching."
- [ ] Settings page: Show watched folders with "Read-only access" badge

**UI mockup:**
```
┌─────────────────────────────────────────────┐
│ Watched Folders                              │
│                                              │
│ 📁 ~/Documents/SalesData          Read-only │
│    47 files • Last synced 2 min ago         │
│    [Scan Now]  [Remove]                     │
│                                              │
│ [+ Watch Folder (Read-Only)]                │
│    We never modify your files               │
└─────────────────────────────────────────────┘
```

---

#### 1.3 Keyboard Shortcuts Guide

**Obsidian lesson:** Show shortcuts on first launch, make app keyboard-friendly

**Action items:**
- [ ] Create keyboard shortcuts overlay (press `?` to show)
- [ ] Document all shortcuts in Help menu
- [ ] Add shortcuts to context menus

**Proposed shortcuts:**
```
Global:
  Cmd/Ctrl + K     → Open command palette (Phase 2)
  Cmd/Ctrl + ,     → Settings
  Cmd/Ctrl + R     → Refresh/Sync metadata
  Cmd/Ctrl + F     → Search datasets

Navigation:
  Cmd/Ctrl + 1-4   → Switch tabs (Folders/History/Privacy/Settings)
  Cmd/Ctrl + N     → New conversation (if in chat view)
  Esc              → Close modals/dropdowns

Actions:
  Cmd/Ctrl + Enter → Send message (in chat)
  Cmd/Ctrl + D     → Delete selected item
```

**Implementation:**
- Add `src/hooks/useKeyboardShortcuts.ts`
- Register shortcuts in `App.tsx`
- Show shortcuts overlay component

---

### Phase 2: Core UX Enhancements (Next Month - 3-4 weeks)

#### 2.1 Command Palette

**Obsidian lesson:** `Cmd+P` makes everything 2 keystrokes away - signature power user feature

**Specification:**

```typescript
// src/components/CommandPalette.tsx

interface Command {
  id: string;
  label: string;
  icon: React.ReactNode;
  keywords: string[];
  action: () => void;
  section: 'datasets' | 'actions' | 'navigation' | 'recent';
}

// Example commands:
const commands: Command[] = [
  // Quick actions
  { id: 'scan', label: 'Scan all folders for new datasets', section: 'actions' },
  { id: 'sync', label: 'Sync metadata to cloud', section: 'actions' },
  { id: 'add-folder', label: 'Watch a new folder...', section: 'actions' },

  // Recent queries (dynamic)
  { id: 'recent-1', label: 'Run query: "What were sales in Q4?"', section: 'recent' },

  // Dataset navigation (fuzzy search)
  { id: 'ds-sales', label: 'Go to dataset: sales_2024.csv', section: 'datasets' },
  { id: 'ds-customers', label: 'Go to dataset: customers.parquet', section: 'datasets' },

  // Navigation
  { id: 'nav-folders', label: 'Go to Folders tab', section: 'navigation' },
  { id: 'nav-history', label: 'Go to History tab', section: 'navigation' },
];
```

**UI mockup:**
```
┌─────────────────────────────────────────────────────┐
│ 🔍 Type a command or search...                      │
├─────────────────────────────────────────────────────┤
│ ACTIONS                                             │
│  ↻  Scan all folders for new datasets              │
│  ☁  Sync metadata to cloud                         │
│  +  Watch a new folder...                           │
│                                                     │
│ RECENT QUERIES                                      │
│  💬 Run query: "What were sales in Q4?"            │
│  💬 Run query: "Show me top customers"             │
│                                                     │
│ DATASETS (2 of 47)                                  │
│  📊 sales_2024.csv                                  │
│  📊 customers.parquet                               │
└─────────────────────────────────────────────────────┘
      Press ↑↓ to navigate, Enter to select
```

**Features:**
- Fuzzy search (use `fuse.js`)
- Recent commands at top
- Grouped by section
- Keyboard navigation (arrow keys, Enter)
- Works offline (local commands always available)

---

#### 2.2 Local Metadata Cache (Offline Mode)

**Obsidian lesson:** App works fully offline - search, browse, edit notes

**Current problem:** Sery Link requires cloud connection to search datasets (pgvector search)

**Solution:** Local metadata index on the agent

**Architecture:**

```rust
// src-tauri/src/local_index.rs

pub struct LocalMetadataIndex {
    db: DuckDB,  // Local DuckDB for metadata
    datasets: Vec<DatasetMetadata>,
    last_sync: DateTime<Utc>,
}

impl LocalMetadataIndex {
    // Build index from scanned folders
    pub fn build_from_scan(&mut self, scan_results: Vec<DatasetMetadata>) {
        // Store in local DuckDB
        self.db.execute("CREATE TABLE IF NOT EXISTS datasets (
            id TEXT PRIMARY KEY,
            name TEXT,
            path TEXT,
            file_format TEXT,
            columns JSON,
            row_count INTEGER,
            size_bytes INTEGER,
            last_modified TIMESTAMP
        )");

        // Insert/update datasets
        for dataset in scan_results {
            self.db.execute("INSERT OR REPLACE INTO datasets VALUES (?, ?, ?, ?, ?, ?, ?, ?)", ...);
        }
    }

    // Fuzzy search locally (instant, works offline)
    pub fn search_local(&self, query: &str) -> Vec<DatasetMetadata> {
        // Simple keyword search on name, columns, path
        self.db.execute("
            SELECT * FROM datasets
            WHERE name LIKE '%' || ? || '%'
               OR columns LIKE '%' || ? || '%'
               OR path LIKE '%' || ? || '%'
            ORDER BY last_modified DESC
            LIMIT 50
        ", query)
    }

    // Sync status
    pub fn needs_sync(&self) -> bool {
        self.last_sync < Utc::now() - Duration::minutes(5)
    }
}
```

**Benefits:**
- ✅ Works fully offline
- ✅ Instant search (no cloud roundtrip)
- ✅ Fallback when cloud is down
- ✅ Reduces cloud API calls
- ✅ Better UX (no loading spinners for dataset list)

**Implementation phases:**
1. Week 1: Local DuckDB metadata store
2. Week 2: Fuzzy search implementation
3. Week 3: Sync logic (local → cloud, cloud → local)
4. Week 4: Offline mode indicator in UI

---

#### 2.3 Enhanced Keyboard Navigation

**Action items:**
- [ ] Tab navigation for folder list (arrow keys to select)
- [ ] Enter to expand/collapse folder details
- [ ] Delete key to remove selected folder (with confirmation)
- [ ] Vim-style navigation (j/k for up/down) - optional power user feature

---

### Phase 3: Strategic Features (Next Quarter - 2-3 months)

#### 3.1 Plugin System via MCP Servers

**Obsidian lesson:** 1,000+ plugins created by community - massive ecosystem effect

**Sery Link approach:** Use Model Context Protocol (MCP) servers as our plugin architecture

**What users could build:**

1. **Custom data sources:**
   - MongoDB MCP server → query MongoDB in natural language
   - ClickHouse MCP server → real-time analytics queries
   - Snowflake MCP server → query data warehouse
   - Postgres MCP server → connect to production database

2. **Custom AI tools:**
   - Financial analysis MCP → domain-specific finance metrics
   - Marketing MCP → CAC, LTV, cohort analysis
   - Healthcare MCP → HIPAA-compliant medical data analysis

3. **Integration hooks:**
   - Slack MCP → send query results to Slack
   - Email MCP → scheduled report emails
   - Webhook MCP → trigger external systems

**Architecture:**

```
┌──────────────────────────────────────┐
│      Sery Link Desktop Agent         │
│                                      │
│  ┌────────────┐   ┌──────────────┐  │
│  │  Core AI   │   │ MCP Client   │  │
│  │  Agent     │───│ (stdio/HTTP) │  │
│  └────────────┘   └──────┬───────┘  │
└────────────────────────────┼─────────┘
                            │
         ┌──────────────────┼──────────────────┐
         │                  │                  │
    ┌────▼─────┐      ┌────▼─────┐      ┌────▼─────┐
    │ MongoDB  │      │Snowflake │      │  Slack   │
    │   MCP    │      │   MCP    │      │   MCP    │
    └──────────┘      └──────────┘      └──────────┘
```

**User experience:**

```
Settings → Plugins
┌──────────────────────────────────────────┐
│ Installed MCP Servers                     │
│                                           │
│ ☑ MongoDB Connector         [Configure]  │
│   Query MongoDB collections               │
│   Status: Connected                       │
│                                           │
│ ☑ Slack Integration         [Configure]  │
│   Send results to Slack                   │
│   Status: Authenticated                   │
│                                           │
│ [+ Add MCP Server]                        │
└──────────────────────────────────────────┘
```

**Implementation:**
- Week 1-2: MCP client in Rust (stdio transport)
- Week 3-4: MCP server discovery and configuration UI
- Week 5-6: Built-in MCP servers (Slack, email, webhooks)
- Week 7-8: Documentation for building custom MCP servers
- Week 9-10: MCP server marketplace (community directory)

**Reference:** We already have `/mcp-server/` in the monorepo - expand this to a full ecosystem.

---

#### 3.2 Dataset Relationship Graph

**Obsidian lesson:** Graph view shows connections between notes - helps discovery

**Sery Link equivalent:** Show which datasets have been queried together

**Visualization:**

```
       sales_2024.csv
             │
             ├─ JOIN ─→ customers.parquet
             │            │
             │            └─ JOIN ─→ regions.parquet
             │
             └─ GROUP BY ─→ [revenue_by_month]
```

**Data model:**

```sql
-- Track dataset relationships from query history
CREATE TABLE dataset_relationships (
    source_dataset_id TEXT,
    target_dataset_id TEXT,
    relationship_type TEXT,  -- 'join', 'union', 'derived'
    query_count INTEGER,     -- How many times they've been used together
    last_used TIMESTAMP,
    PRIMARY KEY (source_dataset_id, target_dataset_id, relationship_type)
);
```

**UI implementation:**

```typescript
// src/components/DatasetGraph.tsx

import ReactFlow from 'reactflow';

const DatasetGraphView = ({ datasets, relationships }) => {
  const nodes = datasets.map(ds => ({
    id: ds.id,
    data: { label: ds.name, format: ds.file_format, rowCount: ds.row_count },
    position: calculatePosition(ds),  // Force-directed layout
    type: 'datasetNode',
  }));

  const edges = relationships.map(rel => ({
    id: `${rel.source}-${rel.target}`,
    source: rel.source,
    target: rel.target,
    label: rel.type,  // 'JOIN', 'UNION', etc.
    animated: rel.queryCount > 5,  // Animate frequently-used connections
  }));

  return <ReactFlow nodes={nodes} edges={edges} />;
};
```

**Features:**
- Click node → show dataset details
- Click edge → show queries that created this relationship
- Filter by relationship type
- Zoom/pan
- Export as PNG/SVG

---

#### 3.3 Pricing Model Revision

**Obsidian lesson:** Free forever core, charge for optional services

**Current Sery Link pricing:**
```
Free:  Limited datasets, limited queries
Pro:   More datasets, more queries, cloud sync
Team:  Shared workspaces
```

**Proposed Obsidian-inspired pricing:**

```
┌──────────────────────────────────────────────────────┐
│ FREE FOREVER                                         │
├──────────────────────────────────────────────────────┤
│ ✓ Unlimited local datasets (Tunnel mode)            │
│ ✓ Unlimited local queries                           │
│ ✓ Desktop agent (Mac/Windows/Linux)                 │
│ ✓ Document support (DOCX, PPTX, PDF, etc.)          │
│ ✓ Basic AI analysis                                 │
│ ✓ Self-hosted backend option                        │
│                                                      │
│ Limitations:                                         │
│ ✗ No cloud sync (Performance mode)                  │
│ ✗ No team workspaces                                │
│ ✗ No anomaly detection                              │
└──────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────┐
│ PRO - $10/month                                      │
├──────────────────────────────────────────────────────┤
│ Everything in Free, plus:                            │
│ ✓ Cloud Sync - 100GB S3 storage                     │
│ ✓ Performance mode (fast cloud queries)             │
│ ✓ Advanced AI - anomaly detection, proactive alerts │
│ ✓ Query history (unlimited)                         │
│ ✓ Priority support                                  │
└──────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────┐
│ TEAM - $20/user/month                                │
├──────────────────────────────────────────────────────┤
│ Everything in Pro, plus:                             │
│ ✓ Team workspaces (hub-and-spoke)                   │
│ ✓ Shared datasets across team                       │
│ ✓ 500GB shared cloud storage                        │
│ ✓ Role-based access control                         │
│ ✓ Audit logs                                         │
└──────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────┐
│ ENTERPRISE - Custom pricing                          │
├──────────────────────────────────────────────────────┤
│ Everything in Team, plus:                            │
│ ✓ SSO/SAML                                           │
│ ✓ On-premise deployment                              │
│ ✓ SLA guarantee                                      │
│ ✓ Dedicated support                                  │
│ ✓ Custom integrations                                │
└──────────────────────────────────────────────────────┘
```

**Philosophy shift:**
- **Before:** Pay to use Sery Link
- **After:** Sery Link is free. Pay for cloud scale and team features.

**Benefits:**
- Removes friction for solo users and students
- Drives adoption (more free users → more conversions)
- Aligns with open-source/local-first community values
- Matches Obsidian's proven model

**Risks:**
- Lower ARPU (average revenue per user)
- More support burden from free users

**Mitigation:**
- Self-service docs for free tier
- Community forum for peer support
- Upgrade prompts for cloud features ("This dataset is 50GB - enable Cloud Sync for 10x faster queries")

---

#### 3.4 Community & Ecosystem

**Obsidian lesson:** Active forum, plugin showcase, monthly changelog builds loyalty

**Action items:**

1. **GitHub Discussions** (Week 1)
   - Enable Discussions on sery-link repo
   - Categories: General, Feature Requests, Plugin Development, Show & Tell
   - Pin "How to build an MCP server for Sery Link"

2. **Monthly Changelog** (Ongoing)
   - Blog post every month: "What's New in Sery Link"
   - Highlight community contributions
   - Show metrics (users, queries, datasets)

3. **MCP Server Marketplace** (Month 3)
   - Directory of community-built MCP servers
   - Rating/review system
   - Featured servers on homepage

4. **"Built with Sery" Showcase** (Month 2)
   - User success stories
   - Example: "How a solo founder analyzes 10M rows without a data team"
   - Example: "Professor uses Sery Link for research data collaboration"

5. **Public Roadmap** (Week 2)
   - GitHub Projects board: Now / Next / Later
   - Users can vote on features (GitHub reactions)
   - Transparency builds trust

---

### Phase 4: Advanced Features (6+ months)

#### 4.1 Mobile Apps (iOS/Android)

**Obsidian lesson:** Mobile apps with full sync - same vault, all devices

**Sery Link mobile use cases:**
- View datasets on mobile
- Run quick queries via voice ("Hey Siri, ask Sery what were Q4 sales")
- Get anomaly alerts on mobile (push notifications)
- Quick insights dashboard

**Architecture:**
- React Native or Flutter
- Connects to desktop agents via WebSocket tunnel (through cloud)
- Read-only view (queries only, no dataset management)

**Not a priority now** - validate desktop-first market first.

---

#### 4.2 Self-Hosted Backend Option

**Obsidian lesson:** Some users want zero cloud dependency

**Sery Link self-hosted:**
- Docker Compose deployment
- Runs entirely on user's infrastructure
- No Sery.ai cloud required
- Supports airgapped environments

**Target users:**
- Enterprises with strict data policies
- Government/healthcare (compliance)
- Self-hosting enthusiasts

**Licensing:**
- Open-source core (MIT/Apache 2.0)
- Proprietary enterprise features (SSO, RBAC, audit logs)

**Implementation:**
- Week 1-2: Simplify deployment (single Docker Compose file)
- Week 3-4: Remove hard-coded Sery.ai URLs (make configurable)
- Week 5-6: Documentation for self-hosting
- Week 7-8: Terraform/Helm charts for production deployment

---

## Success Metrics

Track these to measure Obsidian-inspired improvements:

### Adoption Metrics
- **Desktop agent installs** (target: 10,000 in 6 months)
- **Free tier retention** (target: 60% weekly active users)
- **Free → Paid conversion** (target: 5% monthly)

### Engagement Metrics
- **Command palette usage** (target: 30% of users use it weekly)
- **Keyboard shortcuts usage** (target: 50% of power users)
- **Offline mode sessions** (target: 20% of sessions work offline)

### Community Metrics
- **GitHub Discussions posts** (target: 50/month)
- **MCP servers created** (target: 20 community servers in 6 months)
- **"Built with Sery" submissions** (target: 10 case studies)

### Quality Metrics
- **Local search speed** (target: <50ms for dataset search)
- **Time to first value** (target: <5 minutes from install to first query)
- **Support tickets from free users** (target: <10% of total tickets)

---

## Technical Debt & Prerequisites

Before implementing these features:

1. **Refactor frontend state management** - Zustand → Jotai or Redux Toolkit for better devtools
2. **Add E2E tests** - Playwright tests for critical flows
3. **Performance baseline** - Establish current metrics (search speed, query latency)
4. **Analytics instrumentation** - Track feature usage (PostHog or Mixpanel)

---

## Decision Log

### Why MCP over a custom plugin API?

**Decision:** Use MCP (Model Context Protocol) as our plugin system

**Rationale:**
- MCP is an open standard (backed by Anthropic)
- Already has ecosystem (Claude Desktop uses it)
- Users can reuse existing MCP servers
- Avoids NIH (Not Invented Here) syndrome
- Reduces maintenance burden

**Alternative considered:** Custom Rust plugin API (WASM plugins)
- More control, but more complexity
- Would fragment the ecosystem
- MCP is "good enough" and standard

---

### Why free forever vs. free trial?

**Decision:** Make desktop agent free forever (Tunnel mode), charge for cloud

**Rationale:**
- Obsidian proves this model works ($millions ARR)
- Reduces friction for students, hobbyists, solo founders
- Free users become advocates (word of mouth)
- Easier to upsell to cloud when they need scale
- Aligns with open-source values

**Risk:** Lower ARPU, more support burden
**Mitigation:** Self-service docs, community forum, upgrade prompts

---

## References & Inspiration

### Obsidian Resources
- [Obsidian Pricing](https://obsidian.md/pricing)
- [Obsidian Plugin Development](https://docs.obsidian.md/Plugins/Getting+started/Build+a+plugin)
- [Obsidian Forum](https://forum.obsidian.md/)

### Similar Local-First Tools
- **Logseq** - Open-source knowledge base (outliner format)
- **Notion offline-first** - Hybrid approach
- **Roam Research** - Graph-based notes (cloud-first, lost to Obsidian)

### MCP (Model Context Protocol)
- [MCP Specification](https://modelcontextprotocol.io/)
- [Claude MCP Servers](https://github.com/anthropics/mcp-servers)

### Design Inspiration
- [Raycast](https://raycast.com/) - Command palette UX
- [Linear](https://linear.app/) - Keyboard-first design
- [Superhuman](https://superhuman.com/) - Speed as a feature

---

## Implementation Priority Matrix

| Feature | Impact | Effort | Priority |
|---------|--------|--------|----------|
| Marketing refresh | High | Low | **P0** (do now) |
| Keyboard shortcuts | High | Low | **P0** (do now) |
| Command palette | High | Medium | **P1** (next sprint) |
| Local metadata cache | High | Medium | **P1** (next sprint) |
| Dataset graph | Medium | High | **P2** (next quarter) |
| MCP plugin system | High | High | **P2** (next quarter) |
| Pricing revision | High | Low | **P1** (next month) |
| Mobile apps | Medium | Very High | **P3** (6+ months) |
| Self-hosted backend | Medium | Medium | **P2** (next quarter) |

---

## Next Steps

### Week 1 (Immediate)
- [ ] Update README.md with local-first positioning
- [ ] Add keyboard shortcuts help overlay
- [ ] Update app tagline in UI

### Week 2-4 (Sprint)
- [ ] Implement command palette (Cmd+K)
- [ ] Start local metadata cache (DuckDB)
- [ ] Enable GitHub Discussions
- [ ] Draft public roadmap

### Month 2-3 (Next Quarter)
- [ ] Complete offline mode
- [ ] Launch MCP plugin beta
- [ ] Implement dataset relationship graph
- [ ] Review pricing model with stakeholders

---

**Status:** This document is a living plan. Update as priorities shift.

**Maintained by:** Product & Engineering
**Last updated:** April 14, 2026
**Next review:** June 1, 2026
