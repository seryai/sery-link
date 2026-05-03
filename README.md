# Sery Link

**Universal cloud storage browser, AI-era. Free + open source (AGPL-3.0).**

Connect every cloud storage you have — local, S3, Google Drive, plus SFTP / WebDAV / B2 / Azure / GCS / Dropbox / OneDrive (rolling out v0.7+). Browse, preview tables and Parquet files in-place, run SQL on remote bytes without downloading, and ask AI questions across all of it. Credentials stay in your OS keychain. Sery never sees your files.

> **Think of it as an AI-era Cyberduck** with column-aware search, preview-without-download for tabular files, and cloud AI on top. It's also the desktop endpoint for [Sery](https://sery.ai) — a private data network for your files (multi-machine workspace upgrade is opt-in).

## What you can do with one install

Sery Link works in three independent modes. Use any combination — they coexist by design. See [RUNBOOK.md](./RUNBOOK.md) for the full walkthrough.

| Mode | What it does | Sery account? |
|---|---|---|
| **Local — universal data gateway** | Connect every storage you have. Column-aware search across all of them, per-file column profiles, inline tabular preview (parquet footers read over the wire — no download), CSV/Excel → Parquet conversion. Runs fully offline. | Not required |
| **MCP stdio** | `Settings → MCP` toggle exposes a folder to Claude Desktop / Cursor / Continue via local stdio. The external LLM uses its own key. | Not required |
| **Cloud workspace** | Connect with a workspace key — AI chat across all your sources at app.sery.ai/chat, multi-machine catalog sync, cross-machine search, MCP cloud endpoint at mcp.sery.ai. | Free or Plus |

**Storage protocols supported (today):** Local disk · HTTPS public URLs · S3 · Google Drive (OAuth).
**Coming v0.7.x:** SFTP · WebDAV · Backblaze B2 · Azure Blob · Google Cloud Storage · Dropbox · OneDrive.

> **Where did BYOK go?** v0.5.3 shipped a paste-your-own-key `/ask` tab. v0.6.0 removed it — AI now lives in the cloud dashboard's `/chat` page, where the tool-use agent runs server-side and fans out queries across all your connected sources through the existing tunnel. See the [v0.6.0 changelog entry](./CHANGELOG.md#060--2026-05-01) for the full rationale.

## Features

