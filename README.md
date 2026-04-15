# Sery Link

**Local-first data analytics. Your data never leaves your machine.**

Cross-platform desktop app that analyzes local data sources (Parquet, CSV, Excel, documents) using AI. Cloud sync is optional — tunnel mode queries run entirely on your computer. Part of [Sery.ai](https://sery.ai).

## Features

- 🔐 **Secure OAuth Authentication** - Browser-based auth with local callback server
- 📁 **Folder Watching** - Auto-detect changes in local Parquet, CSV, Excel, and document files
- 📄 **Document Support** - Convert DOCX, PPTX, HTML, PDF to Markdown using bundled MarkItDown sidecar
- 🔄 **Metadata Sync** - Automatically sync file schemas and metadata to the cloud
- 🚀 **Local Query Execution** - Run SQL queries locally using DuckDB
- 🌐 **WebSocket Tunnel** - Persistent connection for real-time query execution
- 🧩 **Plugin System** - Extend functionality with WebAssembly plugins (5 built-in examples: CSV parser, JSON transformer, HTML viewer, clipboard utilities, text analyzer)
- 🛒 **Plugin Marketplace** - Discover, search, and install community plugins (backend infrastructure ready)
- ⌨️ **Keyboard-First UX** - Command Palette (Cmd+K), keyboard shortcuts, fuzzy search
- 🔗 **Relationship Graph** - Visualize connections between datasets (schema + query analysis)
- 📊 **Query History** - Local JSONL persistence with statistics and CSV export
- 🔒 **Secure Credential Storage** - OS-native keychain integration
- 🔒 **Privacy-First** - Raw files never leave your machine (read-only file access). In tunnel mode, queries execute locally with zero data upload. Optional cloud sync for performance mode.
- 💻 **Beautiful UI** - Modern React interface with Tailwind CSS

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

**Marketplace:** Backend infrastructure ready. Search, filter, install plugins programmatically. Frontend UI coming in v0.2.0.

See example plugins in `examples/plugins/` for development reference.
