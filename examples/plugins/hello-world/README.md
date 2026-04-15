# Hello World Plugin

A minimal example plugin for Sery Link demonstrating the plugin system.

## Structure

- `plugin.json` - Plugin manifest with metadata and capabilities
- `plugin.wasm` - Compiled WebAssembly module (141 bytes)
- `src/lib.rs` - Rust source code (for reference)
- `Cargo.toml` - Build configuration

## Installation

To install this plugin:

1. Copy the entire `hello-world` directory to `~/.sery/plugins/com.sery.hello-world/`
2. The plugin will be automatically discovered by Sery Link
3. Enable it in Settings → Plugins

## Exported Functions

The plugin exports the following functions:

- `greet() -> i32` - Returns 42 as a success code
- `parse() -> i32` - Returns 0 (success) - placeholder for data-source parsing
- `_initialize()` - Called when the plugin is loaded

## Building from Source

To rebuild the WASM module:

```bash
cargo build --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/hello_world_plugin.wasm plugin.wasm
```

## Capabilities

- **data-source**: This plugin declares the data-source capability (though it's a stub implementation)

## Permissions

This plugin requires no permissions - it's completely sandboxed.

## Size

The compiled WASM module is only 141 bytes, demonstrating how lightweight plugins can be.
