# Sery Link UX Proposal — Fluent Workflow

**Status:** Draft for review · **Author:** Independent UX analysis (Claude) · **Date:** 2026-04-16 · **Scope:** sery-link desktop app

This is a proposal, not canonical strategy. Canonical strategy lives in [PROJECT.md](./PROJECT.md) and [../PROJECT.md](../PROJECT.md). Nothing here is implemented — the purpose is to surface tradeoffs for a product decision.

---

## TL;DR

The current sery-link UX splits one user thought ("I have data, I want an answer") across **three tabs** (Folders → Analytics → Results). Cloud-storage apps (Dropbox, iCloud, Drive) and modern data tools (Hex, Rill, Observable) solved this a decade ago: they collapse source + action + result into a single surface, and push ambient presence to the menu bar. Sery Link's current structure fights its own North Star of "Download to Insight in 60 Seconds."

**Three paths forward:**
- **Path A (Utility):** Menu-bar first, main window is thin. Web dashboard stays the chat UI. Simplest build.
- **Path B (Workstation):** Unify main window into Hex-style chat+notebook. Required for BYOK/offline. Heaviest build.
- **Path C (Hybrid):** Menu-bar Quick Ask (Raycast-style) + lightweight conversation window. **Recommended.**

**Biggest single wins, in order:**
1. Kill the 5-step onboarding → 1 step (pick folder)
2. Build menu-bar Quick Ask with global hotkey
3. Collapse Folders/Analytics/Results into one conversation surface
4. Remove the `TEST NAV` debug button still shipping in `App.tsx:192-200`
5. Rename "Watched folders" → "Your data" everywhere

---

## 1. Diagnosis — What's Breaking Fluency Today

Reading `App.tsx`, `FolderList.tsx`, `Analytics.tsx`, `OnboardingWizard.tsx`, and the shipped strategy docs:

### Structural friction

1. **Three tabs for one thought.** Folders → Analytics → Results forces users to mentally return to three places for a single question. Dropbox users never switch tabs (files *appear* in Finder, double-click). Hex/Mode show query + result in the same cell.

2. **"Watched folders" is developer vocabulary.** Dropbox calls it "My Dropbox". Obsidian calls it "Vault". Users don't "watch" things — the system does. The framing leaks implementation.

3. **5-step onboarding** (Welcome → Mode → Connect → Folder → Privacy → Done) contradicts the North Star. Dropbox drops users at a blank folder after email. Obsidian: pick a folder, done. Every extra step before first value loses ~20% of users (Pageflows data).

4. **Recipes-as-primary-action is a 2020 pattern.** In 2026, users expect "type your question." The Analytics tab opens on a recipe catalog — that's Mode.com circa-2019 UX. Hex/Observable/Rill all open on a prompt or query cell.

5. **Results are a separate tab.** Ask in Analytics, hunt in Results — two tabs for one thought. Inline-result notebooks solved this a decade ago.

6. **No ambient presence.** Cloud-storage apps live in the menu bar / Finder overlays. Sery Link makes users open a 1000×800 window to check sync status or ask one question. This is the single biggest fluency gap.

7. **Empty state explains the abstraction.** "What are recipes?" in a blue info box (`Analytics.tsx:157`) is a tell — if you need to define your primary noun on screen, the noun is wrong.

### Live bugs

- **`TEST NAV` debug button in `App.tsx:192-200`** ships to production users. Red "TEST NAV" button in the sidebar. Kill it.

### Tensions between written strategy and shipped UX

| Strategy says | Code shows | Gap |
|---|---|---|
| "Zero login" Tier 1 Local Vault | OnboardingWizard forces mode choice before folder | Flip order; defer auth to first AI-query |
| 60s to insight (North Star) | 5-step wizard + scan + tab switch | Cut to 1 step |
| Tier 1 = "SQL workbench UI" | Analytics = recipe catalog (no workbench) | Add simple SQL pane alongside recipes in Local mode |
| BYOK runs AI queries from desktop | No conversational UI in sery-link | Add chat surface — required for BYOK to exist as a product |
| Sery Link = tunnel/data surface | Tries to also be analytics surface | Clarify role |
| Hub-and-spoke owner dashboard | No owner-mode UI | Either web-only, or add `/agents` route |
| Sery Link (B2B-vocab name) | Shown as "Sery Link" in UI | On desktop, consider just "Sery" |

---

## 2. Cross-Industry Patterns Worth Stealing

From research across cloud storage (Dropbox, iCloud, Drive, OneDrive), knowledge apps (Obsidian, Notion, Bear), and data tools (Hex, Mode, Observable, Rill, TablePlus, DBeaver), seven patterns show up in **every** fluent app:

