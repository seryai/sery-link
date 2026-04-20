# Contributing to Sery Link

Thanks for your interest in Sery Link. This file covers what we accept,
how to submit it, and what to expect from us in return.

## License

Sery Link is licensed under the **GNU Affero General Public License v3.0
or later** (AGPL-3.0-or-later). By submitting a contribution, you agree
that your work will be released under the same license.

## Sign-off (DCO)

We use the [Developer Certificate of Origin](https://developercertificate.org/)
to track contributions. Every commit must be signed off with:

```
git commit -s -m "your message"
```

The `-s` appends a `Signed-off-by:` line that asserts you wrote (or have
the right to contribute) the code. No CLA required — the DCO is enough.

## What we're looking for

Things we're generally happy to merge:

- **Platform fixes** — macOS, Windows, Linux edge cases, accessibility
  improvements, keyring quirks, installer issues.
- **Bug fixes with a clear reproduction** — file a small, minimal diff
  that describes the bug and how your change fixes it.
- **Documentation improvements** — README, code comments, `docs/` pages.
- **Tests** — especially for subtle bugs you just fixed.
- **Small, focused features** that fit the v0.5.0 positioning (see
  `../datalake/PROJECT.md` §1). Column-aware search refinements,
  per-file profile enhancements, remote-source connectors, plugin
  system improvements.

## What we usually won't merge

- **Large unsolicited features.** If a PR rewrites a subsystem or adds
  a new top-level feature without prior discussion, expect a close with
  a pointer to open an issue first. Discuss before building.
- **"Style" PRs.** Reformatting, rename-a-variable, prefer-const sweeps
  without a matching behavioral reason. They create noise without
  adding value.
- **Dependency bumps for the sake of newness.** A bump needs a reason:
  security advisory, a bug you hit, a feature you need. "Latest" alone
  is not a reason.
- **Changes that work around our privacy model.** Sery Link's central
  contract is that raw files never leave the machine. A PR that opens
  a new outbound channel, exfiltrates content, weakens sandboxing for
  plugins, or silently enables telemetry won't land.
- **Anything that requires the user to trust a new cloud service**
  we haven't vetted and documented.

## Before you open a PR

1. **Open an issue first** for anything beyond a small fix. Describe
   the problem, the proposed solution, and platforms affected. Getting
   alignment up front saves wasted work.
2. **Keep diffs small and focused.** One PR = one concern. If you find
   unrelated issues while working, file them separately.
3. **Test it on at least one platform.** Note which one in the PR
   description. If your change is platform-sensitive (keychain,
   filesystem notifications, installer), test more than one.
4. **Run the existing tests.** `cargo test` in `src-tauri/`, `pnpm test`
   in the root (when present). If you're adding behavior, add a test.
5. **Don't regenerate files you didn't mean to change.** Lockfiles,
   generated binaries, OS-specific artifacts shouldn't sneak into your
   PR unless they're the actual subject of the change.

## PR review

We aim to respond to every PR within a week — often sooner, sometimes
slower during launch crunch. If a PR sits without a response for two
weeks, nudge us on the thread.

Expect honest feedback. A "nit" comment is low-stakes; a "blocking"
comment is something the PR needs to address before merge. If we close
a PR, we'll tell you why.

## Security issues

**Do not file public issues for security vulnerabilities.** Email
`security@sery.ai` (or whichever inbox the maintainers publish in
SECURITY.md). We take these seriously and will respond within 72 hours.

## Questions

- Usage questions → see [SUPPORT.md](./SUPPORT.md).
- Plugin development → see the `examples/plugins/` folder and the
  [serylink-releases](https://github.com/seryai/serylink-releases) repo
  for the community directory.
- Everything else → open a discussion or an issue.

Thanks for helping make Sery Link better.
