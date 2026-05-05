# Sery Link — Usage Runbook

How Sery Link actually works end-to-end. Written so a confused
user can pick it up cold and understand the difference between
**Local (the universal gateway)**, **MCP**, and the **cloud
workspace** — three independent paths that share one app.

> **Version note**: this runbook describes Sery Link as it ships in
> v0.6.2 (file-manager pivot — no on-device LLM keys, AI lives in
> the cloud dashboard; recursive S3 listings; pre-flight credential
> check; keychain caching; sidebar gating). For the v0.5.3-and-
> earlier BYOK / `/ask` flow, check out the `v0.5.3` git tag.

---

## TL;DR — three paths, one app

Sery Link does three different things. They are **independent** —
you can use any one without the others.

| Path | What it does | Account? | Cost? |
|---|---|---|---|
| **A. Local — the universal data gateway** | Connect every cloud storage you have. **9 protocols** (Local, HTTPS, S3, Drive, SFTP, WebDAV, Dropbox, Azure, OneDrive) + **4 S3-compatible presets** (B2, Wasabi, R2, GCS). Browse, preview tables without downloading, run profiles, search across all of them. Convert CSV / TSV / Excel → Parquet. Document → markdown. Fully offline (no cloud contact). | None | Free |
| **B. MCP stdio** | Claude Desktop / Cursor / Continue spawns Sery Link as a local subprocess to read your folder. The *external* LLM client uses *its* key. | None | Whatever your LLM client costs |
| **C. Cloud workspace** | Connect with a workspace key — AI chat at app.sery.ai (server-side agent fans queries out across all your sources via the existing tunnel), multi-machine catalog sync, cross-machine search. | Sery account | Free + 50 hosted queries/mo, or Plus $19 |

If you remember nothing else: **Local = universal cloud storage browser
with SQL on remote bytes. MCP = external AI tools reading your folders.
Cloud = AI chat across everything + multi-machine network.**

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
   PATH A: Local             PATH B: MCP stdio          PATH C: Cloud workspace
   ─────────────             ─────────────────          ──────────────────────
   pick a folder             Settings → MCP             StatusBar → Connect
   in the app                toggle folder, copy        paste workspace key
                             snippet → external client
        │                             │                             │
        ▼                             ▼                             ▼
   search · preview          Claude Desktop / Cursor    app.sery.ai (cloud)
   convert · profile         spawns sery-link via         /chat → server-side
   (no network)              --mcp-stdio                  agent fans SQL out
                                  │                       over WebSocket to
                                  ▼                       your local DuckDB
                          local folder read               (rows stream back,
                          (no network)                     never the file)
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
| OS keychain | Workspace bearer token (Cloud). OAuth refresh tokens (Google Drive, Dropbox, OneDrive). SFTP / WebDAV / Dropbox PAT / Azure SAS credentials. Never written to disk in plaintext |

### Uninstall

Deleting the app deletes the binary. **It does not delete `~/.seryai/`** —
purge that manually if you want a clean slate. Keychain entries for
"sery-link" persist until you remove them in
*Keychain Access* (macOS) / *Credential Manager* (Windows) / your
Secret Service (Linux).

---

## Path A — Local (file manager, no signup)

The free, offline path. Sery Link indexes a folder, gives you
column-aware search across files, lets you preview any tabular
file inline, converts CSV / TSV / Excel to Parquet, and reads
documents (DOCX, PPTX, PDF, HTML) into markdown. **No network,
no account.**

### Setup (one click)

1. Open Sery Link → **+ Add folder** (or pick one in the welcome
   wizard). Sery walks the folder, infers column types via tabkit,
   and caches metadata in `~/.seryai/scan_cache.db`.
2. Done. The Files view lists every file; click any to inspect.

### What you can do

- **Column-aware search.** Top bar searches filenames, column
  names, and extracted document text in one pass.
- **Inline preview.** Open a CSV / Parquet / Excel file → see
  schema, sample rows, and per-column profile (null %, unique
  values, min/max/avg). Virtualized — handles wide tables fine.
