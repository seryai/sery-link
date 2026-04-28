# libpdfium runtime library

The Tauri app bundles `libpdfium.{dylib,so,dll}` (one of the three,
depending on target) into `<App>.app/Contents/Resources/libpdfium/`
on macOS, or `<binary-dir>/resources/libpdfium/` on Linux/Windows.
The bundled file is loaded at runtime by mdkit's `PdfiumExtractor`
— see `src-tauri/src/scanner.rs` for the discovery logic.

## What's committed here

`libpdfium.dylib` for **macOS arm64**, fetched from
[bblanchon/pdfium-binaries][bblanchon] release `chromium/7811`
(PDFium 149.0.7811.0).

This is the dev team's primary target, so committing it gives
"works after `git clone`" ergonomics for `pnpm tauri dev` without
forcing every dev to run a fetch script.

## Building for other platforms

The release CI (`.github/workflows/release.yml`) overwrites this
file with the per-target binary just before `tauri-action` runs.
Locally, you can do the same:

```bash
TARGET=linux-x64 scripts/fetch-libpdfium.sh --force
TARGET=win-x64   scripts/fetch-libpdfium.sh --force
TARGET=mac-x64   scripts/fetch-libpdfium.sh --force
TARGET=mac-arm64 scripts/fetch-libpdfium.sh --force
```

Valid `TARGET` values: `mac-arm64`, `mac-x64`, `linux-x64`,
`linux-arm64`, `linux-arm`, `win-x64`, `win-x86`.

The script clears any pre-existing `libpdfium.{dylib,so}` /
`pdfium.dll` from this directory before extracting, so you can flip
between targets without ending up with multiple binaries in the
bundle.

## Bumping the pdfium version

Edit `PDFIUM_RELEASE` in `scripts/fetch-libpdfium.sh`. Releases are
chromium-tracked; pick a recent tag from
<https://github.com/bblanchon/pdfium-binaries/releases>.

After bumping, refetch for your dev target and re-test PDF
extraction end-to-end before merging.

## License

`libpdfium` is BSD-3-Clause licensed (Google's PDFium project).
The bundled binary's license text ships with each release on
[bblanchon-binaries][bblanchon] — see the `LICENSE` file in any
of their tarballs.

[bblanchon]: https://github.com/bblanchon/pdfium-binaries
