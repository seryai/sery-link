# CSV Parser Plugin

A production example plugin for Sery Link demonstrating data-source capability.

## Features

- Parse CSV files and extract column information
- Count rows and columns
- Validate CSV format (consistent column counts)
- Minimal size (1.7KB WASM)

## Structure

- `plugin.json` - Plugin manifest with data-source capability and read-files permission
- `plugin.wasm` - Compiled WebAssembly module (1.7KB)
- `src/lib.rs` - Rust source code (no_std CSV parser)
- `Cargo.toml` - Build configuration

## Installation

To install this plugin:

1. Copy the entire `csv-parser` directory to `~/.sery/plugins/com.sery.csv-parser/`
2. The plugin will be automatically discovered by Sery Link
3. Enable it in Settings → Plugins

## Exported Functions

The plugin exports the following functions:

- `get_column_count() -> i32` - Returns number of columns in the CSV
- `get_row_count() -> i32` - Returns number of data rows (excluding header)
- `validate_csv() -> i32` - Returns 1 if CSV is valid, 0 if invalid
- `get_version() -> i32` - Returns plugin version (1000 = version 1.0)
- `_initialize()` - Called when the plugin is loaded

## Test Data

The plugin currently parses embedded test CSV data:
```csv
name,age,city
Alice,30,NYC
Bob,25,SF
Carol,35,LA
```

Results:
- Column count: 3 (name, age, city)
- Row count: 3 (data rows, excluding header)
- Valid: 1 (all rows have 3 columns)

## Future Enhancements

Phase 2 will add:
- Read CSV from actual files via `read_file` host function
- Return parsed data as JSON
- Handle quoted fields and escaped commas
- Support custom delimiters

## Building from Source

To rebuild the WASM module:

```bash
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/csv_parser_plugin.wasm plugin.wasm
```

## Capabilities & Permissions

- **Capability**: `data-source` - Parses data files
- **Permission**: `read-files` - Will read CSV files when host function is implemented

## Size

The compiled WASM module is only 1.7KB, demonstrating efficient plugin architecture.
