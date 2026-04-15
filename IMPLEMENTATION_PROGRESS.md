# Obsidian-Inspired Improvements - Implementation Progress

Tracking implementation of the roadmap from `OBSIDIAN_INSPIRED_IMPROVEMENTS.md`.

---

## ✅ Phase 1: Immediate (1-2 weeks) — COMPLETED

**Goal:** Refresh positioning and clarify data ownership

### Marketing & Messaging
- ✅ Updated README.md tagline: "Local-first data analytics. Your data never leaves your machine."
- ✅ Emphasized tunnel mode (zero upload) vs. optional cloud sync
- ✅ Strengthened privacy-first messaging in README

### Data Ownership Clarity in UI
- ✅ Changed "Add Folder" button → "Watch Folder"
- ✅ Added tooltips: "Read-only access — your files are never modified"
- ✅ Updated app tagline: "Local analytics bridge" → "Local-first data analytics"
- ✅ Updated empty state copy to emphasize read-only access

### Keyboard Shortcuts Guide
- ✅ Created KeyboardShortcuts component (press `?` to show)
- ✅ Displays all navigation, actions, and UI shortcuts
- ✅ Clean modal design with sections and keyboard key visualization
- ✅ Integrated into App.tsx as global overlay

**Commits:**
- `56e6761` - feat: Phase 1 Obsidian-inspired improvements - local-first positioning

---

## 🚧 Phase 2: Next Month — IN PROGRESS

**Goal:** Keyboard-first UX and local metadata foundation

### Command Palette ✅ DONE
- ✅ Created CommandPalette component
- ✅ Keyboard shortcut: Cmd+K (Mac) / Ctrl+K (Windows/Linux)
- ✅ Fuzzy search across all available commands
- ✅ Arrow key navigation, Enter to execute
- ✅ Organized sections: Navigation, Folders, Actions, Recent
- ✅ Dynamic command list based on watched folders
- ✅ Per-folder actions (Rescan, Remove) for each watched folder

**Commits:**
- `85e74be` - feat: Phase 2 - implement Command Palette (Cmd+K)

### Local Metadata Cache ⏳ TODO
**Status:** Not started
**Effort:** ~2-3 days (Rust + DuckDB integration)
**Plan:**
- Create `LocalMetadataIndex` struct in Rust
- Use DuckDB to store dataset metadata locally
- Enable offline fuzzy search
- Sync from backend on metadata updates
- Load from local cache on app startup

**Files to create:**
- `src-tauri/src/metadata_cache.rs` - DuckDB-based local index
- `src-tauri/Cargo.toml` - Add `duckdb` crate dependency

### Dataset Relationship Graph ⏳ TODO
**Status:** Not started
**Effort:** ~3-4 days (data analysis + visualization)
**Plan:**
- Analyze JOIN patterns from query history
- Detect foreign key relationships by scanning column names
- Create React component with graph visualization (use react-flow or similar)
- Add "Show Relationships" button to dataset cards

**Files to create:**
- `src/components/RelationshipGraph.tsx` - Graph visualization
- `src-tauri/src/relationship_detector.rs` - FK detection logic

### Quick Actions Menu ⏳ TODO
**Status:** Not started
**Effort:** ~1 day
**Plan:**
- Add "..." dropdown menu to each dataset card in FolderList
- Actions: "Copy path", "Reveal in Finder", "View schema", "Query in dashboard"
- Use existing MoreVertical icon pattern from FolderCard

**Files to modify:**
- `src/components/FolderList.tsx` - Add dropdown to dataset cards (if we add dataset cards)

---

## 📋 Phase 3: Quarter 2 — NOT STARTED

**Goal:** Extensibility and community features

### MCP Plugin System ⏳ TODO
- Define MCP manifest schema
- Create plugin discovery UI
- Implement sandboxed plugin execution
- Add community plugin marketplace

### Export/Import Metadata ⏳ TODO
- JSON export of all watched folders + metadata
- Import to restore configuration
- Backup/restore workflow

---

## 🎯 Phase 4: Future — NOT STARTED

**Goal:** Advanced workflow automation

### Local-First Query History ⏳ TODO
- Store query history in local SQLite
- Offline access to past queries
- Export query history

### Pricing Model Revision ⏳ TODO
**Recommendation:** Free forever core + paid cloud
- FREE: Unlimited local datasets, tunnel mode, desktop agent
- PRO ($10/mo): Cloud sync, 100GB storage, performance mode
- TEAM ($20/user/mo): Team workspaces, shared datasets

---

## Implementation Statistics

- **Total Tasks:** 16
- **Completed:** 6 (37.5%)
- **In Progress:** 1 (6.25%)
- **Not Started:** 9 (56.25%)

**Phase Breakdown:**
- Phase 1: 100% complete ✅
- Phase 2: 25% complete (1/4 features) 🚧
- Phase 3: 0% complete
- Phase 4: 0% complete

---

## Next Steps (Priority Order)

1. **Local Metadata Cache** (Phase 2) - Foundation for offline-first experience
2. **Dataset Relationship Graph** (Phase 2) - Visual discovery of data connections
3. **Quick Actions Menu** (Phase 2) - Complete the keyboard-first UX
4. **MCP Plugin System** (Phase 3) - Enable community extensibility

---

## Development Notes

### Lessons Learned

**Phase 1:**
- Marketing messaging changes had zero code complexity but high impact
- Tooltip additions clarified data ownership without intrusive UI changes
- Keyboard shortcuts overlay was well-received (press `?` pattern from Obsidian)

**Phase 2:**
- Command palette implementation was straightforward with React hooks
- Fuzzy search could be improved with better scoring algorithm (TODO)
- Dynamic command generation based on config worked well
- Need to add Recent/MRU tracking for better UX

### Technical Debt

- Command palette currently doesn't integrate with local metadata cache (not built yet)
- Add folder action from command palette doesn't trigger FolderList picker (need event system)
- Missing tests for keyboard shortcuts and command palette
- No analytics tracking for command palette usage (consider adding)

---

Last updated: 2024-01-XX (Phase 2 Command Palette shipped)