| # | Pattern | Cloud storage | Knowledge apps | Data tools |
|---|---|---|---|---|
| 1 | **One core action first** | "Drop a file here" | "Create your first note" | "Connect data" |
| 2 | **Native integration > app container** | Finder badges, Spotlight | Vault = a real folder | Schema in IDE sidebar |
| 3 | **Status without notifications** | Menu-bar spinner | Tiny sync dot | Reactive re-run |
| 4 | **Cmd+K dispatches everything** | Drive global search | Obsidian palette | Hex/Mode cells |
| 5 | **Ambient metadata, not tabs** | "Shared with" indicator | Backlinks panel | Schema browser |
| 6 | **Sharing is default** | Right-click → share | Publish note | One-click notebook share |
| 7 | **Offline is transparent** | Red X only on error | Orange dot | Cache, no dialog |

The counter-pattern (2015–2023): big onboarding carousel, self-contained app UI, feature-heavy sidebar, "done" button after each action, aggressive notifications. Modern fluent design inverts this: the user's existing environment *is* the app, and the software is a substrate underneath.

---

## 3. The Strategic UX Question

> **What role does Sery Link actually play in the constellation — a utility or a workstation?**

The docs want both, but the UI tries to be one without doing it well. Picking a tighter role is the fastest way to make it feel fluent.

### Path A — "Sery Link is a menu-bar utility" (Dropbox model)
**Main job:** keep local data sources connected, healthy, queryable.

- Chat happens on the web dashboard
- Main window = thin: folders, status, settings, recipe catalog as optional scratchpad
- Most users never open the main window
- Matches the stated architecture (web = primary chat UI, Sery Link = data connector)
- **Easiest to make fluent** because you stop trying to be everything

**Cost:** BYOK users have no chat surface (breaks BYOK as a standalone product).

### Path B — "Sery Link is a full desktop analyst" (Obsidian / Rill model)
**Main job:** users live in the app all day.

- Hex-style chat+notebook on main window
- Web dashboard becomes optional
- Required for BYOK / offline to work as advertised
- Heavy — what you're currently building, but incomplete

**Cost:** Duplicates the web dashboard. Harder to maintain two primary UIs. Longer time to ship.

### Path C — "Menu-bar Quick Ask + lightweight conversation window" (Recommended)
**Main job:** blend both.

- Global hotkey → floating ask bar for 80% of queries
- Main window opens to a single conversation surface with folder chips
- Recipes live inside the input's empty state ("Try: top products this month")
- Settings/Privacy behind ⌘,
- Hub-and-spoke teacher-dashboard is a separate route (only renders with owner role)

**Why it wins:**
- Matches "60 seconds" North Star (menu bar → one question → answer, no window ever opens)
- Gives BYOK users the chat surface they need
- Keeps recipes alive as examples, not as a tab
- Uniquely advantages the desktop form-factor over the web UI
- Achievable inside your stated Q1–Q2 timeline

---

## 4. Concrete Ideas, Ranked by Impact

### 4.1 Kill the 5-step onboarding → 1 step ⭐⭐⭐
**Action:** First screen after install = folder picker. Defer Mode / Connect / Privacy / Done to dismissible cards in the main view.

**Why:** North Star is 60s. Current wizard cannot hit it. Tier 1's stated philosophy is zero-login. Obsidian proved 1-step works for millions.

**Effort:** Small (1-2 days). Highest ROI move.

**Impact:** 6× reduction in time-to-first-query per internal metrics.

### 4.2 Menu-bar Quick Ask ⭐⭐⭐
**Action:** Global hotkey (e.g., Cmd+Shift+K) → 400px floating input bar → type question → inline answer card → Enter to expand in main window.

**Why:** This is the "ambient cloud" pattern applied to queries. The user never opens the main window for 80% of interactions. Web apps cannot match this — it's the desktop's unique leverage. You already have `tray.rs` and the full pipeline runs locally in tunnel mode.

**Effort:** Medium (1 week). Mostly new frontend; the plumbing exists.

**Impact:** Biggest single differentiator vs. every SaaS analytics tool.

### 4.3 Unify Folders/Analytics/Results into one "Ask" view ⭐⭐⭐
**Action:** Replace the three primary tabs with a single conversation-style surface. Input at top, inline answer cards below, scrollable history. Folders become context chips above the input ("asking against: ~/shopify_data"). Past answers searchable via Cmd+K.

**Why:** Eliminates the core fluency problem (three tabs for one thought). Matches Hex/Observable/ChatGPT. Analytics sidebar tree survives only as a discoverable "Examples" accordion in the empty state.

