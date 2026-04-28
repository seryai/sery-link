# Sery Link — Project Handbook

**Version:** v0.4.0 (Three-Tier Strategy) · **Platform:** macOS / Windows / Linux (Tauri 2.0) · **Last consolidated:** 2026-04-16

The single reference for building, running, testing, and evolving Sery Link. Supersedes BACKEND_API_SPEC, DELIVERABLES_SUMMARY, DEVELOPER_QUICKSTART, IMPLEMENTATION_* (×4), LAUNCH_SUMMARY, NAVIGATION_BEFORE_AFTER, OBSIDIAN_INSPIRED_IMPROVEMENTS, RELEASE_CHECKLIST_v0.4.0, ROUTING_IMPLEMENTATION, RUNBOOK, SHIP_READINESS, SIDECAR_IMPLEMENTATION, STARTUP_CHECKLIST, STRATEGIC_ROADMAP, TESTING_GUIDE, TESTING_v0.4.0, USER_GUIDE, UX_RESTRUCTURE_SUMMARY (all archived under `archive/2026-04/`).

Live docs: [README.md](./README.md) · [CHANGELOG.md](./CHANGELOG.md) · parent project handbook at [../PROJECT.md](../PROJECT.md).

---

## 1. What Sery Link Is

**Sery Link is a data node in your personal fleet.**

You install one copy per machine you want to monitor (home PC, office laptop, server, NAS). Each copy watches folders, indexes local files into DuckDB, and publishes *metadata only* (schema, sample rows, column descriptions, machine identity) to the Sery cloud. When you ask a natural-language question from the web dashboard, the backend routes SQL back to the right agent(s); each agent runs the query locally and streams results home. **Raw files never leave the machine.**

**One user → many Sery Link installs → one workspace.** Multiple machines bound to the same workspace key appear as one fleet.

**North Star:** *"Download to Insight in 60 Seconds"* — from install on the first machine to the first answer. Adding a second machine should be another 60 seconds.

**Positioning:** Sery Link is the sensor, not the brain. The web dashboard is the primary conversation surface (plus future mobile / email / Slack). Sery Link's job is to keep each machine's data indexed, queryable, and reachable.

See the canonical 7-step end-to-end flow in [../PROJECT.md §4.0](../PROJECT.md).

---

## 2. The Three Tiers

### Tier 1 — Local Vault (FREE, zero login)
- No account, no signup, no tracking
- 5 FREE SQL recipes (Shopify churn/top products, Stripe MRR, GA traffic sources, CSV time-series)
- Local DuckDB queries, file watching, query history
- Works completely offline for SQL

**Not included:** PRO recipes, AI natural-language queries, cloud sync, team features.

### Tier 2 — BYOK (Bring Your Own Key)
Everything from Local Vault, plus:
- All 9 recipes (5 FREE + 4 PRO: Customer LTV, Cohort retention, Market basket, Funnel analysis)
- AI-powered natural-language queries (direct Anthropic / OpenAI API calls — zero Sery backend)
- User pays Anthropic directly (~$0.10–$1 per 100 queries)

**Not included:** Cloud sync across devices, team features, Performance Mode.

### Tier 3 — Workspace (FULL)
Everything from BYOK, plus:
- Cloud sync across devices (via `sery.ai` workspace key)
- Team sharing + role-based access
- Performance Mode (cloud-accelerated queries for large datasets)
- Managed AI credits (no API key management)
- Priority support

