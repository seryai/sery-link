# Sery Link — Workflow & UX Walkthrough

**Status:** Shipped reality, not a proposal. Updated 2026-04-18.
**Differs from:** [UX_PROPOSAL.md](./UX_PROPOSAL.md) and
[FLUENT_UX_WORKFLOW_OPTIMIZATION.md](./FLUENT_UX_WORKFLOW_OPTIMIZATION.md)
— those are aspirational/strategy docs for *future* iterations.
This file shows what a user actually sees today.

This walks a user through Sery Link end-to-end: first run, the main
shell, every tab, the pair-a-machine flow, and the daily loop. It's
a reference for onboarding new team members, reviewing the product
holistically, and sanity-checking that the mental model matches
what ships.

---

## 1. First-run flow

```
┌────────────────────────────────────────────────────┐
│                                                    │
│                        🟣                          │    ← sery logo
│                                                    │
│               Welcome to Sery                      │
│                                                    │
│   Pick a folder to analyze. Your files never       │
│   leave this machine — only your questions and     │
│   the answers travel.                              │
│                                                    │
│  ┌─────────────────────────────────────────────┐   │
│  │  📁  Pick a folder                          │   │    primary CTA
│  └─────────────────────────────────────────────┘   │
│                                                    │
│  ┌─────────────────────────────────────────────┐   │
│  │  Skip for now — I'll add folders later      │   │    secondary
│  └─────────────────────────────────────────────┘   │
│                                                    │
│  I already have a Sery machine — join my fleet     │    text link
│                                                    │
│  🔒  No sign-up. No account. Nothing uploaded      │
│      until you ask.                                │
│                                                    │
└────────────────────────────────────────────────────┘
```

**Three branches:**

- **Pick a folder** → native picker → `bootstrap_workspace` silently
  creates anonymous workspace + agent → folder added → scan + tunnel
  start in background → lands on main shell.
- **Skip** → same bootstrap path, no folder yet. User adds one later
  from the Folders tab.
- **I already have a Sery machine** → opens the `JoinFleetForm` modal
  (below).

### JoinFleetForm

```
┌────────────────────────────────────────────────────┐
│  Join Existing Fleet                          [×]  │
│                                                    │
│  On the machine already running Sery, open         │
│  "Add another machine" and enter the code here.    │
│                                                    │
│  Pair code                                         │
│  ┌─────────────────────────────────────────────┐   │
│  │  XXX-XXX-XXX-XXX                            │   │    monospace
│  └─────────────────────────────────────────────┘   │
│  12 characters, hyphens optional                   │
│                                                    │
│  Name this machine                                 │
│  ┌─────────────────────────────────────────────┐   │
│  │  Home Desktop                               │   │
│  └─────────────────────────────────────────────┘   │
│                                                    │
│                     [Cancel]    [ Connect  → ]     │
└────────────────────────────────────────────────────┘
```

Friendly error mapping (see `src/components/JoinFleetForm.tsx`):
- 410 → "This code expired or is no longer valid."
- 409 → "This code has already been used."
- 400 → "The code looks wrong. Double-check the characters."
- 429 → "Too many attempts. Wait a minute and try again."
- network → "Can't reach Sery. Check your internet and retry."

After success → *"You're in the fleet. Now pick a folder on **this**
machine."* → same folder-pick UX → main shell.

---

## 2. Main shell

```
┌───────────────────────────────────────────────────────────────────────────────┐
│ 🟢 Online · 2 machines · 84 queries today                   ≡ Alerts  🔔 3   │   ← StatusBar
├──────────────┬────────────────────────────────────────────────────────────────┤
│              │                                                                │
│  🟣 Sery Link│   [ page content — full-width with a header band ]             │
│  Local-first │                                                                │
│              │                                                                │
│  📁 Folders  │                                                                │
│  ✨ Analytics│                                                                │
│  📊 Results  │                                                                │
│  💻 Fleet    │                                                                │
│  🔔 Notif. 3 │   ← unread badge                                               │
│              │                                                                │
│              │                                                                │
│              │                                                                │
│              │                                                                │
│  ⚙️  More ▾  │                                                                │
│              │                                                                │
└──────────────┴────────────────────────────────────────────────────────────────┘
```

- **Sidebar** — 5 primary tabs (Folders / Analytics / Results / Fleet /
  Notifications) + "More" dropdown (Settings, Privacy).
- **StatusBar** — live connection status, machine count, queries-today.
- **Tray icon** (menu bar / system tray) — always present, even when
  window closed:
  - Show Window
  - Add Another Machine…
  - Open Sery Web
  - Quit
