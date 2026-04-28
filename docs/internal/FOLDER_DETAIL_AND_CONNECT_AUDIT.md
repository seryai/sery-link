# sery-link FolderDetail + ConnectModal audit

Two surfaces in the desktop app:

- `FolderDetail.tsx` — reached by clicking a folder card or opening a
  search result. High-traffic once a user is paired.
- `ConnectModal.tsx` — pasted-key path from StatusBar "Connect"
  button. Second-machine and first-time pair flow.

Audited 2026-04-23.

**Severity**: 🔴 blocker · 🟡 friction · 🟢 polish

---

## FolderDetail findings

### 🟢 F1 — PDF support — RESOLVED in v0.5.0

PDF was added to `DOCUMENT_EXTENSIONS` in scanner.rs and `DOCUMENT_FORMATS` in FolderDetail.tsx during the v0.5.0 cycle. PDF text-layer extraction now goes through mdkit's libpdfium backend (in-process Rust); scanned / image-only PDFs fall through to Apple Vision (macOS) or Windows.Media.Ocr via mdkit's `ocr-platform` feature. README + LAUNCH_ASSETS PDF claims are now accurate.

Original audit text preserved in git history (audit dated 2026-04-23, fix landed 2026-04-27).

---

### 🟡 F2 — "Open in Finder" button is macOS-specific

**Where**: `FolderDetail.tsx:293`

```tsx
<button onClick={revealFolder}>
  <SquareArrowOutUpRight ...>
  Open in Finder
</button>
```

**Problem**: Windows users see "Open in Finder" — Finder doesn't
exist on Windows. Linux users see the same — their file manager
could be Files, Dolphin, Nautilus, Thunar, or whatever.

**Fix**: Platform-aware label:

```tsx
const openLabel = {
  macOS: 'Open in Finder',
  Windows: 'Open in Explorer',
  Linux: 'Show in file manager',
}[detectPlatform()] ?? 'Open folder';
```

Or just "Open folder" — portable, vague, always correct. Loses the
macOS "Finder" specificity but sidesteps the cross-platform copy
problem.

The underlying Tauri command (`reveal_in_finder`) can keep its
current name — it's an internal identifier, not user-visible.

---

### 🟡 F3 — "No indexable files" empty state is terse

**Where**: `FolderDetail.tsx:405–414`

```tsx
<p>No indexable files found in this folder.</p>
<p>Sery indexes parquet, csv, xlsx, xls, docx, pptx, html, and ipynb.</p>
```

**Problem**: Two issues:
- Missing `pdf` (tied to F1).
- No actionable next step. User who's indexed an empty folder or a
  folder full of `.jpg` just hits a wall. Could suggest: (a) pick a
  different folder, (b) open the folder in Finder to see what's
  there, (c) report unsupported formats they want.

**Fix**: Add `pdf` to the format list (post-F1). Add an action link
to pick a different folder via the Folders tab.

---

### 🟢 F4 — Dataset row shows "cols" count even for documents

**Where**: `FolderDetail.tsx:496–502`

```tsx
{dataset.schema.length > 0 && (
  <span>{dataset.schema.length} {dataset.schema.length === 1 ? 'col' : 'cols'}</span>
)}
```

**Problem**: A DOCX has `schema.length === 0` so this branch never
fires for documents. But if anything weird populates `schema` for
a document (e.g., extracted metadata surfaces as fake columns), the
user sees "2 cols" on a Word document which is confusing.

**Fix**: Defensive — only show col count when `!isDocumentFormat(d.file_format)`:

```tsx
{!isDocumentFormat(dataset.file_format) && dataset.schema.length > 0 && (
  <span>{dataset.schema.length} cols</span>
)}
```

Tiny defensive cleanup.

---

### 🟢 F5 — "Rescan" button label is a verb-noun collision

**Where**: `FolderDetail.tsx:282–286`

```tsx
<button disabled={scanState.running}>
  <RefreshCw className={scanRunning ? 'animate-spin' : ''} />
  Rescan
</button>
```

**Problem**: Minor: when `scanState.running === true`, the button
is disabled and the spinner runs, but the label still says "Rescan"
not "Scanning…". Small UX nit.

**Fix**: `{scanState.running ? 'Scanning…' : 'Rescan'}`.

---

## ConnectModal findings

### 🟢 C1 — "cloud sync" language could mislead

**Where**: `ConnectModal.tsx:111–114`

```
Paste a workspace key to enable cross-machine queries, the
Machines view, and cloud sync.
```

**Problem**: "Cloud sync" is ambiguous. To a Dropbox user it reads
as "syncs my files to the cloud." In v0.5.0, what actually syncs is
workspace metadata (machine list, dataset names, schemas) — not
file contents. The existing copy is technically accurate but prone
to the same misread that the LAUNCH_ASSETS rewrite was trying to
avoid.

**Fix**: Soften or drop. Two options:

1. Drop "and cloud sync":
   "Paste a workspace key to enable cross-machine queries and the
   Machines view."
2. Replace with what's actually synced:
   "Paste a workspace key to enable cross-machine queries and keep
   your Machines view live."

Either is fine. No blocker; just worth tightening since the
privacy-claim rigor is the whole brand promise.

---

### 🟢 C2 — Success toast has the same issue

**Where**: `ConnectModal.tsx:80`

```
toast.success('Connected. Your machines are syncing.');
```

**Same problem as C1**: "syncing" language. Users may wonder what's
being synced.

**Fix**: `toast.success('Connected. Your workspace is live.');`

---

## What I couldn't audit

- Actual behavior when the backend is down (ConnectModal): the
  error translation covers 401/403/timeout/network/429. Haven't
  verified the api returns those shapes.
- Whether `reveal_in_finder` actually works on Windows/Linux (Rust
  command inspection would confirm).
- Remote-URL empty state — if a watched remote URL 404s, does the
  scan fail cleanly in FolderDetail?

---

## Ordered fix list

| Sev | Fix | Effort |
|---|---|---|
| 🔴 | F1 — add PDF to scanner + UI + tests | 1 hour + verify real PDF |
| 🟡 | F2 — platform-aware "Open folder" label | 10 min |
| 🟡 | F3 — empty-state copy + pdf | 5 min (after F1) |
| 🟢 | F4 — hide col count on documents | 2 min |
| 🟢 | F5 — button label during scan | 2 min |
| 🟢 | C1 — drop "cloud sync" phrase | 2 min |
| 🟢 | C2 — "syncing" toast | 2 min |

**Ship-now ~1.5 hours** once F1 is verified against a real PDF. The
rest are small polish.

The ConnectModal surface is solid — well-architected, good error
translation, proper validation, dark-mode support. Only copy nits
here.
