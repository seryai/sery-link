# Sery Link

**Understand your data. Free and local.**

Cross-platform desktop app that indexes every CSV, spreadsheet, and document on your machines — find files by column name, inspect schemas and column stats in place, and (when you subscribe to sery.ai) ask questions across everything in plain English. Part of [Sery.ai](https://sery.ai).

## Features

- 🔎 **Column-aware search** — Global search bar matches filenames, column names, and extracted document content across every folder and every remote source in one pass
- 📊 **Per-file column profiles** — Open any file to see schema, sample rows, and column stats (null %, unique values, min/max/avg) auto-computed locally
- 📁 **Folder watching** — Auto-detect changes in local Parquet, CSV, Excel, and document files
- 🌐 **Remote sources** — Add public HTTPS URLs or S3 objects / bucket listings (credentials stored in the OS keychain, data fetched locally — never proxied through our servers)
- 📄 **Document support** — Convert DOCX, PPTX, HTML, PDF to Markdown using bundled MarkItDown sidecar
- 💻 **Multiple machines** — Connect as many devices to a single workspace via workspace keys; cross-machine AI queries on Personal
- 🧩 **Plugin system** — Extend functionality with WebAssembly plugins (5 built-in examples: CSV parser, JSON transformer, HTML viewer, clipboard utilities, text analyzer)
- 🛒 **Plugin marketplace** — Discover, search, and install community plugins
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
- **Backend**: Rust with Tauri 2.0, DuckDB, WebSocket
- **Document Processing**: MarkItDown sidecar (bundled Python binary, 180 MB)
- **Plugin Runtime**: WebAssembly (wasmer 7.1.0) with sandboxed execution
- **Storage**: OS-native credential manager (Keychain/Credential Manager/Secret Service)

### Supported File Types

**Tabular Data** (DuckDB):
- Parquet (`.parquet`)
- CSV (`.csv`)
- Excel (`.xlsx`, `.xls`)

**Documents** (MarkItDown sidecar):
- Word (`.docx`)
- PowerPoint (`.pptx`)
- HTML (`.html`, `.htm`)
- PDF (`.pdf`)
- Jupyter Notebooks (`.ipynb`)

See [SIDECAR_IMPLEMENTATION.md](./SIDECAR_IMPLEMENTATION.md) for details on the document processing architecture.

### Plugin System

Extend Sery Link with WebAssembly plugins. Plugins run in a sandboxed environment with fine-grained permissions.

**Built-in Example Plugins:**
- 📊 **CSV Parser** (2.4KB) - Parse, validate, count rows/columns
- 🔄 **JSON Transformer** (5.7KB) - Pretty-print, minify, validate JSON
- 📝 **HTML Viewer** (9.8KB) - Extract text, count tags, validate structure
- 📋 **Clipboard Utility** (4.2KB) - Read, write, transform clipboard content
- 📖 **Text Analyzer** (9.9KB) - Readability metrics, sentiment analysis, statistics

**Plugin Capabilities:**
- `data-source` - Custom file format parsers
- `viewer` - Data renderers and visualizers
- `transform` - Data transformations
- `exporter` - Export to custom formats
- `ui-component` - UI extensions

**Plugin Permissions:**
- `read-files` - Read from watched folders
- `execute-commands` - Run external commands
- `network` - Make HTTP requests
- `clipboard` - Access clipboard

**Plugin Location:** `~/.sery/plugins/[plugin-id]/`

**Marketplace:** Discover, search, install community plugins from the app UI.

See example plugins in `examples/plugins/` for development reference.
The public community plugin directory lives at
[seryai/serylink-releases](https://github.com/seryai/serylink-releases).

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
