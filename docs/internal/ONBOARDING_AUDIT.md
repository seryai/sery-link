# Onboarding friction audit

Reading the v0.5.0 first-run path as if I were a new user. Each
finding names the file, the specific friction, and a concrete fix.

**Scope**: I'm reading code, not running the app. I can spot logic
issues, missing states, and confusing copy; I can't spot whether the
animation feels smooth, whether buttons have enough contrast, or
whether the native folder picker opens at a sensible location on each
OS. Validate with real beta users.

**Severity legend**:
- 🔴 Blocker — user likely fails or churns
- 🟡 Friction — user succeeds but with an unnecessary beat of
  confusion
- 🟢 Polish — nice-to-have

---

## The happy path I traced

1. First launch → `App.tsx:191` detects `!first_run_completed` →
   renders `OnboardingWizard`.
2. User clicks "Pick a folder" → native dialog → `add_watched_folder`
   + fire-and-forget `rescan_folder` + `start_file_watcher` +
   `complete_first_run`.
3. Wizard closes, `App.tsx` re-renders the main UI.
4. `/` redirects to `/search` → `SearchPage` renders `EmptyPrompt`.

Total time from launch to searchable state: ~3 seconds of user
interaction + variable async indexing.

---

## Findings

### 🔴 F1 — Skipping folder selection leaves the user stranded

**Where**: `OnboardingWizard.tsx:152–157` ("Skip for now") +
`SearchPage.tsx` (empty state has no CTA to add a folder).

**Problem**: A user who clicks "Skip for now" lands on the Find page
with `EmptyPrompt` showing three hint cards about column names and
filenames — but they have *no indexed files yet*. Typing anything
shows "No matches." The hint "visit a folder once to add it to the
search index" (NoResults, line 238) is obscure — they don't have a
folder. The only way forward is for them to notice the sidebar Folders
tab, navigate there, and spot the add-folder button. That's two
wayfinding steps for a user who just said "I don't want to pick a
folder right now."

**Fix**: Either
- (a) `EmptyPrompt` shows a visible "Add a folder" button when
  `config.watched_folders.length === 0`, OR
- (b) the Find page detects the zero-folder state and shows a
  dedicated "You haven't added any folders yet — [Add a folder]"
  empty state instead of `EmptyPrompt`.

I'd do (b). It's clearer and doesn't conflate "I have folders but
haven't typed a query" with "I have no folders at all."

---

### 🔴 F2 — "Ready to go" toast lies when the scan hasn't finished

**Where**: `OnboardingWizard.tsx:94–98` and `FolderList.tsx:53`.

**Problem**: After `add_watched_folder`, the wizard kicks off
`rescan_folder` *fire-and-forget* and immediately toasts "Ready to go.
Sery is watching your folder locally." Then `complete_first_run`
closes the wizard. The user lands on the search page and types a
column name, but the scan for a 50,000-file folder is still running —
they see zero results. From their perspective, "Ready" was a lie and
the search is broken.

**Fix**: The search page (and ideally a status strip visible
everywhere) should show indexing progress when `scansInFlight` is
non-empty. Two levels:

1. **Minimum**: a small "Indexing your folder… 2,341 files so far"
   pill in the SearchPage header while any scan is active. Keeps the
   user from thinking the empty results are a bug.
2. **Better**: an inline result-area banner on NoResults that reads
   "Sery is still indexing. Results will appear as files are found."

Already there is a `scansInFlight` store field populated by
`scan_progress` events — just needs to be surfaced on SearchPage and
in the NoResults component.

---

### 🟡 F3 — Wizard doesn't tell the user what Sery is

**Where**: `OnboardingWizard.tsx:136–142`.

**Problem**: Current copy:
> Welcome to Sery
> Pick a folder to index on this machine. Sery works fully offline
> until you decide to connect to the cloud.

This explains *what to do*, not *why*. A user arriving from a HN link
or a friend's recommendation without reading the website first sees
"pick a folder" and has no frame for what value they'll get. A user
arriving from the website already knows — but most users will have
read a tweet, not a landing page.

**Fix**: Add one line between the headline and the instruction. Not a
paragraph — one sentence that names the payoff. Examples:

- "Search across every CSV, spreadsheet, and document on your machine
  by filename *or column name*. No uploads, no account."
- "Sery reads the columns inside your spreadsheets so you can search
  across them the way you search across filenames."

The existing "No sign-up. No account. 100% local until you say
otherwise." line (OnboardingWizard.tsx:159–162) is great for trust
but doesn't name the job. Put the job above the buttons, keep the
trust line below.

---

### 🟡 F4 — "Pick a folder" has no suggested default

**Where**: `OnboardingWizard.tsx:105–116`, `pickFolder()`.

**Problem**: `openDialog({ directory: true, multiple: false })` opens
at the user's home directory on macOS, varies on other platforms. The
user has to think: "what folder?" For a first-time user unfamiliar
with the product, choosing wrong (e.g., picking `~/Downloads` with 8
files) leaves the product looking empty-ish.

