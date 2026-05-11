# Telemetry

Sery Link sends anonymous usage pings to `analytics.sery.ai` so we
can answer two questions:

1. **How many people have installed Sery Link?**
2. **How many use it on a given day?**

That's the entire scope. The rest of this document is the literal
contract: what's collected, what isn't, where it goes, and how to
turn it off.

## What gets sent

One event, called `daily_ping`. Fired on launch and every 12 hours
while the app is running.

The on-disk shape (you can read your own queue at
`~/.seryai/events.jsonl`):

```json
{
  "event_id": "5f3a…",          // random per-ping UUID
  "event_name": "daily_ping",
  "occurred_at": "2026-05-10T14:23:00Z",
  "install_id": "b21c…",        // see below
  "props": {
    "version": "0.7.9",         // app build
    "platform": "macos"         // OS family: macos|linux|windows
  }
}
```

### `install_id`

A random UUID generated **once** the first time Sery Link loads its
config, then never changed for the lifetime of that install. It's the
only thing tying multiple pings together — without it, we couldn't
tell "10,000 distinct users opened the app today" from "one user
opened it 10,000 times."

The id is **not** tied to:

- Your workspace (`workspace_id` is never sent in pings)
- Your account (`user_id` is never sent in pings)
- The paired agent record (`agent_id` is never sent in pings)
- Your machine name, hostname, IP address, or MAC address

If you want a fresh `install_id` — e.g. you're handing the machine to
someone else, or you want to "reset" your contribution to our stats —
delete `~/.seryai/config.json` (or just clear the `install_id`
field). The next launch mints a new one.

## What does NOT get sent

The full list of things this code path never touches:

- **File contents** — not now, not ever. The brand promise.
- **File paths** — not even hashed. Earlier drafts hashed them; this
  build doesn't carry path-shaped fields on the wire at all.
- **File names**
- **SQL text** or **chat queries** you typed
- **Workspace ID, user ID, agent ID, machine hostname**
- **IP address** — the server records `received_at` and the JSON
  body. The reverse proxy may log an IP for rate-limiting/abuse, but
  that log is rotated and never joined back to the ping payload.
- **Email address** or any account-level info

If you find anything in `src-tauri/src/analytics.rs` that doesn't
match this list, that's a bug — please file an issue.

## Where it goes

`https://analytics.sery.ai/v1/pings`. Anonymous POST (no bearer
token), batched in groups of up to 200, flushed every 60 seconds or
when the queue grows past 10 events.

The server appends each ping as one JSON line to a date-partitioned
file (`dt=YYYY-MM-DD/source=desktop/<batch_uuid>.jsonl`) and that's
all. No database. The aggregated query is literally:

```sql
SELECT count(DISTINCT install_id)
FROM read_json('events_dir/dt=2026-05-10/**/*.jsonl');
```

## How to turn it off

Settings → Telemetry → toggle off, or set `app.telemetry_enabled =
false` in `~/.seryai/config.json`. When off:

- No new pings are queued
- The flusher stops attempting POSTs
- Any pings still buffered in `~/.seryai/events.jsonl` stay on disk
  until you delete them (or turn telemetry back on, at which point
  they drain)

Defaulted to **on** because the metrics genuinely help us know whether
the project is alive. We don't condition any product feature on the
flag — turning it off costs you nothing.

## Retention

- Raw JSONL files: kept on the server for **90 days**, then deleted.
- Aggregated counts (installs, DAU/WAU/MAU): kept indefinitely.

## Changes

Any change to this policy means a corresponding change to
`src-tauri/src/analytics.rs`, the version bump that ships it, and a
mention in the release notes. If we ever want to record something
beyond `install_id + version + platform`, the policy + the code
change land in the same PR — never one without the other.