- 🌐 **Browse every cloud you have, in one app** — local + S3 + Google Drive today; SFTP / WebDAV / B2 / Azure / GCS / Dropbox / OneDrive in v0.7+. Credentials in the OS keychain, fetched directly from your machine.
- ⚡ **Preview without downloading** — click any Parquet on S3 → schema + sample rows in <2s via the Parquet footer (no full file pull). CSV / TSV / Excel preview streams just enough.
- 📊 **Per-file column profiles** — null %, unique values, min/max/avg, computed locally via DuckDB SUMMARIZE.
- 🔎 **Column-aware search** — match filenames, column names, and extracted document content across every connected source in one pass.
- 📁 **Folder + bucket watching** — auto-detect changes in Parquet, CSV, Excel, and document files.
- 🔄 **Convert to Parquet** — turn any CSV / TSV / Excel into Parquet next to the source. The fastest way to make a pile of CSVs queryable.
- 📄 **Documents → markdown** — DOCX, PPTX, HTML, PDF via the in-process [`mdkit`](https://crates.io/crates/mdkit) Rust crate (bundled libpdfium + pandoc).
- 💻 **Multi-machine workspace** (opt-in) — connect as many of your own machines as you want via one workspace key. AI chat at app.sery.ai/chat fans queries out across them.
- ⌨️ **Keyboard-first UX** — Command Palette (Cmd+K), Quick-Search hotkey (Cmd+Shift+S), fuzzy search.
- 🔒 **Verifiable privacy** — every outbound network event is logged to `~/.seryai/sync_audit.jsonl` with byte counts and host but never prompt or response text. Open the file in Settings → Privacy and watch it as you work.
- 📦 **AGPL-3.0** — the protocol, auth flow, audit log format, and command surface are all inspectable. The privacy claims on [sery.ai/trust](https://sery.ai/trust) are verifiable in the source you're reading.

## Install

Pre-built binaries for macOS (Apple Silicon + Intel), Windows, and Linux are on the [Releases page](https://github.com/seryai/sery-link/releases).

Or build from source — see [Development](#development) below.

> **First-launch on macOS**: builds aren't Apple-notarized yet, so Gatekeeper blocks the first open. Right-click the app → Open, or System Settings → Privacy & Security → "Open Anyway". Auto-updates after that work normally (verified via [minisign](https://jedisct1.github.io/minisign/), independent of Apple's signing).

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  Sery Link (this repo, AGPL-3.0)                             │
│  ┌────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │  React UI  │  │  Tauri 2.x   │  │  Rust kits           │  │
│  │  + Tailwind│←→│  + WebSocket │  │  scankit · tabkit ·  │  │
│  └────────────┘  └──────────────┘  │  mdkit · sery-mcp    │  │
│                                    └──────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
                              │ (workspace key, opt-in)
                              ▼
┌──────────────────────────────────────────────────────────────┐
│  Sery Cloud (closed source)                                  │
│  Identity · Catalog · Routing · MCP gateway                  │
│  Holds: file paths, schemas, optional sample rows.           │
│  Never holds: file contents.                                 │
└──────────────────────────────────────────────────────────────┘
```

The kits ([`scankit`](https://crates.io/crates/scankit), [`tabkit`](https://crates.io/crates/tabkit), [`mdkit`](https://crates.io/crates/mdkit), [`sery-mcp`](https://crates.io/crates/sery-mcp)) are separate crates published on crates.io — pull them into your own Tauri / Iced / native Rust desktop apps if you want.

## Supported file types

**Tabular** (via tabkit): Parquet · CSV · XLSX · XLS

**Documents** (via mdkit): DOCX · PPTX · HTML · PDF (text-layer + Apple Vision / Windows.Media.Ocr fallback for scanned pages) · Jupyter Notebooks

## Development

### Prerequisites

- Node.js 20+ and pnpm
- Rust 1.88+ ([rustup](https://rustup.rs/))

### Setup

```bash
pnpm install

# Local dev — point at a local backend if you have one
SERY_API_URL=http://localhost:8000 \
SERY_WEBSOCKET_URL=ws://localhost:8000 \
SERY_WEB_URL=http://localhost:3000 \
pnpm tauri dev

# Or just run against production sery.ai (the defaults if env vars aren't set)
pnpm tauri dev
```

### Build for production

```bash
# CI uses .github/workflows/release.yml — triggered by pushing a v* tag.
# Locally, build a single-platform release artifact via:
./scripts/build-local.sh arm64    # or `intel`, or omit for both
```

See [RELEASE.md](./RELEASE.md) for the full release workflow (signing, GitHub Releases, auto-updater manifest).

## Verify the privacy claims yourself

The marketing site says "raw files never leave your machines, the cloud holds the catalog never the data." Don't take our word for it:

| Claim | Where it's enforced |
|---|---|
| Workspace catalog is metadata-only | [`src-tauri/src/scanner.rs`](./src-tauri/src/scanner.rs) — what gets read; [`src-tauri/src/sync.rs`](./src-tauri/src/sync.rs) — what gets uploaded |
| Cloud AI queries fan out via the workspace tunnel, not by uploading data | [`src-tauri/src/websocket.rs`](./src-tauri/src/websocket.rs) — long-lived WebSocket; the cloud agent sends SQL, the desktop runs it on local DuckDB and streams rows back |
| Local audit log is the source of truth | [`src-tauri/src/audit.rs`](./src-tauri/src/audit.rs) — schema + rotation; `~/.seryai/sync_audit.jsonl` on disk |
| Document text is opt-in (off by default) | [`src-tauri/src/config.rs`](./src-tauri/src/config.rs) — `SyncConfig::include_document_text = false` |

## License

[GNU Affero General Public License v3.0 or later](./LICENSE) (AGPL-3.0-or-later).

In short: you can use, inspect, modify, and redistribute the source freely, including for commercial purposes. If you run a modified version as a network service, you must make your modified source available to users of that service under the same license. AGPL is what makes the privacy claim at the heart of Sery Link — "your files never leave your machines" — auditable by anyone who wants to verify it.

The Sery cloud backend (identity, workspace catalog, AI orchestration, billing) is a separate, proprietary service and is not covered by this license. Sery Link talks to that backend only when the user explicitly opts in to the cloud workspace mode.

## Contributing

PRs welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for what we accept, DCO sign-off, and review expectations. Usage questions + bug reports → [SUPPORT.md](./SUPPORT.md). Security issues → `security@sery.ai` (don't file publicly — see [SECURITY.md](./SECURITY.md)).