- **⌘K command palette** — fuzzy search over nav + folders + datasets
  + actions.

All pages use one consistent shell: `flex h-full flex-col overflow-hidden`
with a `border-b bg-white px-6 py-4` header band containing a `text-2xl`
title + purple accent icon + subtitle, then a `flex-1 overflow-y-auto
p-6` scrollable content area. See `components/Analytics.tsx` as the
reference implementation.

---

## 3. Folders — the daily starting point

```
┌─ 📁 Watched folders ──────────────────────────────────────────────────────────┐
│    3 folders · 2,841 datasets · 412 MB              [ Show relationships ]   │
│                                                     [ + Watch Folder     ]   │
├───────────────────────────────────────────────────────────────────────────────┤
│                                                                               │
│  ┌──────────────────────────┐  ┌──────────────────────────┐                  │
│  │ 📂  Documents            │  │ 📂  Obsidian Vault       │                  │
│  │ ~/Documents              │  │ ~/vault                  │                  │
│  │                          │  │                          │                  │
│  │ 1,204 files · 340 MB     │  │ 412 notes · 8 MB         │                  │
│  │ Last scan: 2 min ago     │  │ Last scan: 14 h ago      │                  │
│  │                          │  │                          │                  │
│  │ [ Rescan ] [ ⋯  ]        │  │ [ Rescan ] [ ⋯  ]        │                  │
│  └──────────────────────────┘  └──────────────────────────┘                  │
│                                                                               │
│  ┌──────────────────────────┐                                                │
│  │ 📂 Taxes 2024            │  ← scan in progress                            │
│  │ ~/Documents/Taxes 2024   │                                                │
│  │                          │                                                │
│  │ [█████████░░] 72%        │     live progress bar                          │
│  │ Reading W-2.pdf…         │                                                │
│  └──────────────────────────┘                                                │
│                                                                               │
└───────────────────────────────────────────────────────────────────────────────┘
```

**Actions per folder:** Rescan (fire-and-forget), `⋯` menu (Analytics
for this folder / Remove / Exclude patterns). Relationship-graph modal
visualizes detected foreign-key links across datasets. Live progress
comes from `scan_progress` events in `useAgentStore.scansInFlight`.

---

## 4. Fleet — multi-machine awareness

```
┌─ 💻 Your Fleet ────────────────────────────────────────────────────────────────┐
│    Every Sery machine connected to this workspace.  [+ Add another machine]  │
├───────────────────────────────────────────────────────────────────────────────┤
│  ● MacBook Pro                         [ This machine ]   1,204 files  340MB │
│    macOS 14 · johns-mbp                                                      │
│                                                                               │
│  ● Home Desktop                                          847 files  280 MB   │
│    Linux 22.04 · pop-desktop            🔔 2               (click → notif.)  │
│                                                                               │
│  ○ Office Laptop                                         412 files  65 MB    │
│    macOS 14 · office-mbp                                                      │
│    (last seen 2 h ago)                                                        │
└───────────────────────────────────────────────────────────────────────────────┘
```

- **Green dot** = online, **grey** = offline, **red** = error.
- **Per-machine unread badge** — click jumps to `/notifications`.
  Counts are derived client-side from the notifications store
  filtered by `origin_agent_id`.
- **"+ Add another machine"** opens the QR + pair-code modal (§7).
- Polls `list_fleet` every 15 s so online/offline transitions
  surface without a manual refresh.

---

## 5. Notifications — the self-healing signal

```
┌─ 🔔 Schema changes ─────────────────────────────────────────────────────────┐
│    12 total · 2 unread                       [ Mark all read ] [ Clear ]   │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  ● orders-q3.csv                                              3 min ago   │    ← unread
│    taxes/2024/orders-q3.csv                                               │
│    1 added · 1 type changed                                               │
│    ─────────────────────────────────────                                  │
│       ╋  currency  (VARCHAR)                                              │
│       →  amount    INTEGER → VARCHAR                                      │
│                                                                           │
│  ○ prices.csv (from Home Desktop)                             12 h ago    │
│    data/prices.csv                                                        │
│    2 added                                                                │
│       ╋  discount  (DOUBLE)                                               │
│       ╋  valid_from (DATE)                                                │
│                                                                           │
│  ○ ...                                                                    │
└───────────────────────────────────────────────────────────────────────────┘
```

