# Settings + History + Notifications + Privacy audit

The remaining sery-link surfaces. Power-user territory — users hit
these less often than search / folder detail, but bugs here are
still shippable.

Audited 2026-04-23. Scope:
- `src/components/Settings.tsx` + subcomponents (GeneralPanel,
  SyncPanel, AppPanel, AboutPanel) and PluginsPanel / MarketplacePanel
- `src/components/History.tsx`
- `src/components/Notifications.tsx`
- `src/components/Privacy.tsx`

**Severity**: 🔴 blocker · 🟡 friction · 🟢 polish

Verdict: no blockers; four friction items in Settings and Privacy;
History + Notifications + Plugins/Marketplace panels are clean.

---

## Settings findings

### 🟡 S1 — "Agent name" in General tab

**Where**: `Settings.tsx:301`

```tsx
<Field label="Agent name" hint="Shown in the cloud and on this device.">
```

**Problem**: Same terminology drift as everywhere else — this
device is a "machine" in v0.5.0 vocabulary. A user looking at
their sidebar-labeled "Machines" view and then hitting Settings
sees "Agent name" and has to connect two different words to one
concept.

**Fix**: `<Field label="Machine name" hint="Shown in the Machines view and on this device.">`.

---

### 🟡 S2 — SyncPanel hint phrasing exposes mechanism

**Where**: `Settings.tsx:428–429`

```
Sync cadence
How often the agent sends heartbeats to the cloud.
```

**Problem**: "The agent sends heartbeats to the cloud" leaks
implementation detail. Reads like a server ops console, not a
user-settings page. Also uses "agent" again.

**Fix**: User-level description of what the setting actually
affects:

```
Connection frequency
How often Sery Link checks in with Sery.ai. Lower values feel
more live; higher values reduce network traffic.
```

---

### 🟡 S3 — AboutPanel "API endpoint" Row shouldn't normally be exposed

**Where**: `Settings.tsx` AboutPanel, the `Row` with `draft.cloud.api_url`

**Problem**: Shows the backend endpoint URL (`https://api.sery.ai`
or whatever is configured) directly in the About tab. Useful for
debugging, not useful for users. Users who see it wonder if they
need to configure it.

**Fix**: Either hide it entirely or gate it behind an "Advanced"
accordion. The Tauri auto-updater + connection code use this URL
internally; users shouldn't have to think about it.

Low priority — cosmetic. Defer.

---

### 🟢 S4 — "Sync" tab name is a moderately strong term

**Where**: `Settings.tsx:222`

The second tab is named "Sync" and houses auto-sync toggles +
scan intervals. Fine for the tab name, but if the v0.5.0 pivot
continues away from "sync" terminology generally, consider
"Scan" or "Indexing" as the tab name. The settings it houses are
really about when/how often files get scanned locally, not about
syncing to the cloud.

**Fix**: Rename tab to "Indexing" and update the auto-sync toggle
label. Low priority.

---

## Privacy findings

### 🟡 P1 — Disclosure cards don't distinguish local vs connected state

**Where**: `Privacy.tsx:125–148`

The two disclosure cards list:

- **What goes to the cloud**: file paths, schemas, row counts/sizes,
  query results
- **What stays on this device**: raw file contents, files outside
  watched folders, OS credentials, excluded files

**Problem**: The "goes to the cloud" list only applies when the
user is **connected** (has a workspace key paired). In local-only
mode (which v0.5.0 pitches as the default), *nothing* goes to the
cloud. The card as-written implies these uploads always happen.

This matters for the brand promise: a user reading Privacy while
local-only sees "File paths… goes to the cloud" and worries their
paths are being exfiltrated right now when they aren't.

**Fix**: Gate the cards on `authenticated` state, or add a banner:

```tsx
{!authenticated && (
  <div className="mb-4 rounded-lg border border-emerald-200 bg-emerald-50 p-3 text-sm text-emerald-900 dark:border-emerald-900 dark:bg-emerald-950/40 dark:text-emerald-200">
    <Lock className="h-4 w-4 inline mr-1" />
    You're running Sery Link locally. Nothing below has been sent to
    the cloud because you haven't connected yet. The disclosure
    cards show what <em>would</em> cross the network if you did.
  </div>
)}
```

Makes the privacy story accurate in both states.

---

### 🟢 P2 — "Delete cloud metadata" has no confirmation of what's deleted

**Where**: `Privacy.tsx:57–71`

The confirm prompt says:

> Delete all metadata this device has uploaded to Sery? You can
> re-sync at any time.

Decent, but doesn't tell the user what specifically: dataset
schemas? Query history? Machine record? A short bullet list would
improve confidence.

**Fix** (nice-to-have): expand the modal copy. Low priority.

---

## Clean surfaces

- **History** (`History.tsx`, 515 lines) — local query history with
  JSONL persistence, filters, CSV export. Clean code, clean copy.
  No findings worth flagging.
- **Notifications** (`Notifications.tsx`, 225 lines) — schema-change
  notifications list with mark-read / clear actions. Clean.
- **PluginsPanel + MarketplacePanel** — reviewed briefly; no
  terminology drift, no broken links, no obvious copy issues.

---

## Ordered fix list

| Sev | Fix | Effort |
|---|---|---|
| 🟡 | S1 — "Agent name" → "Machine name" | 2 min |
| 🟡 | S2 — SyncPanel hint phrasing | 3 min |
| 🟡 | P1 — gate disclosure cards on authenticated state | 10 min |
| 🟢 | S3, S4, P2 — defer |

**Ship-now**: ~15 minutes.
