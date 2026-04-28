# Sery Link

**Understand your data. Free and local.**

Cross-platform desktop app that indexes every CSV, spreadsheet, and document on your machines — find files by column name, inspect schemas and column stats in place, and (when you subscribe to sery.ai) ask questions across everything in plain English. Part of [Sery.ai](https://sery.ai).

## Features

- 🔎 **Column-aware search** — Global search bar matches filenames, column names, and extracted document content across every folder and every remote source in one pass
- 📊 **Per-file column profiles** — Open any file to see schema, sample rows, and column stats (null %, unique values, min/max/avg) auto-computed locally
- 📁 **Folder watching** — Auto-detect changes in local Parquet, CSV, Excel, and document files
- 🌐 **Remote sources** — Add public HTTPS URLs or S3 objects / bucket listings (credentials stored in the OS keychain, data fetched locally — never proxied through our servers)
- 📄 **Document support** — Convert DOCX, PPTX, HTML, PDF to Markdown via the in-process [`mdkit`](https://crates.io/crates/mdkit) Rust crate (bundled libpdfium + pandoc, ~12 MB)
- 💻 **Multiple machines** — Connect as many devices to a single workspace via workspace keys; cross-machine AI queries on Personal
- ⌨️ **Keyboard-first UX** — Command Palette (Cmd+K), keyboard shortcuts, fuzzy search
- 📜 **Query history** — Local JSONL persistence with statistics and CSV export
- 🔒 **Privacy-first** — Raw files never leave your machines (read-only file access). Works fully offline; connecting to sery.ai for AI queries is an explicit opt-in.
- 💻 **Beautiful UI** — Modern React interface with Tailwind CSS

## Development

### Prerequisites

- Node.js 20+ and pnpm
- Rust 1.88+ (install via [rustup](https://rustup.rs/))

### Setup

```bash
# Install dependencies
pnpm install

# Run in development mode
pnpm tauri dev
```

### Build for Production

```bash
pnpm tauri build
```

## Architecture

- **Frontend**: React 19 + TypeScript + Tailwind CSS
- **Backend**: Rust with Tauri 2.0 + WebSocket
- **Folder walking**: [`scankit`](https://crates.io/crates/scankit) — `walkdir` + size cap + exclude globs in one in-process Scanner.
- **Tabular extraction**: [`tabkit`](https://crates.io/crates/tabkit) — Parquet / CSV / XLSX / XLS schema + sample rows + row count, in-process; DuckDB stays as a fallback for the rare format tabkit doesn't claim.
- **Document → markdown**: [`mdkit`](https://crates.io/crates/mdkit) — bundled libpdfium for PDF, pandoc subprocess for DOCX/PPTX/EPUB/RTF/ODT/LaTeX, anytomd fallback for everything else. Fully in-process Rust; no Python interpreter, no sidecar fork.
- **Storage**: OS-native credential manager (Keychain/Credential Manager/Secret Service)

### Supported File Types

**Tabular Data** (tabkit):
- Parquet (`.parquet`)
- CSV (`.csv`)
- Excel (`.xlsx`, `.xls`)

**Documents** (mdkit):
- Word (`.docx`)
- PowerPoint (`.pptx`)
- HTML (`.html`, `.htm`)
- PDF (`.pdf`) — text-layer extraction via libpdfium; Apple Vision / Windows.Media.Ocr fallback for scanned pages
- Jupyter Notebooks (`.ipynb`)

## License

Sery Link is licensed under the
[GNU Affero General Public License v3.0 or later](./LICENSE)
(AGPL-3.0-or-later).

In short: you can use, inspect, modify, and redistribute the source
freely, including for commercial purposes. If you run a modified version
as a network service, you must make your modified source available to
users of that service under the same license. The AGPL makes the
privacy claim at the heart of Sery Link — "your files never leave your
machines" — auditable by anyone who wants to verify it.

The Sery.ai cloud backend (AI query orchestration, workspace catalog,
billing) is a separate, proprietary service and is not covered by this
license. Sery Link talks to that backend only when the user explicitly
opts in to the AI tier.

## Contributing

PRs are welcome. See [CONTRIBUTING.md](./CONTRIBUTING.md) for what we
accept, how to sign off commits (DCO), and what to expect from review.
For usage questions and bug reports, see [SUPPORT.md](./SUPPORT.md).
Security issues: `security@sery.ai` (do not file publicly).
