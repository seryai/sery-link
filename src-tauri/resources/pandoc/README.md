# pandoc runtime binary

The Tauri app bundles `pandoc` (or `pandoc.exe` on Windows) into
`<App>.app/Contents/Resources/pandoc/` on macOS, or
`<binary-dir>/resources/pandoc/` on Linux/Windows. The bundled
binary is loaded at runtime by mdkit's `PandocExtractor::with_binary`
when system PATH discovery fails — see `src-tauri/src/scanner.rs`
for the discovery logic.

## What's committed here

Just this README. The `pandoc` binary itself is **180 MB** —
GitHub's per-file limit is 100 MB, so we can't commit it even if
we wanted to. `.gitignore` excludes `pandoc` and `pandoc.exe` in
this directory; CI fetches them per-target before building.

## First-clone setup

After `git clone`, devs must run the fetch script once before
`pnpm tauri dev` will find the bundled pandoc:

```bash
./scripts/fetch-pandoc.sh
```

The script auto-detects your platform/arch and pulls the right
binary from [jgm/pandoc][jgm-pandoc] (currently pinned to
`3.9.0.2`). Re-runs are no-ops once the binary exists.

If pandoc isn't present, mdkit's `PandocExtractor` falls through
to system PATH discovery; if that ALSO fails, document extraction
hits the `anytomd` safety net (lower fidelity than Pandoc).

## Building for other platforms

The release CI (`.github/workflows/release.yml`) overwrites this
file with the per-target binary just before `tauri-action` runs.
Locally, you can do the same:

```bash
TARGET=linux-amd64    scripts/fetch-pandoc.sh --force
TARGET=windows-x86_64 scripts/fetch-pandoc.sh --force
TARGET=x86_64-macOS   scripts/fetch-pandoc.sh --force
TARGET=arm64-macOS    scripts/fetch-pandoc.sh --force
```

Valid `TARGET` values: `arm64-macOS`, `x86_64-macOS`,
`linux-amd64`, `linux-arm64`, `windows-x86_64`.

The script clears any pre-existing `pandoc` / `pandoc.exe` from
this directory before extracting, so flipping between targets
doesn't leave stale binaries in the bundle.

## Why bundle a 180 MB binary?

mdkit's `PandocExtractor` handles DOCX / PPTX / EPUB / RTF / ODT /
LaTeX. Pandoc is the gold-standard converter for these formats —
the alternative would be falling through to the `anytomd` Rust
crate, which has materially lower fidelity for DOCX in particular
(table layout, embedded media, footnote handling). The
correctness-vs-size tradeoff was made deliberately: a
production-bound app is ~225 MB instead of ~75 MB, but DOCX
extractions match what a desktop user would get from
`pandoc input.docx -o output.md` directly.

For comparison: the original Python markitdown sidecar was 179 MB
per platform on its own. Pandoc + libpdfium + mdkit/scankit
together are ~180 + ~7 + ~10 = ~197 MB — slightly more than the
v0.x sidecar bundle, but with in-process Rust speed (no fork
cost), parallel extraction, no Python supply-chain surface, and
the option to drop pandoc later if the size becomes a real issue.

## Bumping the pandoc version

Edit `PANDOC_VERSION` in `scripts/fetch-pandoc.sh`. Releases at
[jgm/pandoc/releases][jgm-releases].

After bumping, refetch for your dev target and re-test DOCX /
PPTX extraction end-to-end before merging.

## License

Pandoc is GPL-2.0-or-later (the binary is GPL'd; the formats it
emits — markdown, HTML, etc. — are not). Bundling pandoc inside a
proprietary or non-GPL Tauri app is fine because the bundling
mechanism is "shipping a separate binary as data," not static
linking — same model Pandoc itself recommends in its FAQ.
Distributing the binary alongside the app does mean the binary's
GPL terms apply to that copy, which means recipients can extract
and redistribute it under GPL — the bundled `pandoc` is not
encumbered by the host app's license.

[jgm-pandoc]: https://github.com/jgm/pandoc
[jgm-releases]: https://github.com/jgm/pandoc/releases