**Sources:**
- Local scans detect drift via `metadata_cache::compute_schema_diff`.
- Cross-machine events arrive over the WebSocket tunnel as
  `schema_change` messages; `websocket::handle_remote_schema_change`
  threads them through with `(from <origin>)` in the display name.

**Storage:** append-only JSONL at `~/.seryai/schema_notifications.jsonl`,
500-entry cap with lazy rotation, atomic writes. Mutations (mark-read,
clear) go through Tauri commands that modify the file.

**Dedup:** same `(workspace_id, dataset_path, diff)` within 60 s
refreshes the existing record instead of spamming new ones.

**Toast:** transient toast on each new event. User can silence via
Settings → App → **"Schema-change toasts"**.

**Backend interaction:** this is the same stream that invalidates
cached query plans on the cloud side via
`mark_plans_stale_for_dataset`. Feature and loop are fully connected.

---

## 6. Results (query history)

Reached from the sidebar. Reflects queries run *from this machine*
via tunnel; cloud-native queries live on the web dashboard.

```
┌─ 📊 Query Results ─────────────────────────────────────────────────────────┐
│    All queries executed on your local data            [ Stats ] [ Export ]│
├───────────────────────────────────────────────────────────────────────────┤
│  [ All 124 ]  [ Success 118 ]  [ Errors 6 ]     🔍 Search…                │
├───────────────────────────────────────────────────────────────────────────┤
│  ✓  2 min ago · 84 ms · 1,204 rows                                        │
│    SELECT sale_date, SUM(amount) FROM sales-2025.csv GROUP BY sale_date   │
│    Expand ▾                                                               │
│                                                                           │
│  ✓  14 min ago · 12 ms · cache hit · 42 rows                              │
│    SELECT * FROM read_csv_auto('~/prices.csv') WHERE unit_price > 50      │
│                                                                           │
│  ✗  1 h ago · error                                                       │
│    SELECT amount::INTEGER FROM sales-2025.csv                             │
│    "Conversion failed: VARCHAR to INTEGER on row 847"                     │
└───────────────────────────────────────────────────────────────────────────┘
```

Filters (All / Success / Errors), full-text search over file path + SQL +
error, export to JSON / CSV, cache-hit indicator. Stats panel (off by
default) shows rollups: total query count, average execution time,
cache-hit rate.

---

## 7. Pair a second machine

Initiated from machine #1 via tray → Add Another Machine… OR Fleet →
**+ Add another machine**:

```
┌────────────────────────────────────────────────────┐
│  Add a machine to your fleet                  [×]  │
│                                                    │
│         ┌─────────────────────────┐                │
│         │                         │                │
│         │     ▓▓▓▓▓▓▓▓▓▓▓▓▓       │                │
│         │     ▓ ░░░ ▓ ░ ▓ ▓       │                │    QR code
│         │     ▓ ░ ░ ▓ ░ ▓ ▓       │                │
│         │     ▓▓▓▓▓▓▓▓▓▓▓▓▓       │                │
│         │                         │                │
│         └─────────────────────────┘                │
│                                                    │
│                 ABCD-1234-EFGH                     │    code below QR
│                     📋 Copy                        │
│                                                    │
│   Expires in 4:32                                  │
│                                                    │
│   On the other machine:                            │
│   1. Install Sery Link                             │
│   2. Click "I already have a Sery machine"         │
│   3. Scan the QR or paste the code                 │
│                                                    │
└────────────────────────────────────────────────────┘
```

**Mechanics:**
- Polls `pair_status` every 2 s.
- On redemption from the second machine, modal swaps to a success
  state ("Home Desktop joined your fleet") and auto-closes.
- Both machines refresh their Fleet view — the new machine appears
  within 15 s (the default fleet poll interval).

---

## 8. Settings

Tabbed: **General / Sync / App / Plugins / Marketplace / About**.

Most users never touch it. Common edits when they do:

| Tab | Typical edits |
|---|---|
| General | Theme (light / dark / system) |
| Sync | Exclude patterns, max file size, sync interval |
| App | Launch at login, notifications, auto-update, schema-change toasts |
| Plugins | Enable / disable / uninstall loaded plugins |
| Marketplace | Browse + install community plugins |
| About | Agent ID, workspace ID, logout, clear cloud metadata, export / import config |

---

## 9. Privacy

The "receipts" tab. Reads from `~/.seryai/sync_audit.jsonl`.

