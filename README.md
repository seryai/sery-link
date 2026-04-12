# Sery Link

Cross-platform desktop app that connects local data sources to Sery Cloud, enabling natural language queries over local files without uploading raw data. Part of [Sery.ai](https://sery.ai).

## Features

- 🔐 **Secure OAuth Authentication** - Browser-based auth with local callback server
- 📁 **Folder Watching** - Auto-detect changes in local Parquet, CSV, and Excel files
- 🔄 **Metadata Sync** - Automatically sync file schemas and metadata to the cloud
- 🚀 **Local Query Execution** - Run SQL queries locally using DuckDB
- 🌐 **WebSocket Tunnel** - Persistent connection for real-time query execution
- 🔒 **Secure Credential Storage** - OS-native keychain integration
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
- **Storage**: OS-native credential manager (Keychain/Credential Manager/Secret Service)