- **Convert to Parquet.** Tabular files get a Convert button that
  writes a Parquet sibling next to the source. Useful for piping
  ad-hoc CSVs into the cloud workspace catalog later.
- **Folder filters.** Format (Parquet / CSV / Excel / Document),
  recency (24h / 7d / 30d / All), and sort (Name / Newest /
  Largest). Selections persist per-folder.
- **Documents → markdown.** DOCX, PPTX, HTML, PDF (text layer +
  Apple Vision / Windows.Media.Ocr fallback for scans), and
  Jupyter notebooks all become indexable markdown via the
  in-process `mdkit` Rust crate.

### What it does NOT do (in v0.6.0)

- **No on-device AI.** v0.5.x had a `/ask` tab where you pasted
  an Anthropic / OpenAI / Gemini key and asked questions on local
  DuckDB. That's gone — see [v0.6.0 changelog](./CHANGELOG.md#060--2026-05-01)
  for the rationale. AI lives in cloud `/chat` (Path C) or in
  your own LLM client via MCP (Path B).
- **No network.** Path A is fully offline. The audit log
  (`~/.seryai/sync_audit.jsonl`) will be empty until you connect
  a workspace or add a remote source.

### Remote sources (still local, just non-disk)

The Sources sidebar accepts more than disk paths. Each row uses
its own credential storage in the OS keychain (or no creds for
public/anonymous protocols), and bytes flow direct to your
machine — the cloud never proxies.

- **Public HTTPS URLs** — point at a CSV / Parquet hosted
  publicly; Sery fetches and indexes it like a local file.
- **S3 buckets** (+ Backblaze B2 / Wasabi / Cloudflare R2 / GCS
  presets) — access keys in the keychain. DuckDB-httpfs reads
  the bytes in place; nothing cached.
- **Google Drive** — OAuth (PKCE + loopback redirect), per-folder
  Watch, hourly background refresh. Files cache locally under
  `~/.seryai/gdrive_cache/`. Storage tab shows disk usage and a
  Clear-cache button.
- **SFTP** — password or SSH key auth. Concurrent downloads
  via 4 parallel sessions. Cache: `~/.seryai/sftp-cache/<id>/`.
- **WebDAV** — Anonymous / Basic / Digest auth (Nextcloud, ownCloud,
  Apache mod_dav, etc). Cache: `~/.seryai/webdav-cache/<id>/`.
- **Dropbox** — Connect-with-Dropbox OAuth (PKCE no-redirect) or
  Personal Access Token. Tokens auto-refresh. Cache:
  `~/.seryai/dropbox-cache/<id>/`.
- **Azure Blob** — SAS-token auth. Cache:
  `~/.seryai/azure-cache/<id>/`.
- **OneDrive** — Microsoft device-code OAuth. Tokens
  auto-refresh. Cache: `~/.seryai/onedrive-cache/<id>/`.

All 5 cache-and-scan kinds (SFTP / WebDAV / Dropbox / Azure /
OneDrive) share the same incremental sync (skip files whose
remote size + mtime match the previous walk), concurrent
downloads (4 in flight), and per-byte progress for files >10MB.

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
| Dataset SHA / mtime (for change detection) | OS credentials (S3 keys, Drive refresh tokens) |
| Optional: sample rows (toggle in Settings → Sync) | |
| Optional: extracted document text (toggle, OFF by default) | |

The two optional toggles are **off by default**. Sample rows + doc
text only sync if you opt in — that's the F2 privacy commitment.

### What you can do once connected (v0.6.0)

- **Cross-machine search** at <https://app.sery.ai>: search column
  names across every machine. Returns hits with a `machine` column
  so you know where each file lives.
- **AI chat** at <https://app.sery.ai/chat>: ask plain-English
  questions about any tabular file in your workspace. The agent
  runs **server-side**; when it needs to query a file, it sends
  SQL down the WebSocket tunnel to the machine that owns it,
  the desktop runs that SQL on local DuckDB, and only the result
  rows stream back to the cloud. **Raw files never leave the
  device.** Multi-machine questions fan out — each machine runs
  its slice, and the dashboard shows a per-machine
  rows / ms / share-% breakdown.
