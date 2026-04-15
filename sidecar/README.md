# MarkItDown Sidecar

This directory contains the **MarkItDown sidecar** — a standalone Python binary bundled with Sery Link that converts documents (DOCX, PPTX, HTML, PDF, etc.) to Markdown.

## Why a Sidecar?

Sery Link is written in Rust. Microsoft's MarkItDown is a Python library. The sidecar pattern allows us to:

1. **Call MarkItDown from Rust** without requiring users to install Python
2. **Bundle everything** into a single installer (no external dependencies)
3. **Preserve privacy** — raw files never leave the user's machine, only extracted Markdown syncs to the backend

## Architecture

```
┌─────────────┐         stdin         ┌──────────────────┐
│  Sery Link  │────────────────────────▶│  markitdown-     │
│   (Rust)    │   "/path/to/file.docx" │     sidecar      │
│             │                        │    (Python)      │
│             │◀───────────────────────│                  │
└─────────────┘   JSON response        └──────────────────┘
                  {"success":true,
                   "markdown":"..."}
```

The sidecar binary is a **self-contained Python executable** created with PyInstaller. It includes:
- Python 3.11 interpreter
- MarkItDown library
- All dependencies (mammoth, lxml, pdfminer, pptx, Pillow, magika, etc.)

**No external Python installation required.**

## Files

- `markitdown_worker.py` — The Python script that wraps MarkItDown
- `markitdown-sidecar.spec` — PyInstaller configuration (includes magika model data)
- `build.py` — Build script that compiles the sidecar binary
- `requirements.txt` — Python dependencies
- `dist/markitdown-sidecar` — The compiled binary (macOS ARM64)

## Building

To rebuild the sidecar binary:

```bash
cd sidecar
python3 build.py
```

This will:
1. Install PyInstaller
2. Install MarkItDown + dependencies
3. Compile everything into `dist/markitdown-sidecar`

**Binary size:** ~180 MB (includes Python + all deps)

## Testing

Test the binary directly:

```bash
echo "/path/to/document.docx" | dist/markitdown-sidecar | python3 -m json.tool
```

Expected output:
```json
{
  "success": true,
  "markdown": "# Document Title\n\nContent here...",
  "error": null
}
```

## Integration

The sidecar is registered in `tauri.conf.json`:

```json
"bundle": {
  "externalBin": [
    "../sidecar/dist/markitdown-sidecar"
  ]
}
```

Tauri automatically:
- Bundles the binary into the `.app` (macOS) / `.exe` installer (Windows)
- Makes it available at runtime in the same directory as the main binary

The Rust code calls it in `src-tauri/src/scanner.rs`:

```rust
fn try_sidecar_conversion(file_path: &Path) -> Option<String> {
    let sidecar_path = std::env::current_exe()
        .ok()?
        .parent()?
        .join("markitdown-sidecar");

    let output = Command::new(&sidecar_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?
        .wait_with_output()?;

    // Parse JSON response...
}
```

## Fallback Strategy

Sery Link tries the sidecar first, then falls back to **anytomd** (Rust-native) if it fails:

1. **MarkItDown sidecar** (primary) — More capable, handles DOCX/PPTX/PDF/HTML/IPYNB
2. **anytomd** (fallback) — Faster, Rust-native, but less capable

This two-tier approach ensures we always extract *something*, even if the sidecar fails.

## Cross-Platform

The current binary (`dist/markitdown-sidecar`) is macOS ARM64 only.

To build for other platforms:

- **macOS Intel**: Run `python3 build.py` on an Intel Mac
- **Windows**: Run `python build.py` on Windows (produces `markitdown-sidecar.exe`)
- **Linux**: Run `python3 build.py` on Linux

The Tauri bundle system will automatically pick the right binary for each platform.

## CI/CD TODO

In production, you'd build the sidecar binaries in CI for all platforms:

```yaml
# .github/workflows/build-sidecar.yml
- name: Build macOS sidecar
  run: cd sidecar && python3 build.py

- name: Upload artifact
  uses: actions/upload-artifact@v3
  with:
    name: sidecar-macos-arm64
    path: sidecar/dist/markitdown-sidecar
```

Then reference the CI-built binaries in `tauri.conf.json`.

## Privacy Model

**Important:** The sidecar runs **entirely on the user's machine**. Raw documents never leave their computer.

- ❌ Raw DOCX files: **never** sent to the backend
- ✅ Extracted Markdown: synced to the backend for AI analysis

This is critical for enterprise users with sensitive documents.

## Performance

| Operation | Time (avg) |
|-----------|------------|
| DOCX (5k chars) | ~500ms |
| PPTX (20 slides) | ~1-2s |
| PDF (10 pages) | ~2-3s |

The sidecar spawns a new Python process per file, so it's slower than a long-running daemon. For bulk processing, consider batching or using the backend script (`process_local_documents.py`) instead.

## Comparison: Sidecar vs Backend Script

| | Sidecar | Backend Script |
|---|---------|----------------|
| **Use case** | Real-time, on-demand conversion | Bulk processing of existing files |
| **Runs on** | User's desktop | Backend server |
| **When** | New files detected by file watcher | Manual one-time backfill |
| **Pros** | Instant UX, privacy-first | Can process cloud S3 docs |
| **Cons** | Slower (spawns per file) | Requires backend access |

**Best practice:** Use the sidecar for new files, use the backend script for backfilling existing data.
