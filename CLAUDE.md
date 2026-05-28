# sery-link — Claude Code guide

Tauri 2 / Rust desktop app. The gateway: connects storage sources, runs SQL locally via DuckDB, exposes MCP. AGPL-3.0.

## Build & run

```bash
cd src-tauri
cargo check --tests          # fast type-check (always run this, not just cargo check)
cargo test                   # unit tests

# Full app (from repo root)
bun install
bun run tauri dev            # dev build + hot-reload frontend
bun run tauri build          # production build
```

> Always use `cargo check --tests` — test fixtures construct structs positionally; new fields fail CI but pass plain `cargo check`.

## Version bump

```bash
scripts/bump-version.sh 0.12.0    # updates sery-link + homebrew-tap + scoop-bucket, commits, tags, pushes
```

Never edit version files manually.

## Key source files

| File | Purpose |
|---|---|
| `src-tauri/src/sources.rs` | `SourceKind` enum — one variant per protocol |
| `src-tauri/src/db_engine.rs` | Query execution for all DB sources (DuckDB ATTACH, ClickHouse HTTP, MongoDB bridge, Redis scan) |
| `src-tauri/src/db_creds.rs` | OS keychain storage for DB passwords (`service = "sery-link-db"`) |
| `src-tauri/src/commands.rs` | All `#[tauri::command]` functions |
| `src-tauri/src/config.rs` | `Config` struct — persisted to `~/.seryai/config.json` |
| `src-tauri/src/lib.rs` | `invoke_handler!` registration — add every new command here |
| `src-tauri/src/agent_rpc/commands/sql.rs` | Agent SQL dispatch — file path or `db://source_id` |
| `src/components/AddSourceModal.tsx` | Add Source UI — one stage per source kind |
| `src/types/events.ts` | TypeScript mirror of Rust event payloads and `SourceKind` |

## Adding a new source type — checklist

1. **`sources.rs`** — add variant to `SourceKind`, add `default_*_port()` fn, update `is_database()` and `derive_name_from_kind()`
2. **`db_engine.rs`** — add execution path (extend `build_attach_string` for DuckDB ATTACH types, or add new `execute_*_query` fn for custom connectors)
3. **`db_creds.rs`** — no changes needed (same keychain pattern)
4. **`commands.rs`** — add `add_*_source` Tauri command, add match arms in `rescan_source_by_id` and `remove_source`
5. **`config.rs`** — add match arms in 4 locations: `filter_map`, `add_source` needle, inner dedup, `remove_source` mirror_path
6. **`lib.rs`** — register new command in `invoke_handler!`
7. **`agent_rpc/commands/sources.rs`** — add arm to `kind_str()`
8. **`events.ts`** — add variant to `SourceKind` discriminated union
9. **`AddSourceModal.tsx`** — add tile to `IMPLEMENTED`, new `Stage` variant, new stage component, update `PickerStage`, `ModalHeader`, `legacyIconKindForTile`
10. Run `cargo check --tests` — fix all exhaustive match errors before committing

## DB source query path

```
user SQL → agent_rpc/commands/sql.rs
  → if path starts with "db://":   db_engine::execute_db_query(source_id)
      → resolve source + load password from keychain
      → dispatch by SourceKind:
          MySQL/PostgreSQL/Snowflake/SQLite → DuckDB ATTACH (READ_ONLY)
          ClickHouse                        → reqwest::blocking HTTP
          MongoDB                           → mongodb crate → temp JSON → DuckDB
          Redis                             → redis crate SCAN → virtual `keys` table
  → else: existing DuckDB file engine
```

All DB sources enforce SELECT-only (`validate_db_sql`). 100k row cap. 60s timeout.

## Keychain pattern

```rust
// All DB sources use the same service
const SERVICE: &str = "sery-link-db";
// account = source_id (UUID)
db_creds::save(&source_id, &password)
db_creds::load(&source_id) -> Option<String>
db_creds::delete(&source_id)
```

SFTP, WebDAV, Dropbox, Azure, OneDrive each have their own `*_creds.rs` with the same shape.

## Commit & push policy

Always commit and push after finishing a logical unit of work. Never wait to be asked.