- **Workspace recipes**: prompts saved once on any machine, runnable
  from every other. Click Run in Sery Link's Recipes view → opens
  the prompt in your browser.
- **Schema-change notifications**: when a column changes shape on
  any machine, every other machine's tray gets a notification.
- **MCP from cloud**: <https://mcp.sery.ai> exposes the same nine
  tools as Path B but routes `query_sql` through the WebSocket
  tunnel to whichever machine owns the file. See
  [mcp-server/RUNBOOK.md](https://github.com/seryai/mcp-server/blob/main/RUNBOOK.md).

### Why does AI live in the cloud now (and not on the desktop)?

v0.5.x shipped a paste-your-own-key `/ask` tab that translated
questions into SQL on-device. It was unreliable across question
shapes (meta-questions, content-search, multi-file) and adding a
weaker parallel implementation on every desktop install hurt more
than it helped. The cloud agent already handles fan-out across
machines via the tunnel and ships once instead of N times. See
[v0.6.0 changelog entry](./CHANGELOG.md#060--2026-05-01).

### Free vs Plus on the cloud path

| | Free | Plus ($19/mo) |
|---|---|---|
| Owned machines connected | Unlimited | Unlimited |
| Invited machines (other people) | 0 | 5 on your bill |
| Sery-hosted AI queries (`/chat`) | 50 / month | Unlimited |
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

- **General** — display name for this device, platform, hostname
- **Sync** — auto-sync on change, sync interval, document-text
  toggle (default OFF), sample-rows toggle
- **Storage** — disk usage breakdown (scan cache, Drive cache,
  audit log) + Clear-cache buttons
- **Privacy** — outbound activity counters (Syncs, errors), reveal
  audit file in Finder/Explorer
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
| ⌘⇧S (macOS) | **Quick-Ask** — bring window to front, jump to `/search`, focus the input |
| Ctrl+Shift+S (Windows / Linux) | Same |
| ⌘K | In-window command palette |
| ⌘/ | Show keyboard shortcuts overlay |
| ⌘Enter | Submit current search |

---

## Privacy + audit

Sery's privacy guarantee is **enforced by code and verifiable on
disk**:

1. **Cloud AI fan-out runs on your machine.** When the cloud agent
   needs data, it sends SQL down the WebSocket tunnel; your
   desktop runs the SQL on local DuckDB and streams only the
   result rows back. The raw file never leaves the device. See
   [`src-tauri/src/websocket.rs`](./src-tauri/src/websocket.rs).
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

### *"Where do I paste my Anthropic / OpenAI key?"*

You don't, in v0.6+. The desktop AI flow was removed — see
[v0.6.0 changelog](./CHANGELOG.md#060--2026-05-01). Use cloud
`/chat` (Path C) for AI, or wire your own LLM client to a folder
via MCP (Path B).

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
| Finder context menu ("Ask Sery") | Year 2 |
| `seryai://pair` deep-link receiver UI | placeholder only in v0.6 |
| Full SOC 2 / HIPAA compliance posture | Not on near-term roadmap |
| iOS / Android native apps | Not planned |
| Team / Enterprise tier | After 5+ small firms ask for SSO unprompted |

## What was removed in v0.6.0

| Feature | Status |
|---|---|
| BYOK (paste Anthropic / OpenAI / Gemini key on the desktop) | Removed — use cloud `/chat` instead |
| `/ask` tab + on-device text-to-SQL agent | Removed — placeholder route now redirects to dashboard |
| AI Provider settings panel | Removed |
| Quick-Ask hotkey routing to `/ask` | Now always routes to `/search` |

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

- **Want a free local file manager + indexer?** → Path A (Local).
- **Want Claude Desktop / Cursor to see your files?** → Path B (MCP).
- **Want AI chat over your files, optionally across multiple
  machines you own?** → Path C (Cloud) and ask in `/chat` at
  app.sery.ai.

You can use any combination. They don't fight.