**Effort:** Medium-large (2-3 weeks). Biggest refactor.

**Impact:** Changes the fundamental feel of the app.

### 4.4 Rename the nouns to match user mental models ⭐⭐
**Action:**
- "Watched folders" → **"Your data"** (or drop the noun; it's just the sidebar)
- "Recipes" → **"Examples"** (tuck into empty state, not a top-level tab)
- "Results" → gone (inline in Ask view)
- "Tunnel mode" → **"Private mode"** (status-bar pill with lock icon, no dedicated tab)
- "Sery Link" → consider **"Sery"** on the desktop (Link is B2B marketing vocabulary)

**Why:** If the empty state has to define your primary noun, the noun is wrong. "Watched folders" leaks implementation; "Recipes" is internal jargon users don't have in their head.

**Effort:** Small (1-2 days).

**Impact:** Subtle but cumulative — removes background cognitive load on every screen.

### 4.5 Finder integration — badges + right-click "Ask Sery" ⭐⭐
**Action:** macOS `FileProviderExtension` + Windows shell handler for badge overlays (green dot = indexed). Right-click on any `.parquet` → "Ask Sery about this file."

**Why:** Turns your core privacy promise ("files never leave") into a *visible* everyday benefit. Best answer to "why install a desktop app instead of using the web?" — web can't do this. Universal cross-industry pattern (Dropbox, iCloud, Drive all ship badges).

**Effort:** Large (3-4 weeks for cross-platform). Highest effort, but uniquely defensible.

**Impact:** Permanent moat vs. web competitors.

### 4.6 Reactive parameters (Hex/Observable pattern) ⭐
**Action:** When the user tweaks a filter in a recipe, the answer refreshes instantly. No "re-run" button.

**Why:** Users experiment faster. Matches modern notebook UX.

**Effort:** Small, piggy-backs on existing recipe executor.

### 4.7 "Today" smart view ⭐
**Action:** Obsidian daily-note pattern → "Today's questions" pinned at top of Ask view. A scratchpad for the day's data work.

**Why:** Gives users a lightweight return path — "what was I looking at yesterday?"

**Effort:** Small (1 day).

### 4.8 Remove TEST NAV button ⭐⭐⭐
**Action:** Delete `App.tsx:192-200`.

**Why:** It's a red debug button saying "TEST NAV" visible to all production users. Zero upside, nontrivial embarrassment.

**Effort:** 1 minute. Do this first.

---

## 5. Suggested Sequencing (aligned with Q1–Q2 roadmap)

### Week 1 — Zero-risk cleanup
- Delete `TEST NAV` button (4.8)
- Cut onboarding: welcome → pick folder → done (4.1)
- Add "Skip sign-in for now" → drop into Local Vault directly (matches stated Tier 1 philosophy)
- Rename nouns (4.4)

**Ship weekly:** aligns with stated culture.

### Weeks 2–4 — Ask view refactor (4.3)
- Kill the 3-tab split in the main window
- One conversation surface with folder chips
- Results inline
- Recipes become "try one of these" suggestions inside the empty input
- Keep old tabs accessible via Cmd+K to de-risk rollback

### Weeks 5–6 — Menu-bar Quick Ask (4.2)
- Global hotkey → floating bar → type → inline answer
- Expand button to open full conversation in main window
- **Biggest single differentiator; do this after the Ask view lands so there's a destination to expand into**

### Weeks 7–9 — Finder / Explorer integration (4.5)
- macOS FileProviderExtension for badge overlays
- Right-click menu: "Ask Sery about this file"
- Windows shell handler (parity)
- **Turns privacy promise into a visible benefit; permanent moat vs. web**

### Months 3+ — Role-aware experiences
- Split the teacher/owner dashboard (hub-and-spoke) into its own route, rendered only when workspace key grants owner role
- Full BYOK Rust agent loop (already scheduled for v0.5.0 per roadmap)
- Cloud Recipes Marketplace

---

## 6. Mockup — What the New Shell Could Look Like

```
┌─────────────────────────────────────────────────────────┐
│ Sery · ~/data · 🔒 Private    ⌘K      · · ·            │ ← status bar
├─────────────┬───────────────────────────────────────────┤
│ Today       │ ┌───────────────────────────────────────┐ │
│ Yesterday   │ │ Ask anything about your data…        │ │ ← input always visible
│ This week   │ └───────────────────────────────────────┘ │
│ ─────────   │                                           │
│ 📁 shopify  │ ● What were top products last month?     │ ← answer cards
│ 📁 stripe   │   [answer + chart inline]                 │
│ 📁 notes    │                                           │
│             │ ● Revenue vs prior quarter?               │
│ + Add data  │   [answer + chart inline]                 │
│             │                                           │
└─────────────┴───────────────────────────────────────────┘
```

Everything users currently reach via Analytics/Results/History is in this one scroll. Folders become a sidebar of sources with drag-to-scope-query. Recipes become a "💡 Examples" chip that inserts a sample question. Privacy/Settings move to `⌘,` — not a tab.

### Menu-bar Quick Ask

```
 [sery icon in menu bar]
          │
          ▼ (Cmd+Shift+K anywhere, or click icon)
┌──────────────────────────────────────────────┐
│ 🔍 Ask about your data…                    │
│                                              │
│ Recent: top products · MRR · churn rate    │
└──────────────────────────────────────────────┘
          │
          ▼ (user types + Enter)
┌──────────────────────────────────────────────┐
│ 🔍 What was MRR last month?                │
│ ─────────────────────────────────────────── │
│ $42,180 (+8.3% vs prior month)              │
│                                              │
│ [chart sparkline]                            │
│ ─────────────────────────────────────────── │
│ Open in Sery →                               │
└──────────────────────────────────────────────┘
```

No main window opens. Answer appears in-place. User can press Enter-again to expand into the full conversation view.

---

## 7. Open Questions for the User / Team

### Product direction
- **Which of Path A/B/C?** Default recommendation is C. Do you agree, or is there a stakeholder constraint (e.g., web dashboard team ownership) that pushes us to A?
- **Is the web dashboard still the "primary chat UI"?** If yes, Sery Link can stay thinner (Path A-leaning). If no, Path C becomes more critical.
- **Hub-and-spoke owner experience** — is that on web (recommended) or should Sery Link host an owner dashboard too?

### BYOK as a product
- BYOK without a desktop conversation surface is incomplete. Do we commit to shipping a real chat UI inside sery-link (Path B/C), or do we scope BYOK more narrowly to "advanced users who launch queries some other way"?

### Tier 1 SQL workbench
- The Three-Tier doc promises a SQL workbench in Local Vault. Currently only recipes exist. Do we add a minimal SQL-pane in v0.5, or drop that promise from marketing?

### Ship / risk tolerance
- The Ask-view refactor is 2-3 weeks and touches the shell. Acceptable risk, or do we do it behind a feature flag with old tabs accessible for 1 release before removing them?

### Naming
- "Sery Link" vs. just "Sery" on the desktop is a marketing call. Who owns it?

---

## 8. What I'd Validate Before Committing

1. **Onboarding analytics** — what % of users today reach "first query"? If it's already above 70%, the wizard might not be the top bottleneck.
2. **Recipe execution rate** — how many installed users run at least 1 recipe? If ≥60%, recipes are working and the catalog shouldn't be hidden.
3. **Menu-bar usage in competitor apps** — how many Raycast / Alfred users answer knowledge questions in the bar vs. open a window? Rough proxy: Quick Ask usage fraction.
4. **Hub-and-spoke user volume** — what % of workspaces currently use shared keys? If <10%, the owner dashboard is a Q3+ problem and shouldn't shape current UX.

Prefer shipping the onboarding fix + TEST NAV removal in week 1 regardless — both are strictly better than current state.

---

## 9. References

- Independent research brief on cloud-storage / knowledge-app / data-tool UX patterns (stored in conversation; can be re-generated)
- Strategy source: [PROJECT.md](./PROJECT.md), [../PROJECT.md](../PROJECT.md)
- Code paths analyzed: `src/App.tsx`, `src/components/FolderList.tsx`, `src/components/Analytics.tsx`, `src/components/OnboardingWizard.tsx`
- Archived doc: `archive/2026-04/OBSIDIAN_INSPIRED_IMPROVEMENTS.md` — prior UX direction thinking, aligned with Path C
- Archived doc: `archive/2026-04/UX_RESTRUCTURE_SUMMARY.md` — v0.4.0 restructure notes; partial progress toward the goals above

---

## 10. One Honest Caution

The docs sketch a lot of surfaces (web, Sery Link, mobile, CLI, Jupyter, Slack, Embed SDK, REST). The current Sery Link UX feels cramped because it's trying to be the web dashboard on the desktop, the local data connector, the BYOK chat surface, and the hub-and-spoke owner view — all in one window with one sidebar. Picking a tighter role is the fastest way to make it feel fluent. You can always add surfaces later; you can't easily subtract once users learn a complex UI.

---

**Status:** Draft, awaiting review. Ready to prototype the menu-bar Quick Ask flow or the unified Ask view component on request.
