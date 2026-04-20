# Getting help with Sery Link

Sery Link is an open-source desktop app (AGPL-3.0) maintained by a small
team at [Sery.ai](https://sery.ai). This file is the quickest path from
"I hit something" to "I have an answer."

## Before you file an issue

1. **Use a released binary** from
   [GitHub Releases](https://github.com/seryai/serylink-releases/releases)
   if possible. If you've built from source, please verify the bug
   reproduces on a recent release before filing — self-builds have their
   own failure modes (wrong Rust toolchain, missing system libraries)
   that aren't bugs in Sery Link.
2. **Search existing issues** — your problem may already be reported
   or fixed on `main`.
3. **Check the docs at** [sery.ai/docs](https://sery.ai/docs) — common
   setup and usage questions are covered there.

## Where to file what

| What | Where |
|---|---|
| Bug in the Sery Link app | [seryai/sery-link/issues](https://github.com/seryai/sery-link/issues) |
| Bug in a community plugin | That plugin's own repo |
| Questions about how to use Sery Link | [seryai/sery-link/discussions](https://github.com/seryai/sery-link/discussions) |
| Questions about the AI tier / billing | hello@sery.ai |
| Security vulnerabilities | security@sery.ai — **do not file publicly** |
| Questions about publishing a community plugin | [seryai/serylink-releases](https://github.com/seryai/serylink-releases) |

## A good bug report

A bug report that gets fixed fast usually has:

- **Platform and version.** `Help → About` in the app, or `sery-link --version` if you have the CLI. "macOS 14.4, Sery Link 0.5.2."
- **Steps to reproduce.** Exactly what you did, in order. "I added a folder. I searched for `foo`. The app crashed."
- **What you expected to happen** and what actually happened.
- **Screenshot or short screen recording** for UI issues.
- **Relevant logs.** Sery Link writes logs to `~/.sery/logs/`; attach the most recent file, redacted if it contains paths you'd rather not share.
- **Your plugin list**, if you have any installed. Some bugs only show up with specific plugins active.

## What we can't help with

- **Compiling the source on your specific distro.** Distros vary — if you
  hit compile errors on, say, a non-mainstream Linux, community
  discussion is a better venue than an issue. We'll help when we can
  but can't commit to covering every build environment.
- **Plugin problems.** File at the plugin's own repo.
- **Feature requests without a problem.** We're happy to hear ideas,
  but "please add X" with no context of what problem X solves is likely
  to be closed politely. Describe the friction; we'll think about the
  fix.

## Response expectations

- **Critical bugs** (data loss, crash on launch, privacy regression) —
  we aim to respond within 24 hours and patch within a week.
- **Normal bugs** — response within a week, fix shipped on a normal
  release cadence.
- **Feature requests** — response within two weeks, fix ships whenever
  (or never, depending on fit with the roadmap).

We're a small team. Please be patient and kind; we're doing this
because we care about the product, not because it's a 24/7 enterprise
support contract.

## If you want to contribute a fix

See [CONTRIBUTING.md](./CONTRIBUTING.md). PRs are welcome, especially
for the long tail of platform-specific issues a small team can't cover
alone.
