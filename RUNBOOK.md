# Sery Link — Usage Runbook

How Sery Link actually works end-to-end. Written so a confused
user can pick it up cold and understand the difference between
**BYOK**, **MCP**, and the **cloud workspace** — three independent
paths that share one app.

> **Version note**: this runbook describes Sery Link as it ships in
> `CHANGELOG.md` v0.5.0. The first public release is still pending;
> the auto-update manifest at
> [`serylink-releases`](https://github.com/seryai/serylink-releases)
> is a `v0.0.0` placeholder until release artifacts are signed and
> uploaded.

---

## TL;DR — three paths, one app

Sery Link does three different things. They are **independent** —
you can use any one without the others.

| Path | What it does | Account? | Cost? |
|---|---|---|---|
| **A. BYOK** | Paste your Anthropic API key into Sery Link → ask questions in the app's `/ask` tab. Question + answer go direct to Anthropic. | None | You pay Anthropic per token |
| **B. MCP stdio** | Claude Desktop / Cursor / Continue spawns Sery Link as a local subprocess to read your folder. The *external* LLM client uses *its* key. | None | Whatever your LLM client costs |
| **C. Cloud workspace** | Connect every machine you own with one workspace key. Cross-machine search + (Plus) cross-machine AI. | Sery account | Free + 50 hosted queries/mo, or Plus $19 |

If you remember nothing else: **BYOK = in-app questions with your
key. MCP = external AI tools reading your folders. Cloud =
multi-machine.**

---

## Mental model

```
                       ┌─────────────────────────────┐
                       │  Sery Link — single binary  │
                       └──────────────┬──────────────┘
                                      │
        ┌─────────────────────────────┼─────────────────────────────┐
        │                             │                             │
        ▼                             ▼                             ▼
   PATH A: BYOK              PATH B: MCP stdio          PATH C: Cloud workspace
   ────────────              ─────────────────          ──────────────────────
   Settings → Sync           Settings → MCP             StatusBar → Connect
   paste Anthropic key       toggle folder, copy        paste workspace key
                             snippet → external client
        │                             │                             │
        ▼                             ▼                             ▼
   /ask tab in app           Claude Desktop / Cursor    api.sery.ai (cloud)
        │                    spawns sery-link via                   │
        ▼                    --mcp-stdio                            ▼
   api.anthropic.com                                       multi-machine fan-out
                                  │                       (other Sery Link
                                  ▼                        machines you own)
                          local folder read
                          (no network)
```

---

## Install

1. Download from <https://sery.ai/download> (or directly from
   [GitHub Releases](https://github.com/seryai/sery-link/releases)).
2. macOS: drag the `.app` to `/Applications/`. Windows: run the
   `.msi` installer. Linux: `.deb` or AppImage.
3. Launch. The app shows a **welcome wizard**: pick a folder, or
   skip. No signup, no email, no network calls until you ask for
   them.

### macOS first-launch warnings

Sery Link v0.5.x is **not Apple-notarized** (Developer enrollment
deferred — see DECISIONS.md). On first launch, depending on your
macOS version + how you downloaded, you may see one of two warnings:

#### *"Sery Link is damaged and can't be opened"*

Despite the wording, **the app isn't damaged.** macOS shows this for
any downloaded app that isn't Apple-notarized, and there's no
"Open Anyway" button for it. Strip the quarantine attribute that
macOS attached on download:

```bash
xattr -dr com.apple.quarantine /Applications/Sery\ Link.app
```

The app then opens normally. **One-time fix per install.** This is
the same approach Homebrew Cask and most independent macOS app
distributors document.

#### *"Sery Link cannot be opened because the developer cannot be verified"*

Milder variant. Open **System Settings → Privacy & Security**, scroll
to **Security**, click **"Open Anyway"** next to the Sery Link
entry. Or right-click the app in Finder → **Open** → **Open** in
the confirmation dialog.

Future releases will be Apple-notarized once Developer enrollment
lands; both warnings disappear at that point.

### Windows first-launch warning

SmartScreen says *"Windows protected your PC"*. Click **More info**
→ **Run anyway**. Once you've done it once, future launches and
auto-updates work without re-prompting.

### Linux

No warning. AppImage needs `chmod +x` first:

```bash
chmod +x Sery-Link_*.AppImage
./Sery-Link_*.AppImage
```

### Where things live on disk

| File | Purpose |
|---|---|
| `~/.seryai/config.json` | Watched folders, theme, auth mode, machine name. Plain JSON, hand-editable in a pinch |
| `~/.seryai/scan_cache.db` | Local SQL index of file metadata (DuckDB-backed) |
| `~/.seryai/sync_audit.jsonl` | One line per outbound network event — **the privacy proof on disk** |
| `~/.seryai/query_history.jsonl` | Local query transcript |
| OS keychain | Anthropic key (BYOK), workspace bearer token (Cloud). Never written to disk in plaintext |

### Uninstall

Deleting the app deletes the binary. **It does not delete `~/.seryai/`** —
purge that manually if you want a clean slate. Keychain entries for
"sery-link" persist until you remove them in
*Keychain Access* (macOS) / *Credential Manager* (Windows) / your
Secret Service (Linux).

---

## Path A — BYOK (paste an Anthropic key, ask questions in-app)

This is the **Free-tier moat**. Your prompt + your data + the
answer all go directly between your laptop and Anthropic. **Zero
bytes traverse Sery's servers.**

### Setup (one time, ~30 seconds)

1. Go to <https://console.anthropic.com/settings/keys> — create an
   API key. Copy it.
2. In Sery Link: **Settings → Sync → AI Provider**.
3. Paste the key. Click **Test & Save**.
4. Sery validates the key against `api.anthropic.com` before
   storing. The key goes into your OS keychain — nowhere else.

### Daily use

- Click **Ask** in the sidebar (or hit ⌘⇧S / Ctrl+Shift+S anywhere).
- Type a question. Hit Enter.
- Each message has a green badge: *"Direct to Anthropic"* — visible
  proof the request didn't traverse Sery.
- Tokens are billed to your Anthropic account.

### What gets logged?

Every BYOK call appends an entry to `~/.seryai/sync_audit.jsonl`:

```json
{"ts":"2026-04-29T14:33:01Z","kind":"byok_call","provider":"anthropic",
 "host":"api.anthropic.com","prompt_chars":127,"response_chars":842,
 "duration_ms":2341,"status":"ok"}
```

**The prompt + response text are never logged.** Only metadata
(host, character counts, latency). You can verify by opening the
file (Settings → Privacy → "Reveal audit file") and watching it as
you ask a question.

### Privacy proof

There's a unit test in the source —
`byok::anthropic::tests::anthropic_request_url_targets_anthropic_only` —
that fails if the BYOK code constructs any URL other than
`api.anthropic.com`. Source is AGPL-3.0; you can verify yourself.

### Limits today

- Anthropic only (OpenAI BYOK is on the v0.6 roadmap).
- Single-question, no streaming. Each `/ask` is one round trip.
- No tool use / SQL generation against your catalog. The question
  must stand on its own (use Path C — Cloud — when you want Sery to
  reason over your indexed schemas).

---

## Path B — MCP stdio (let Claude Desktop / Cursor see your files)

Sery Link doubles as a **local stdio MCP server**. When you toggle
a folder ON in Settings → MCP, it generates a config snippet you
paste into your LLM client. From then on, your LLM client spawns
Sery Link as a subprocess and gets nine read-only tools to browse
that folder.

### Setup (per folder, ~1 minute)

1. **Settings → MCP** in Sery Link.
2. Find the folder you want to expose. Toggle **Enable MCP** ON.
3. Sery shows you a copy-paste config snippet — one for each
   supported client:
   - Claude Desktop
   - Cursor
   - Continue
4. Click the copy icon. Paste into your client's config file.
5. Restart the client.

A typical Claude Desktop snippet looks like:

```json
{
  "mcpServers": {
    "sery-documents": {
      "command": "/Applications/Sery Link.app/Contents/MacOS/sery-link",
      "args": ["--mcp-stdio", "--root", "/Users/me/Documents"]
    }
  }
}
```

### What the LLM client sees

Nine tools, all read-only:

- `list_folder` — catalog of every file Sery has indexed
- `search_files` — search by filename or column name
- `get_schema` — columns + types for a tabular file
- `sample_rows` — first N rows
- `read_document` — extracted markdown for DOCX / PDF / PPTX / etc.
- `query_sql` — read-only SQL against a tabular file (DuckDB dialect)
- Three legacy tools (`query_data`, `list_datasets`, `get_dataset_schema`)
  for backward compat — prefer the six above.

### What it doesn't do

- **No upload.** The folder content stays on your disk. Only what
  the LLM explicitly asks for (e.g. column names, sample rows, SQL
  result rows) flows through stdio to the LLM client.
- **No account, no Sery cloud contact.** Path B is fully local. You
  can use it offline.
- **No write paths.** `INSERT` / `UPDATE` / `DROP` etc. are
  rejected at validation time.

### Run it standalone (no Sery Link UI)

If you'd rather skip the GUI entirely, the same MCP server is
published as a Rust crate:

```bash
cargo install sery-mcp
# then point Claude Desktop at the binary with --root <folder>
```

See <https://github.com/seryai/sery-mcp>. Same nine tools, same
on-device-only guarantee.

---

## Path C — Cloud workspace (multi-machine network)

This is what turns a one-machine app into a Tailscale-shape
data network. Install Sery Link on every machine you own; connect
each to the same workspace; ask questions that fan out across all
of them.

### Setup

1. Go to <https://app.sery.ai>. Sign up (Free or Plus).
2. **Settings → Workspace Keys → New Key.** Name it
   (e.g. `home-laptop`). Copy the key.
3. In Sery Link's status bar, click **Connect**. Paste the key.
4. Sery validates with `api.sery.ai`, stores the bearer token in
   your OS keychain, and starts a WebSocket tunnel.
5. The status indicator turns green. **Machines** tab activates.
6. Repeat on every other machine — each gets its own key but shares
   the workspace.

### What Sery uploads (catalog metadata only)

| Uploaded | Stays local |
|---|---|
| File paths (relative to the watched folder) | Raw file contents |
| Column names + inferred types | Anything outside watched folders |
| Row counts, null %, byte size | OS credentials |
| Dataset SHA / mtime (for change detection) | The Anthropic / OpenAI key (BYOK) |
| Optional: sample rows (toggle in Settings → Sync) | |
| Optional: extracted document text (toggle, OFF by default) | |

The two optional toggles are **off by default**. Sample rows + doc
text only sync if you opt in — that's the F2 privacy commitment.

### What you can do once connected (today, v0.5.0)

- **Cross-machine search**: at <https://app.sery.ai>, search column
  names across every machine. Returns hits with a `machine`
  column so you know where each file lives.
- **Workspace recipes**: hand-authored prompts you save once and run
  from any machine.
- **Schema-change notifications**: when a column changes shape on
  any machine, every other machine's tray gets a notification.
- **MCP from cloud**: <https://mcp.sery.ai> exposes the same nine
  tools as Path B but routes `query_sql` through the WebSocket
  tunnel to whichever machine owns the file. See
  [mcp-server/RUNBOOK.md](https://github.com/seryai/mcp-server/blob/main/RUNBOOK.md).

### Coming v0.6 (not shipped yet — don't expect it today)

- Cross-machine **AI queries** — ask one question on machine A and
  Sery dispatches it to every other machine in parallel, then
  merges results.

### Free vs Plus on the cloud path

| | Free | Plus ($19/mo) |
|---|---|---|
| Owned machines connected | Unlimited | Unlimited |
| Invited machines (other people) | 0 | 5 on your bill |
| Sery-hosted AI queries | 50 / month | Unlimited |
| BYOK queries | Unlimited | Unlimited |
| Catalog sync | Yes | Yes |
| MCP endpoint at `mcp.sery.ai` | No (403) | Yes |

---

## Watched folders

A *watched folder* is a directory Sery indexes locally. You can
have many of them — local disks, network shares, even remote sources
(HTTPS / S3) treated as virtual folders.

### Add a folder

- **Onboarding wizard** — pick the first one when you launch the
  app.
- **Settings → Folders → +** afterwards.
- **Drag-drop** onto the main window.

### Per-folder config

Each `WatchedFolder` carries:

| Field | Default | Meaning |
|---|---|---|
| `path` | (your pick) | Absolute path |
| `recursive` | `true` | Walk subdirectories |
| `exclude_patterns` | `.git`, `node_modules`, `.DS_Store`, `target`, `.venv`, `venv`, `.cache`, `~$*`, `.~lock*` | Globs to skip |
| `max_file_size_mb` | `1024` | Files larger than this are skipped |
| `mcp_enabled` | `false` | Whether to expose this folder via Path B's `--mcp-stdio` mode |
| `last_scan_at` | (auto) | RFC3339 timestamp |
| `last_scan_stats` | (auto) | dataset count, columns, errors, bytes, duration_ms |

### What gets indexed

**Tabular files** (read by an embedded SQL engine):
CSV · Parquet · XLSX · XLS

**Documents** (converted to markdown locally):
DOCX · PPTX · HTML · PDF

Larger or unsupported files are skipped silently and counted in the
scan stats.

---

## Settings tour

Five tabs:

- **General** — display name for this device, platform, hostname
- **Sync** — auto-sync on change, sync interval, document-text
  toggle (default OFF), AI Provider panel (BYOK)
- **App** — theme, launch at login, auto-update, notifications
- **MCP** — per-folder toggle + copy-to-clipboard config snippets
- **About** — versions, agent ID, workspace ID, export / import
  config, logout

The toggles default to **the most private setting** — sample rows
and document text are opt-in, not opt-out.

---

## Tray menu

Click the menubar icon (macOS) or system tray icon (Windows / Linux):

| Item | What it does |
|---|---|
| Status header (●/◐/○) | Live: Connected · Syncing · Offline · Error · Sync paused |
| Stats label | "N queries today · M total" from local stats |
| Show / Hide window | Toggle the main window |
| Pause / Resume Syncing | Halts the file watcher + cloud sync |
| Open Sery in Browser | Opens `https://app.sery.ai` (or the configured cloud URL) |
| Quit Sery Link | Exit |

Left-click toggles the main window — classic menubar-app pattern.

---

## Hotkey

| Shortcut | Action |
|---|---|
| ⌘⇧S (macOS) | **Quick-Ask** — bring window to front, jump to `/ask` (if BYOK is configured) or `/search` (if not), focus the input |
| Ctrl+Shift+S (Windows / Linux) | Same |
| ⌘K | In-window command palette |
| ⌘/ | Show keyboard shortcuts overlay |
| ⌘Enter | Submit current question (Ask / Search) |

---

## Privacy + audit

Sery's privacy guarantee is **enforced by code and verifiable on
disk**:

1. **Unit-tested provider isolation** — BYOK code can only target
   `api.anthropic.com`; tests fail if anyone ever changes that.
2. **Local audit log** — `~/.seryai/sync_audit.jsonl` records every
   outbound network event with timestamp, kind, host, byte counts,
   and outcome. **Never the prompt or response text.**
3. **Reveal in OS file manager** — Settings → Privacy → "Reveal
   audit file" opens the log so you can inspect it as you use the
   app.
4. **Capped at 10,000 entries** so it doesn't grow unbounded;
   older entries rotate out.
5. **AGPL-3.0 source** — you can verify what's actually running.

The Privacy tab also shows what crosses the network in human terms:
file paths (relative), column names, row counts, query results.
What stays local: raw files, OS credentials, anything outside
watched folders.

---

## Troubleshooting

### *"I pasted my Anthropic key but Ask says 'Not configured'"*

Settings → Sync → AI Provider — confirm the green checkmark is
showing. If "Test & Save" failed, the key was rejected by Anthropic
(check that you copied the whole `sk-ant-...` string and that your
account has API access).

### *"Claude Desktop doesn't see Sery's tools"*

1. Confirm the snippet is in `~/Library/Application Support/Claude/claude_desktop_config.json`
   (macOS) or `%APPDATA%\Claude\claude_desktop_config.json` (Windows).
2. Confirm the `command:` path actually points at the installed
   Sery Link binary. Try running it manually:
   ```
   "/Applications/Sery Link.app/Contents/MacOS/sery-link" --mcp-stdio --root /tmp
   ```
   Should output `Sery Link v0.5.0 — MCP stdio mode` and wait on
   stdin.
3. Restart Claude Desktop fully (Quit, then re-open).
4. The hammer / wrench icon in Claude Desktop should list the tools.

### *"Connect button says 'Invalid workspace key'"*

Workspace keys are 8 characters, generated at
<https://app.sery.ai/settings/workspace-keys>. They expire if you
delete the key or revoke the workspace. Generate a fresh one.

### *"Catalog says my files are in cloud"*

Open Settings → Privacy. The list shows what's actually been
uploaded. Files are *never* uploaded — only metadata. If you see
unexpected entries (e.g. document text), check Settings → Sync —
the *Include document text* toggle may be on. Toggle it off and
clear cloud metadata to remove what was already uploaded.

### *"App is offline but I'm online"*

Status bar → click the status indicator → "Reconnect". If that
fails, check the audit log (Privacy → Reveal audit file) for
network errors. The api.sery.ai cloud may be down — see
<https://sery.ai/status>.

### *"Tray menu won't go away after I quit"*

Known macOS edge case. Restart the OS or kill any leftover
`sery-link` processes:
```
pkill -9 sery-link
```

---

## What's NOT shipped yet (don't expect these today)

| Feature | Where it lives |
|---|---|
| Cross-machine AI fan-out | v0.6 roadmap |
| OpenAI BYOK | v0.6 roadmap |
| Finder context menu ("Ask Sery") | Year 2 |
| `seryai://pair` deep-link receiver UI | placeholder only in v0.5 |
| Full SOC 2 / HIPAA compliance posture | Not on near-term roadmap |
| iOS / Android native apps | Not planned |
| Team / Enterprise tier | After 5+ small firms ask for SSO unprompted |

---

## Reference

| What | Where |
|---|---|
| Source code | <https://github.com/seryai/sery-link> (AGPL-3.0) |
| Releases + auto-update manifest | <https://github.com/seryai/serylink-releases> |
| Standalone MCP CLI | <https://github.com/seryai/sery-mcp> on crates.io |
| Cloud MCP endpoint runbook | <https://github.com/seryai/mcp-server/blob/main/RUNBOOK.md> |
| Cloud dashboard | <https://app.sery.ai> |
| Cloud API | <https://api.sery.ai> |
| MCP endpoint | <https://mcp.sery.ai> |
| Status page | <https://sery.ai/status> |

---

## When in doubt

- **Want to ask questions in Sery's own UI?** → Path A (BYOK).
- **Want Claude Desktop / Cursor to see your files?** → Path B (MCP).
- **Want to search across multiple machines you own?** → Path C
  (Cloud).

You can use any combination. They don't fight.