**Fix**: Two options:

1. **Suggest a default**: pass `defaultPath: os.homedir() + '/Documents'`
   (Tauri's `@tauri-apps/plugin-dialog` supports `defaultPath`).
   On macOS most users have years of Documents content; on Windows
   it's `%USERPROFILE%\Documents`; on Linux it's `~/Documents`.
   Better first-run experience because the picker lands in a
   content-rich place.
2. **Add a one-line hint below the button**: "Tip: `~/Documents`,
   `~/Downloads`, or wherever you keep your spreadsheets."

I'd do (1) — reduces a decision.

---

### 🟡 F5 — Error state "folder doesn't exist" is unreachable

**Where**: `OnboardingWizard.tsx:252`, `friendlyError()`.

**Problem**: The native folder picker can only return existing
folders. The "That folder doesn't exist" branch can't fire via the
picker — only via stale `selectedFolder` on retry after the folder
was deleted. Unlikely but possible.

**Fix**: Minor. Current code is fine; the branch is defensive. Maybe
log-and-move-on instead of a visible error message for this edge.

---

### 🟡 F6 — `start_file_watcher` is fire-and-forget with no user feedback on failure

**Where**: `OnboardingWizard.tsx:78–80`.

**Problem**: If the file watcher fails (e.g., on macOS without
full-disk access), the wizard completes successfully but future file
changes in the watched folder won't be detected. The user will
eventually search for a new file that they know was added, get no
hits, think the product is broken.

**Fix**: Two parts:

1. Make `start_file_watcher` await-able in the wizard and surface
   failures in the friendlyError text (hint the user toward granting
   full-disk access in System Settings).
2. Add a visible "File watcher inactive — changes won't be detected"
   banner in FolderList.tsx if the watcher isn't running. This is a
   persistent hint, not a one-time toast.

The Rust side already has `watcher::WatcherHandle` state; just needs
a `get_watcher_status` command and a UI surface.

---

### 🟢 F7 — No timestamp/progress on initial scan

**Where**: wizard → main UI transition.

**Problem**: If the user picks a 500 GB folder of CSVs, the scan runs
for minutes. The UI shows no indication of how long it's been running
or how many files are left. Some power users will give up thinking
it's frozen.

**Fix**: The FolderList.tsx cards already show per-folder scan state
from `scansInFlight`. That's the right place. Just make sure the user
can find FolderList from the empty SearchPage (overlaps with F1).

---

### 🟢 F8 — "Local-first data analytics" tagline in sidebar is weak

**Where**: `App.tsx:208–210`.

**Problem**: The small subtitle under the Sery Link logo says
"Local-first data analytics." This is the v0.3 positioning, not
v0.5's "Understand your data" positioning. Anyone reading code on
GitHub will note the drift between the site copy and the app copy.

**Fix**: Change to `"Understand your data"` or just drop it — the
main title is enough on a small sidebar element.

---

### 🟢 F9 — OnboardingWizard doesn't mention the AI tier

**Where**: `OnboardingWizard.tsx` whole file.

**Problem**: The wizard is silent on the fact that AI queries are
available (on the paid tier). A user who just picked a folder has no
idea they can "ask questions" at all — they might poke around
searching for 10 minutes before discovering the AI surface exists.

This is a deliberate tradeoff (the "lead with local, sell the AI"
strategy from the market analysis), but the wizard could drop one
soft hint without front-loading a paywall. Example after the success
toast:

> "Want to ask questions in plain English? Try AI analysis free for
> 7 days — Settings → AI tier."

**Fix**: Low priority. Wait for beta feedback. If users aren't
discovering AI, add the nudge.

---

## Ordered fix list (if you want me to ship them)

| Sev | Fix | Effort | Impact |
|---|---|---|---|
| 🔴 | F1 — dedicated "no folders" empty state on SearchPage | 30 min | High |
| 🔴 | F2 — surface indexing progress on SearchPage | 1 hour | High |
| 🟡 | F3 — welcome copy names the job | 5 min | High |
| 🟡 | F4 — defaultPath on folder picker | 5 min | Medium |
| 🟡 | F6 — watcher status surface | 2 hours | Medium |
| 🟢 | F8 — fix the subtitle drift | 5 min | Low |
| 🟢 | F5 — minor error-branch tightening | skip | Low |
| 🟢 | F7 — handled by F1 | — | — |
| 🟢 | F9 — AI tier hint | wait | Low |

Total to knock out the 🔴 + 🟡 tier (the part that matters before
beta): ~4 hours.

---

## What I couldn't audit without running the app

- Native folder picker UX per OS (macOS sheet style vs Windows
  File Explorer modal vs Linux Gtk dialog)
- Animation smoothness during the wizard → main UI transition
- Accessibility (keyboard nav, screen reader labels)
- Dark-mode contrast on the HintCards at dim ambient light
- Actual scan performance on a 50 GB folder of CSVs

All of these require a running app with realistic data. Flag for
beta users to probe.