See [../PROJECT.md §5](../PROJECT.md) for the full tier philosophy. Workspace keys (`sery_k_…`) also enable hub-and-spoke collaboration (owner connects N members' agents).

---

## 3. Architecture

### Stack
- **Frontend:** React 19 + TypeScript + Tailwind CSS + Vite, Zustand stores
- **Backend:** Rust (Tauri 2.0), `tokio-tungstenite` WebSocket
- **Folder walking:** [`scankit`](https://crates.io/crates/scankit) — `walkdir` + size cap + exclude globs in one in-process Scanner
- **Tabular extraction:** [`tabkit`](https://crates.io/crates/tabkit) — Parquet / CSV / XLSX / XLS schema + sample rows + row count, in-process. DuckDB stays as the fallback for formats tabkit doesn't claim.
- **Document → markdown:** [`mdkit`](https://crates.io/crates/mdkit) — bundled libpdfium for PDF, pandoc subprocess for DOCX/PPTX/EPUB/RTF/ODT/LaTeX, anytomd fallback. Fully in-process Rust; no Python interpreter.
- **File watcher:** `notify` crate (debounced 1s)
- **Plugin Runtime:** WebAssembly (wasmer 7.1.0), sandboxed
- **Storage:** OS-native credential manager (Keychain / Credential Manager / Secret Service), local DuckDB scan cache (`~/.sery/scan_cache.db`), JSONL for query history
- **Packaging:** `.dmg` (mac), `.msi` (win), `.deb` + `.AppImage` (linux)

### Components (src-tauri/src/)

```
auth.rs                 OAuth loopback + auth mode detection (LocalOnly / BYOK / WorkspaceKey)
commands.rs             All Tauri RPC commands (add_folder, rescan, execute_recipe, etc.)
config.rs               Config schema + persistence (~/.seryai/config.json)
scanner.rs              Two-pass scan: scankit walk → mdkit/tabkit extract (PDFs serial, others parallel)
watcher.rs              notify-based file watcher (debounced 1s)
duckdb_engine.rs        Local query execution
websocket.rs            WebSocket client for cloud tunnel
keyring_store.rs        OS keychain wrapper for tokens
tray.rs                 macOS/Windows system tray
recipe_executor.rs      SQL recipe loader, parameter validation, template rendering
plugin.rs               WebAssembly plugin lifecycle
plugin_runtime.rs       wasmer host functions (read-files, clipboard, network)
plugin_marketplace.rs   Search / filter / install (6 Tauri commands)
relationship_detector.rs  Schema + query-based dataset relationship inference
metadata_cache.rs       Local SQLite for offline search
history.rs              Query history JSONL persistence
csv.rs / excel.rs       Format-specific helpers
events.rs               Tauri event bus
export_import.rs        Config backup/restore
audit.rs                Sync audit log
stats.rs                Usage statistics
```

### System Diagram

```
┌──────────────────────────────────────────────────────────────────────┐
│  Sery Link (Tauri Desktop App)                                       │
│                                                                      │
│  ┌──────────────┐         ┌────────────────────────────────────┐     │
│  │ File Watcher │────────▶│  scanner.rs (two-pass)             │     │
│  │ (notify)     │  event  │                                    │     │
│  └──────────────┘         │  Pass 1: scankit::Scanner.walk()   │     │
│                           │   → emit shallow DatasetMetadata   │     │
│                           │                                    │     │
│                           │  Pass 2: extract_one() per file    │     │
│                           │   ├─ PDF   → mdkit (libpdfium)     │     │
│                           │   │          serial thread         │     │
│                           │   ├─ DOCX  → mdkit (pandoc subproc)│     │
│                           │   ├─ HTML  → mdkit (html2md)       │     │
│                           │   └─ tabular → tabkit (in-process) │     │
│                           │     ↑ both run on rayon pool ×N    │     │
│                           └────────────────────────────────────┘     │
└──────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼ WebSocket (TLS)
                          ┌──────────────────────────┐
                          │   Sery Backend           │
                          │   (api/ - FastAPI)       │
                          └──────────────────────────┘
```

### Auth Mode Detection (auth.rs)

```rust
pub fn get_auth_mode() -> AuthMode {
    if let Ok(key) = env::var("WORKSPACE_KEY") {
        return AuthMode::WorkspaceKey(key);
    }
    if let Ok(api_key) = env::var("ANTHROPIC_API_KEY") {
        return AuthMode::BYOK { provider: "anthropic", api_key };
    }
    if let Ok(api_key) = env::var("OPENAI_API_KEY") {
        return AuthMode::BYOK { provider: "openai", api_key };
    }
    AuthMode::LocalOnly
}
```

Feature gating uses `useFeatureGate('pro_recipes')` hook on the frontend which calls `check_feature_available` on Rust side.

---

## 4. Supported File Types

**Tabular (tabkit fast path, DuckDB fallback):**
- Parquet (`.parquet`, `.pq`) — best performance
- CSV (`.csv`, `.tsv`) — auto-detect delimiters
- Excel (`.xlsx`, `.xls`) — via calamine, reads all sheets

**Documents (mdkit → Markdown):**
- Word (`.docx`), PowerPoint (`.pptx`) — pandoc subprocess (bundled binary)
- PDF (`.pdf`) — libpdfium text layer; Apple Vision / Windows.Media.Ocr fallback for scanned pages
- HTML (`.html`, `.htm`) — html2md
- Jupyter (`.ipynb`) — serde_json
- mdkit also handles EPUB / RTF / ODT / LaTeX via pandoc when the user adds them to a watched folder

Scanner falls back to the Rust-native `anytomd` crate if mdkit returns an error for a given format.

---

## 5. Build & Run

### Prerequisites

```bash
node --version        # 18+ (22 LTS recommended)
pnpm --version        # 9+
cargo --version       # 1.88+
```

No Python required — mdkit replaced the old PyInstaller sidecar.

### First-time setup

```bash
pnpm install

# Fetch bundled runtime binaries (libpdfium ~7 MB, pandoc ~180 MB).
# Pandoc is gitignored because it exceeds GitHub's per-file 100 MB cap;
# devs run this once per clone, CI runs it as a release-workflow step.
./scripts/fetch-libpdfium.sh
./scripts/fetch-pandoc.sh
```

### Development

```bash
pnpm tauri dev       # Vite dev server + Rust build + Tauri window (first run ~2-3 min)
```

Hot reload: frontend `.tsx/.ts/.css` auto-reload · Rust `.rs` rebuilds (~5-15s) · both ~15-30s.

### Production Builds

**macOS Universal (Intel + Apple Silicon):**
```bash
pnpm tauri build --target universal-apple-darwin
# → src-tauri/target/universal-apple-darwin/release/bundle/dmg/Sery Link_<v>_universal.dmg
```

**macOS ARM-only / Intel-only:**
```bash
pnpm tauri build --target aarch64-apple-darwin    # or x86_64-apple-darwin
```

**Windows:**
```bash
# Pre-req: WebView2 runtime installed
./scripts/fetch-libpdfium.sh    # fetches per-target libpdfium.dll
./scripts/fetch-pandoc.sh       # fetches pandoc.exe for the build target
pnpm tauri build
# → src-tauri/target/release/bundle/msi/Sery Link_<v>_x64_en-US.msi
```

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
./scripts/fetch-libpdfium.sh
./scripts/fetch-pandoc.sh
pnpm tauri build
# → src-tauri/target/release/bundle/deb/sery-link_<v>_amd64.deb
# → src-tauri/target/release/bundle/appimage/sery-link_<v>_amd64.AppImage
```

### Installed size
~12 MB main binary (mdkit + tabkit + scankit compiled in) + ~7 MB libpdfium + ~180 MB pandoc = ~200 MB installed. Compressed installers ~60–80 MB. Pandoc dominates; mdkit's `pandoc` feature is the only reason we ship it (DOCX / PPTX / EPUB / RTF / ODT / LaTeX → markdown). Without pandoc the whole bundle drops to ~25 MB at the cost of those formats falling back to anytomd's lower-fidelity output.

### Env vars (dev)
```bash
export RUST_LOG=debug                 # Verbose logs
export ANTHROPIC_API_KEY=sk-ant-xxx   # Trigger BYOK mode
export SERY_CONFIG_DIR=/custom/path   # Override config location
export API_URL=http://localhost:8000  # Backend URL (prod: https://api.sery.ai)
```

---

## 6. Kit-family Architecture (mdkit / tabkit / scankit)

Three sibling crates were extracted from sery-link's scanner during the v0.5.0 cycle and published to crates.io. They replaced the old MarkItDown Python sidecar (179 MB PyInstaller bundle, slow subprocess spawn, missing-deps headaches) with in-process Rust.

### Why kits
1. **No Python interpreter shipping with the app.** Users don't install Python; we don't ship one. Bundle drops from ~200 MB sidecar to ~12 MB compiled Rust + ~180 MB pandoc binary that's optional.
2. **No subprocess spawn per file.** Each conversion is an in-process function call. Pass-2 parallelism becomes meaningful — see scanner.rs comments for the libpdfium serialisation gotcha.
3. **Reusable across Tauri / Iced / native desktop apps.** Anyone building "index files on the user's machine" can pull these in.
4. **Stable APIs documented per crate.** `#[non_exhaustive]` on every public type so we can grow them without major-version churn.

### The three kits

| Crate | Job | Used by sery-link for |
|---|---|---|
| [`scankit`](https://crates.io/crates/scankit) | Walk + filter + emit ScanEntry | Pass 1 of the scan (the "what files exist" iterator) |
| [`tabkit`](https://crates.io/crates/tabkit) | Tabular file → schema + sample rows + row count | `extract_schema()` fast path; DuckDB stays as the fallback |
| [`mdkit`](https://crates.io/crates/mdkit) | Document file → markdown | `extract_document_markdown()`; PDF/DOCX/PPTX/HTML/IPYNB |

### Bundled runtime binaries

mdkit's PDF backend depends on libpdfium (C++ library, ~7 MB per platform). The pandoc backend depends on the `pandoc` binary (~45 MB compressed, ~180 MB on disk per platform).

`tauri.conf.json` `bundle.resources` ships:
```
src-tauri/resources/libpdfium/libpdfium.{dylib,so,dll}
src-tauri/resources/pandoc/pandoc{,.exe}
```

`scripts/fetch-libpdfium.sh` and `scripts/fetch-pandoc.sh` fetch the per-target binary into those folders. The pandoc binary is gitignored (per-file 100 MB cap on GitHub); CI runs the fetch script as a release-workflow step. libpdfium is small enough to commit for the dev macOS arm64 path; CI fetches per-target on release builds.

`scanner.rs` resolves these at runtime via `bundled_resource_dir()` → `MDKIT_ENGINE` constructs `PdfiumExtractor::with_library_path(...)` and `PandocExtractor::with_binary(...)` from those paths.

### Rust integration (scanner.rs)

```rust
static MDKIT_ENGINE: Lazy<mdkit::Engine> = Lazy::new(|| {
    let (mut engine, errors) = mdkit::Engine::with_defaults_diagnostic();
    // ... patch in bundled libpdfium + pandoc paths if system search failed ...
    engine
});

static TABKIT_ENGINE: Lazy<tabkit::Engine> = Lazy::new(tabkit::Engine::with_defaults);

fn extract_document_markdown(file_path: &Path, ext: &str) -> Option<String> {
    let bytes = fs::read(file_path).ok()?;
    match MDKIT_ENGINE.extract_bytes(&bytes, ext) {
        Ok(doc) => Some(doc.markdown),
        Err(_) => anytomd::convert_bytes(&bytes, ext, &Default::default())
            .ok().map(|r| r.markdown),
    }
}
```

### Conversion performance
| Format | Avg size | Time | Backend |
|---|---|---|---|
| DOCX | 50 KB | ~200–800 ms | pandoc subprocess |
| PPTX | 2 MB | ~500 ms–2 s | pandoc subprocess |
| HTML | 100 KB | ~10–50 ms | html2md (in-process) |
| PDF (text) | 1 MB | ~100–500 ms | libpdfium (in-process) |
| PDF (scanned) | 1 MB | ~2–10 s/page | Apple Vision / Windows.Media.Ocr |
| IPYNB | 500 KB | ~10 ms | serde_json (in-process) |
| Tabular (CSV / XLSX / Parquet) | varies | ~50–500 ms | tabkit (in-process) |

The dominant cost is now PDF text extraction; DOCX dropped from ~500 ms (sidecar spawn floor) to ~200 ms (pandoc spawn). HTML / IPYNB are essentially free.

### Pass 2 threading

libpdfium throws `PdfiumLibraryInternalError(FormatError)` on concurrent loads of *different* PDFs despite pdfium-render's `thread_safe` feature. `scan_folder_blocking` partitions the pass-2 work queue:

- **PDFs** → dedicated serial thread (1 file at a time)
- **Other formats** → rayon pool sized by `max_scan_workers()` (default `(num_cpus / 2).clamp(2, 8)`)

Both halves run **concurrently** via `std::thread::scope`. Wall time = `max(pdf_serial_time, other_parallel_time)`. The `SERY_SCAN_WORKERS` env var overrides the worker count if instability appears.

### Troubleshooting
- **"libpdfium not found":** run `./scripts/fetch-libpdfium.sh` (or install via Homebrew on macOS dev machines).
- **"pandoc binary not found":** run `./scripts/fetch-pandoc.sh`. mdkit will silently fall through to anytomd for DOCX/PPTX/EPUB without it.
- **PDF FormatError on parallel scans:** shouldn't happen since the partition lands in main; if it does, set `SERY_SCAN_WORKERS=1` and file an issue.

---

## 7. Plugin System (WebAssembly)

### Built-in plugins (production)
1. **CSV Parser** (2.4 KB) — parse, validate, row/column count
2. **JSON Transformer** (5.7 KB) — pretty-print, minify, validate
3. **HTML Viewer** (9.8 KB) — text extraction, tag counting, structure validation
4. **Clipboard Utility** (4.2 KB) — read, write, transform
5. **Text Analyzer** (9.9 KB) — readability, sentiment, statistics

Plugins are **no_std Rust → WASM** — tiny modules, sandboxed, startup <1ms with module caching.

### Capabilities (plugin manifest)
`data-source`, `viewer`, `transform`, `exporter`, `ui-component`

### Permissions
`read-files`, `execute-commands`, `network`, `clipboard` — all path-validated, permission-gated via host functions (FunctionEnvMut pattern).

### Marketplace (backend ready, UI pending)
6 Tauri commands (load, search, featured, popular, get, install). Install sources: GitHub releases, arbitrary HTTPS, local folders. Metrics: downloads, stars, ratings, reviews. **UI frontend slated for v0.2.0 completion.**

Plugin install path: `~/.sery/plugins/<plugin-id>/`.

---

## 8. SQL Recipe Marketplace (v0.3.0 — shipped)

### What a recipe is
Pre-built SQL templates for common analytics questions. Users fill in parameters (date range, thresholds) and run — no SQL knowledge required. 530 lines Rust executor (`recipe_executor.rs`) + 8 Tauri commands.

### Recipe schema
```json
{
  "id": "shopify-churn-rate",
  "name": "Calculate Monthly Churn Rate",
  "data_source": "Shopify",
  "tier": "FREE",
  "sql_template": "SELECT ... WHERE date > '{{start_date}}' ...",
  "parameters": [
    {"name": "start_date", "type": "date", "default": "30 days ago"},
    {"name": "min_orders", "type": "int", "default": 2}
  ],
  "author": "Sery Team",
  "rating": 4.8
}
```

Parameter types: `date`, `int`, `float`, `string`, `boolean`. SQL templating via `{{parameter}}` substitution with validation against detected schema.

### Shipped recipes (9)

**FREE (5):** Shopify churn rate, Shopify top products, Stripe MRR, GA traffic sources, CSV time-series aggregation.

**PRO (4):** Customer LTV with cohort analysis, Stripe cohort retention, Market basket (lift), Multi-step funnel.

Recipe JSON files live in `examples/recipes/`. They load automatically in dev mode.

### UI
- `RecipePanel` (480 lines) — browse, search, filter
- `RecipeExecutor` (380 lines) — parameter form + SQL preview + results table + CSV export

### Current v0.4.0 location
Recipes live under the **Analytics** primary tab (promoted from Settings). Context-aware suggestions detect data sources (Shopify, Stripe, GA) from folder paths and show 6 suggested recipes before the full library.

### Deferred (v0.3.0 punt list)
- DuckDB integration (executor returns mock data — needs wiring)
- Ratings/reviews submission (display works)
- Bookmarking / favorites
- Community recipe contributions (marketplace API)

---

## 9. UX Direction (v0.4.0 restructure)

### The workflow shift
From implicit "figure out what to do" to explicit **Data → Analysis → Results**.

### Navigation (v0.4.0)

```
Sidebar:
├── 📁 Folders        (INPUT — data sources)
├── ✨ Analytics      (ACTION — recipes + future query builder)
├── 📊 Results        (OUTPUT — was "History")
└── ⚙️  More ▼        (Settings + Privacy, dropdown)
```

### Key v0.4.0 changes
1. **New Analytics tab** — combines recipe library + context-aware suggestions. Auto-detects Shopify/Stripe/GA from folder paths, surfaces 6 relevant recipes before the full catalog.
2. **"Analyze This Folder" CTA** on each folder card (after scan completes) — navigates to Analytics with data-source filter pre-applied.
3. **History → Results** rename — emphasizes outcomes over chronology.
4. **Settings + Privacy → More dropdown** — 3 primary nav items instead of 4+.
5. **Recipes removed from Settings** — they're actions, not configuration. Settings now has 6 tabs (was 7).
6. **Command Palette additions** (Cmd+K) — "analytics", "results", "recipes" keywords all route correctly.

### Time-to-first-query
- Before: ~120s (hidden recipes, no CTA)
- After: ~20s (obvious path from folder to recipe)
- North Star: ≤60s from install

### Navigation comparison (old vs new)
Old: flat nav, all items equal weight, recipes buried 2 levels deep in Settings.
New: hierarchical — primary workflow (Folders → Analytics → Results) vs. secondary config (More).

### Current shipped UX (for a complete walkthrough)
See [UX_WALKTHROUGH.md](./UX_WALKTHROUGH.md) for the end-to-end
user-facing flow: first run, main shell, every tab, pair-a-machine
modal, daily loop, and UX design principles. That document reflects
shipped reality (updated 2026-04-18); this section captures the
strategic direction that got us there.

### Open UX direction (from independent review, April 2026)
See independent UX analysis in `archive/2026-04/` (OBSIDIAN_INSPIRED_IMPROVEMENTS, NAVIGATION_BEFORE_AFTER, UX_RESTRUCTURE_SUMMARY). Three strategic paths under consideration for next iteration:
- **Path A (Utility):** Menu-bar-first, web dashboard remains primary chat UI. Sery Link = data connector.
- **Path B (Workstation):** Unified chat+notebook in main window. Required if BYOK is to offer full conversational UX without web dashboard.
- **Path C (Hybrid):** Menu-bar Quick Ask (Raycast-style) + lightweight conversation window. Current recommended direction.

Known issues flagged for cleanup: `TEST NAV` debug button in `App.tsx:192-200` still shipped; "Watched folders" nomenclature is developer-vocabulary; 5-step onboarding contradicts "60 seconds" North Star and Tier 1 zero-login philosophy.

---

## 10. Testing

### Unit tests (Rust)
```bash
cd src-tauri
cargo test                                           # All
cargo test auth::tests::test_local_only_mode_defaults  # One
cargo test --lib                                     # Library only
```

Coverage: auth mode logic (9), config (10), recipe execution (3), plugins (8), and more.

### mdkit / tabkit smoke test
The kit-family crates have their own test suites; sery-link's scanner just composes them. To smoke-test the document path end-to-end:

```bash
# Drop a sample DOCX / PDF / XLSX into a watched folder and run dev.
pnpm tauri dev
# Click Rescan in FolderDetail; watch terminal for:
#   [scanner] pass 2 — N PDFs (serial …) + M others (parallel × W workers)
#   [scanner] ✅ mdkit converted "..."
#   [scanner] ✓ "..."
```

If a backend is misconfigured (e.g. libpdfium not on the library path), `MDKIT_ENGINE` logs the failed backend name on first init: `[scanner] mdkit: backend 'pdf' failed system search: ...`.

### E2E dev flow
1. `cargo test && cd .. && pnpm tauri dev`
2. Onboarding wizard → pick mode (Local Vault recommended for first run)
3. Add folder with Parquet/CSV/DOCX
4. Verify scan completes, dataset count shown
5. Click "Analyze This Folder" → Analytics opens with suggestions
6. Execute a FREE recipe → verify results + CSV export

### Clean-slate reset (dev)
```bash
# macOS
rm -rf ~/.seryai
security delete-generic-password -s "com.sery.link" 2>/dev/null || true
rm -rf node_modules/.vite src-tauri/target/debug
```

### Auth-mode debugging (browser DevTools)
```typescript
const mode = await invoke('get_current_auth_mode');
const available = await invoke('check_feature_available', { feature: 'pro_recipes' });
const config = await invoke('get_config');
```

### Feature-gate verification matrix
| Feature | LocalOnly | BYOK | WorkspaceKey |
|---|---|---|---|
| free_recipes | ✅ | ✅ | ✅ |
| pro_recipes | ❌ | ✅ | ✅ |
| ai_queries | ❌ | ✅ | ✅ |
| semantic_search | ❌ | ✅ | ✅ |
| cloud_sync | ❌ | ❌ | ✅ |
| performance_mode | ❌ | ❌ | ✅ |
| team_features | ❌ | ❌ | ✅ |

---

## 11. Backend API Contract

Sery Link talks to the backend (`api/`) via 5 HTTP endpoints + 1 WebSocket. Full schemas and Python reference implementation live in `api/app/api/v1/`. This section is a pointer, not a duplication.

### HTTP

| Endpoint | Purpose |
|---|---|
| `GET /agent/authorize` | Browser authorization page (user approves agent); 5-min Redis code |
| `POST /api/v1/agent/token` | Exchange auth code → JWT access token (1-year expiry for agents) |
| `GET /api/v1/agent/info` | Verify token, update `last_ping`, return agent info |
| `POST /api/v1/agent/sync-metadata` | Upsert dataset metadata (schema, row count, size, embedding) |
| `POST /v1/agent/auth/key` | Workspace-key auth (hub-and-spoke): `{key, display_name}` → JWT |

### WebSocket

`WS /api/v1/agent/tunnel?token=<jwt>` — persistent bidirectional.

**Message types:**
- `ping` / `pong` — 30s heartbeat
- `run_sql` (server → agent) — `{query_id, sql, file_path, timeout_seconds, max_rows}`
- `query_result` (agent → server) — `{query_id, columns, rows, row_count, execution_ms, truncated, total_rows}`
- `query_error` (agent → server) — `{query_id, error, error_code, suggestion}`
- `sync_schema` (server → agent) — request fresh schema for a dataset
- `schema_update` (agent → server) — updated schema + version + hash
- `invalidate_cache` (server → agent) — flush cached results

### Dev testing
```bash
# Log in to web app first to get session cookie
# Then test token exchange:
curl -X POST http://localhost:8000/api/v1/agent/token \
  -H "Content-Type: application/json" \
  -d '{"code":"test123"}'

# WebSocket (browser console):
const ws = new WebSocket('ws://localhost:8000/api/v1/agent/tunnel?token=eyJ...');
ws.onmessage = (e) => {
  const d = JSON.parse(e.data);
  if (d.type === 'ping') ws.send(JSON.stringify({type: 'pong', timestamp: d.timestamp}));
};
```

### Reconnection
Agent disconnect → backend marks offline after 30s → pending queries queue in Redis (`agent:<id>:pending`). Agent reconnects within 5 min → sends `{type: "reconnect", last_message_id}` → backend replays backlog. Beyond 5 min: user sees "Agent offline" with retry.

### Row limit (tunnel mode)
10,000 rows max per query. If exceeded: `truncated: true, total_rows: 45230, truncation_reason: "Tunnel mode row limit"`. Claude's response guides user to (1) enable Cloud Sync, (2) add filters, (3) download partial results.

---

## 12. Strategic Roadmap

### v0.2.0 — Marketplace UI ✅ SHIPPED
Frontend marketplace browser, plugin detail pages, one-click install, seed 5 plugins.
**Deferred:** HTTP/GitHub auto-download (still manual install).

### v0.3.0 — SQL Recipe Marketplace ✅ SHIPPED (January 2025)
9 recipes, parameter validation, tier gating, UI, CSV export.

### v0.4.0 — Three-Tier + UX Restructure ✅ SHIPPED (April 2026)
Local Vault mode, BYOK mode, feature gating across modes, Analytics tab, "Analyze This Folder" CTA, Results rename, More dropdown.

### v0.5.0 — Mobile / Windows / Linux parity [NEXT]
- Windows + Linux builds in CI (currently macOS primary; Linux exists)
- Mobile apps via Tauri (iOS + Android) — same WASM plugins, same recipes
- Mobile-optimized UI (bottom sheets, swipe, offline-first)
- **Rust BYOK agent loop** (port `api/app/services/agent/agent.py` to Rust) — unblocks offline AI
- Custom recipe builder (AI-assisted)
- In-app visualizations (charts)

### v0.6.0 — Self-Hosted Backend [ENTERPRISE]
Docker Compose for TEAM tier, same API contract as cloud, team recipe library, audit logs. `docker-compose up` → working Sery backend <10 min.

### Pricing

| Tier | Price | What's included |
|---|---|---|
| **FREE** | $0 | Unlimited folders, tunnel mode, 5 FREE recipes, basic metadata sync, 5 built-in plugins, local query history (last 1000) |
| **PRO** | $15/mo | Everything FREE + all 9 recipes + cloud query execution + advanced relationship graph + priority support + community plugins + Excel export |
| **TEAM** | $50/mo (up to 10 users) | Everything PRO + shared folders + collaborative history + team recipe library + RBAC + audit logs + SSO (Google, Okta) |

Note: Parent project (Sery.ai) uses a different tier structure ([../PROJECT.md §8](../PROJECT.md)). Sery Link tiers align operationally.

### Success targets (12-month)
- 10,000 active users (FREE)
- 500 PRO subscribers → $7,500 MRR
- 20 TEAM accounts → $10,000 MRR
- **Total: $17,500 MRR**
- 100 SQL recipes (50 FREE, 50 PRO)
- 25 community plugins
- 1st enterprise deal ($50k ARR)

---

## 13. Release Process

### Pre-release checklist
- [ ] Update version in `src-tauri/tauri.conf.json` + `package.json`
- [ ] Fetch bundled binaries: `./scripts/fetch-libpdfium.sh && ./scripts/fetch-pandoc.sh`
- [ ] Test document conversion: DOCX, PPTX, HTML, PDF, IPYNB
- [ ] Test OAuth flow end-to-end
- [ ] Test WebSocket tunnel connection + reconnect
- [ ] Test file watcher detects changes
- [ ] `cargo test` — all pass
- [ ] `pnpm build` — no TS errors
- [ ] Production build: `pnpm tauri build`
- [ ] Install on clean machine — verify onboarding + first query < 60s
- [ ] Check libpdfium + pandoc bundled (inspect `.app` / `.exe` / AppImage contents)
- [ ] Update CHANGELOG.md
- [ ] `git tag -a v0.x.0 -m "Release …" && git push origin v0.x.0`

### CI (GitHub Actions)
Build on push to `main` / `feat/*`. Matrix: macos-latest (arm64 + x86_64), windows-latest, ubuntu-latest. Steps:
1. Checkout, setup Node 22, setup Rust stable, install pnpm
2. `pnpm install`
3. `./scripts/fetch-libpdfium.sh <pdfium_target>` and `./scripts/fetch-pandoc.sh <pandoc_target>`
4. `pnpm tauri build` (universal for macOS, native for win/linux)
5. Upload artifacts (DMG / MSI / deb / AppImage)

### Code signing (production)
- macOS: sign + notarize `.app` (Apple Developer cert)
- Windows: sign `.msi` with code-signing certificate
- Linux: usually unsigned (AppImage) or distro-managed (.deb)

---

## 14. Runbook & Troubleshooting

### Log locations
- macOS: `~/Library/Logs/ai.sery.link/`
- Windows: `%APPDATA%\ai.sery.link\logs\`
- Linux: `~/.local/share/ai.sery.link/logs/`

### Config / state
- Config: `~/.seryai/config.json`
- Metadata cache: `~/.seryai/metadata.db` (SQLite)
- Query history: `~/.seryai/history.jsonl`
- Plugins: `~/.sery/plugins/<id>/`

### Common issues

| Symptom | Likely cause | Fix |
|---|---|---|
| "Sery Link cannot be opened" (macOS) | Gatekeeper blocking unsigned app | Settings → Privacy → Open Anyway |
| Onboarding stuck on "You're all set" | Pre-v0.4.0 bug | Force quit, reinstall latest |
| Generic icon in Login Items | LaunchAgent not associated | Settings → Login Items → remove + re-toggle |
| Folder scan stuck at 0% | Huge folder OR permission denied OR network drive | Wait 2-3min OR check perms OR copy locally |
| Recipe shows "requires PRO" | Local Vault mode, PRO recipe | Upgrade to BYOK or Workspace |
| "Out of memory" error | DuckDB exceeds RAM | Settings → Advanced → increase memory; add filters; use Parquet over CSV |
| App slow / unresponsive | Large query OR scan OR too many folders | Wait for current op; reduce watched folders; exclude `node_modules` |
| WebSocket won't connect | Backend down OR firewall OR expired token | `curl /health`; allow in firewall; re-auth in Settings |
| Rust compilation fails (Linux) | Missing `libwebkit2gtk-4.1-dev` | Install system deps (§5 Linux) |
| mdkit "backend `pdf` failed system search" | libpdfium not installed | `./scripts/fetch-libpdfium.sh` (or `brew install pdfium-binaries` on macOS) |
| mdkit DOCX falls through to anytomd (low fidelity) | pandoc not on PATH or not bundled | `./scripts/fetch-pandoc.sh` |
| "Tauri build: resource path doesn't exist" | libpdfium / pandoc missing in `src-tauri/resources/` | Run the fetch scripts before `pnpm tauri build` |

### Debugging checklist

```bash
# 1. Verbose logs
RUST_LOG=debug pnpm tauri dev
# Look for: "[scanner] pass 1 — walking …", "[scanner] ▶ <file> tier=Content",
#           "[scanner] ✅ mdkit converted …", "[scanner] mdkit: backend X failed …"

# 2. Check keychain
security find-generic-password -s "com.sery.link" -a "access_token"

# 3. Inspect scan cache
duckdb ~/.sery/scan_cache.db "SELECT COUNT(*) FROM scan_cache;"

# 4. Test mdkit directly (cargo example from the mdkit repo)
cargo run --manifest-path ../mdkit/Cargo.toml --example convert -- /path/to/problem.docx

# 5. Check OAuth callback
# Should see: "OAuth callback server listening on 127.0.0.1:7777"
# If blocked: check macOS firewall
```

### Performance tips (development)
- Frontend-only changes → Vite hot reload (instant)
- Rust changes → Tauri rebuild (~5-15s)
- Install `sccache`: `cargo install sccache && export RUSTC_WRAPPER=sccache`
- Use `cargo check` instead of `cargo build` when iterating

---

## 15. File Structure Reference

```
sery-link/
├── src/                          # React frontend
│   ├── App.tsx                   # Main shell + routing + auth gate
│   ├── components/
│   │   ├── OnboardingWizard.tsx  # First-run 5-step wizard
│   │   ├── FolderList.tsx        # Watched folders + cards + "Analyze" CTA
│   │   ├── Analytics.tsx         # Recipe suggestions + full library
│   │   ├── RecipePanel.tsx       # Recipe browse (480 lines)
│   │   ├── RecipeExecutor.tsx    # Parameter form + execution (380 lines)
│   │   ├── History.tsx           # Query history ("Results" in UI)
│   │   ├── CommandPalette.tsx    # Cmd+K fuzzy search
│   │   ├── Settings.tsx          # 6 tabs: General, Sync, App, Plugins, Marketplace, About
│   │   ├── Privacy.tsx           # Sync audit log
│   │   ├── PluginsPanel.tsx
│   │   ├── MarketplacePanel.tsx
│   │   ├── RelationshipGraph.tsx
│   │   ├── StatusBar.tsx, Toast.tsx, ReAuthModal.tsx, UpgradePrompt.tsx,
│   │   │   KeyboardShortcuts.tsx, FolderDetailModal.tsx
│   ├── hooks/
│   │   ├── useAgentEvents.ts     # Tauri event subscriptions
│   │   ├── useFeatureGate.ts     # Feature availability
│   │   ├── useMetadataCache.ts
│   │   └── useTheme.ts
│   ├── stores/                   # Zustand stores
│   └── types/events.ts
│
├── src-tauri/
│   ├── src/                      # Rust backend (see §3 component list)
│   ├── resources/                # Bundled runtime binaries
│   │   ├── libpdfium/            # libpdfium.{dylib,so,dll} per platform
│   │   └── pandoc/               # pandoc binary per platform (gitignored, fetched)
│   ├── tauri.conf.json           # bundle.resources, window config, permissions
│   └── Cargo.toml
│
├── scripts/
│   ├── fetch-libpdfium.sh        # per-target libpdfium fetcher
│   └── fetch-pandoc.sh           # per-target pandoc fetcher
│
├── examples/
│   └── recipes/                  # 9 recipe JSON files (5 FREE, 4 PRO)
│
├── marketplace.json              # Plugin marketplace seed
├── recipe-schema.json            # Recipe JSON schema (validation)
│
├── README.md                     # User-facing README (live)
├── CHANGELOG.md                  # Version history (live)
├── PROJECT.md                    # This file (live)
└── archive/2026-04/              # Archived historical docs
```

---

## 16. Metrics to Watch

### Leading (user behavior)
- Command Palette usage rate
- Recipe search CTR
- Plugin install rate
- Query repeat rate
- "Analyze This Folder" button CTR (target >40%)
- Analytics tab engagement (target >70%)

### Lagging (revenue)
- FREE → PRO conversion (target 5% in 30 days)
- PRO month-1 retention
- TEAM seat expansion rate
- Net revenue retention (target 120%+)

### Vanity (don't optimize for)
- Total signups (meaningless if no query run)
- Total plugins downloaded (meaningless if unused)
- Total recipes viewed (meaningless if not executed)

### Technical
- Time to first query (target <60s from install)
- Scan cache hit rate (target 30%)
- mdkit conversion success rate per format
- WebSocket reconnect rate

---

## 17. Open Questions

### Product
- Recipe authorship: curate seed set first, or open community contributions day 1?
- Recipe versioning: auto-update saved recipes when SQL template changes?
- Multi-dataset recipes: support JOINs across Shopify + Stripe?

### Business
- Recipe marketplace revenue share model (App Store 70/30?)
- TEAM trial length (14 days all features) vs. self-serve only?
- PRO pricing A/B test ($10 vs $15 vs $20)?

### Technical
- Recipe sandboxing: run in WASM like plugins?
- Recipe result caching (with TTL)?
- Mobile offline recipe caching strategy?
- Bump pass-2 worker count higher than `(cores / 2).clamp(2, 8)` if memory headroom allows?

---

## 18. Getting Help

- **Support:** support@sery.ai
- **Issues:** https://github.com/seryai/sery-link/issues
- **Docs:** https://sery.ai/docs
- **Release:** https://github.com/seryai/sery-link/releases

---

**Maintainers:** Sery.ai Engineering Team · **Review cadence:** per minor release · **Canonical strategy:** [../PROJECT.md](../PROJECT.md).