```
┌─ 🛡️ Privacy & Activity ─────────────────────────────────────────── [Refresh]─┐
│    Full transparency into what this device has shared with Sery.             │
├──────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│  ┌─── ☁️ What goes to the cloud ─────┐ ┌─── 🔒 What stays on this device ──┐│
│  │ ✓ File paths (relative)          │ │ ✓ Raw file contents              │ │
│  │ ✓ Schemas — names and types      │ │ ✓ Files outside watched folders  │ │
│  │ ✓ Row counts and file sizes      │ │ ✓ Your OS credentials            │ │
│  │ ✓ Results of queries you run     │ │ ✓ Files matching exclude patterns│ │
│  └──────────────────────────────────┘ └──────────────────────────────────┘ │
│                                                                              │
│  Syncs 47  ·  Datasets 2,841  ·  Columns 18,402  ·  Failed 0                │
│                                                                              │
│  [ Export diagnostic bundle ]  [ Clear local audit log ]  [ Delete cloud … ]│
│                                                                              │
│  👁 Sync activity (newest first)                                            │
│  ├─ ✓ 2 min ago · Documents · 12 datasets · 340 MB                          │
│  ├─ ✓ 14 h ago  · Vault    · 0 datasets · 0 MB  (no changes)                │
│  └─ ✗ 2 d ago   · Taxes 2024 · network timeout                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

Every metadata post is logged. The "Delete cloud metadata" button is
the nuclear option — wipes everything the backend has about this
workspace's datasets (agent + workspace remain; files stay local as
always).

---

## 10. The daily loop, user's perspective

```
Morning:
  ┌─ Laptop wakes → tray icon lights green
  ├─ Watcher sees 3 new files in ~/Documents/Taxes 2025
  ├─ Background rescan (no user interaction)
  ├─ Schema of taxes-q1.csv changed since last quarter
  │   → toast: "Schema changed: taxes-q1.csv (1 added)"
  │   → Notifications badge → 1
  │
  │  [user switches apps, forgets about it]
  │
  ▼
Later, back at dashboard (web):
  ┌─ Types "revenue by product last quarter" in chat
  ├─ Agent: search_similar_past_sessions → hits a 👍-rated plan from last month
  ├─ Agent: re-verifies schema (catches the new column, still works)
  ├─ Answer back in < 2s with Blue Mug, Desk Lamp chart
  ├─ User clicks 👍 → locks in this plan for next time
  │
  ▼
That evening:
  ┌─ User installs Sery on their office laptop
  ├─ "I already have a Sery machine" → enters pair code from tray
  ├─ Picks Work folder → scanner starts
  └─ Fleet view now shows both machines
```

---

## 11. UX design principles in force

1. **One decision on first run.** Pick a folder, that's it.
   Everything else happens in the background or lives in Settings for
   the 5% who care.
2. **Full-width pages with a consistent shell.** Every page has the
   same header band + scroll pane. Learned from Analytics; applied
   uniformly in commit `acde604`.
3. **No SQL ever shown by default.** Results tab can surface it in
   a disclosure, but the primary surface is the answer.
4. **The tray is the permanent entry point.** Even with the window
   closed, users can add a machine, show status, or quit. Critical
   for a "quiet background app" mental model.
5. **Receipts over promises.** Privacy tab, audit log, toggle-able
   toasts — users can verify, not just be told.
6. **Self-healing invisible unless asked.** Plans go stale silently;
   schema changes show a toast; the user doesn't see the agent's
   retry dance.

---

## Where to find the code

| Thing | File |
|---|---|
| Onboarding wizard | `src/components/OnboardingWizard.tsx` |
| Join-fleet form | `src/components/JoinFleetForm.tsx` |
| Main app shell | `src/App.tsx` |
| Sidebar + route wiring | `src/App.tsx` |
| Folders page | `src/components/FolderList.tsx` |
| Analytics page | `src/components/Analytics.tsx` |
| Results page | `src/components/History.tsx` |
| Fleet page | `src/components/FleetView.tsx` |
| Notifications page | `src/components/Notifications.tsx` |
| Settings page | `src/components/Settings.tsx` |
| Privacy page | `src/components/Privacy.tsx` |
| Pair modal | `src/components/AddMachineModal.tsx` |
| Command palette | `src/components/CommandPalette.tsx` |
| Status bar | `src/components/StatusBar.tsx` |
| Event wiring | `src/hooks/useAgentEvents.ts` |
| Tauri bridge | `src-tauri/src/commands.rs` |
| Tray | `src-tauri/src/tray.rs` |
| WebSocket tunnel | `src-tauri/src/websocket.rs` |
| Scanner | `src-tauri/src/scanner.rs` |
| Schema-change persistence | `src-tauri/src/schema_notifications.rs` |
