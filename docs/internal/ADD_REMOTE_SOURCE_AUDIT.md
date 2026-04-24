# AddRemoteSourceModal audit

The S3 / HTTPS source entry flow. Critical for the v0.5.0 remote-
sources feature; if broken, the "Sery also indexes your S3
buckets" pitch dies.

Audited 2026-04-23. Scope:
`src/components/AddRemoteSourceModal.tsx`.

**Severity**: 🔴 blocker · 🟡 friction · 🟢 polish

Verdict: no blockers; modest copy polish. The security messaging,
URL handling, and dark-mode support are already strong.

---

## Findings

### 🟡 A1 — "macOS Keychain" is platform-specific

**Where**: Line 148

```
Keys are saved to your macOS Keychain and used only to read this
bucket. They never leave your machine — queries run in DuckDB on
your laptop.
```

**Problem**: Sery Link ships on macOS, Windows, and Linux. Windows
users have the **Windows Credential Manager**; Linux users have
**Secret Service / libsecret** (or the `keyring` crate's fallback).
Users not on macOS read this and either (a) assume it doesn't
apply, or (b) worry their secrets aren't protected the same way.

**Fix**: Neutral language — "your system's credential store" or
"the OS keychain" (lowercase, generic):

```
Keys are saved to your OS's credential store (Keychain on macOS,
Credential Manager on Windows, Secret Service on Linux) and used
only to read this bucket. Nothing is uploaded.
```

Or simpler:

```
Keys are stored securely in your OS keychain and used only to
read this bucket. Nothing is uploaded.
```

---

### 🟡 A2 — "DuckDB on your laptop" exposes an internal

**Where**: Line 149–150

```
They never leave your machine — queries run in DuckDB on your laptop.
```

**Problem**: Mentions DuckDB by name. Users don't need to know the
engine, and v0.5.0 positioning leans away from naming internals.
Also: "your laptop" is one device form factor; some users are on
desktops, servers, or NASes.

**Fix**:

```
They never leave your machine. Reads happen locally — nothing is
uploaded.
```

Same claim, less plumbing exposed, device-neutral.

---

### 🟡 A3 — Description says "a CSV or Parquet file" but modal accepts more

**Where**: Line 105–107

```
Paste a public URL to a CSV or Parquet file. Sery Link fetches
the schema locally — the file is never uploaded to our servers.
```

**Problem**: Two mismatches with the actual capability:
1. The modal accepts **S3 URLs with credentials** too, not just
   public URLs.
2. The "What works" footer (line 215–223) lists bucket listings
   (`s3://bucket/prefix/`) and globs (`s3://bucket/**/*.parquet`)
   — not just "a CSV or Parquet file."

A user who comes here wanting to add a whole S3 prefix reads the
header and thinks they're in the wrong place.

**Fix**: Expand the description to match capability:

```
Add a public URL, an S3 object, or an S3 bucket prefix. Sery
Link reads the schema locally — nothing is uploaded to our
servers.
```

Drop "CSV or Parquet file" as the only shape. The "What works"
box below already gives the detail.

---

### 🟡 A4 — Title "Add a remote file" understates the surface

**Where**: Line 102

```tsx
<h2>Add a remote file</h2>
```

**Problem**: Modal also adds S3 prefixes (folders, not files) per
the What Works section. A user adding a whole prefix sees the word
"file" and second-guesses.

**Fix**: `Add a remote source` — matches the component name and
covers both cases.

---

### 🟡 A5 — URL placeholder is hard to parse

**Where**: Line 128

```
https://example.com/data.csv,  s3://bucket/path/file.parquet,  or  s3://bucket/prefix/
```

**Problem**: Three comma-separated URLs in a placeholder are
visually noisy, and some users pattern-match on "looks like a
placeholder, ignore it" without reading. Also: a single input
field showing three examples doesn't convey that you enter *one*.

**Fix**: Single clean placeholder + supplementary help text:

```tsx
placeholder="https:// or s3:// URL"
```

The "What works" footer below already shows concrete examples.
Don't duplicate them in the placeholder.

---

### 🟢 A6 — No client-side URL format validation

**Where**: Submit handler lines 56–79.

**Problem**: If a user types "example.com" (no protocol), the
submit goes to the backend, which returns an error. Client-side
validation could catch this earlier with a gentler "URL must start
with https:// or s3://" message.

**Fix** (nice-to-have):

```tsx
const urlValid =
  /^(https?:\/\/|s3:\/\/)/i.test(trimmedUrl);

// In canSubmit:
!busy && trimmedUrl !== '' && urlValid && (...)

// Inline error below field when trimmedUrl && !urlValid
```

Low priority — the backend error is already reasonably friendly.

---

### 🟢 A7 — No quick-help link to AWS cred docs

**Where**: Credentials section (lines 141–203).

**Problem**: First-time S3 users might not know where to get an
access key. A "How do I get these?" link to AWS IAM docs (or
Sery's own docs explaining least-privilege IAM policy) would
reduce confusion.

**Fix** (nice-to-have): small "Where do I find these?" anchor
linking to `sery.ai/docs/s3-credentials` (a docs page that should
exist; if not, create one).

---

## Ordered fix list

| Sev | Fix | Effort |
|---|---|---|
| 🟡 | A1 — platform-neutral keychain copy | 2 min |
| 🟡 | A2 — drop "DuckDB" mention | 1 min |
| 🟡 | A3 — expand description to match capability | 2 min |
| 🟡 | A4 — "remote file" → "remote source" | 1 min |
| 🟡 | A5 — clean placeholder | 1 min |
| 🟢 | A6 — client-side URL validation | 15 min |
| 🟢 | A7 — cred docs link | 5 min + docs page |

**Ship-now**: ~10 minutes to knock out A1–A5. The rest can wait.
