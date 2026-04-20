# Security policy

## Reporting a vulnerability

**Please do not file public issues for security vulnerabilities in
Sery Link.** Public disclosure gives attackers a window to exploit
users before we can ship a fix.

Instead, email `security@sery.ai` with:

- A description of the vulnerability.
- Steps to reproduce, ideally with a minimal test case or proof of
  concept.
- The affected version(s) of Sery Link.
- Your assessment of severity and impact, if you have one.
- Whether you'd like credit in the advisory and, if so, how to be
  attributed.

We will acknowledge receipt within **72 hours** and keep you updated as
we investigate. For critical issues (data loss, remote code execution,
privacy regression, sandbox escape), we aim to ship a fix within
**7 days**. Less severe issues follow normal release cadence.

## Scope

In scope:

- The Sery Link desktop app (this repo).
- The bundled plugin runtime and sandbox.
- Any file included in a signed release binary.

Out of scope (please report to the appropriate owner instead):

- Vulnerabilities in a specific **community plugin** → report to that
  plugin's own repo.
- Vulnerabilities in **upstream dependencies** (Tauri, DuckDB, wasmer,
  etc.) → report upstream. If the upstream fix is slow and the impact
  on Sery Link is material, we'll take a local mitigation.
- Vulnerabilities in the **Sery.ai cloud backend** → same
  `security@sery.ai` inbox; we'll route internally.
- Social-engineering attacks on our users (phishing, fake installers)
  → we take these seriously but the fix is usually takedown, not
  patch; email the same inbox.

## Supported versions

We patch the **latest release** and the previous minor (e.g., if the
current release is 0.6.x, we patch 0.6.x and 0.5.x). Older versions are
not supported — please upgrade.

## Safe harbor

We will not pursue legal action against researchers who:

- Report vulnerabilities in good faith through the channel above.
- Do not exfiltrate user data, degrade service for other users, or
  violate privacy beyond what is strictly necessary to demonstrate the
  vulnerability.
- Give us a reasonable window to patch before public disclosure
  (typically 90 days for standard issues; shorter by mutual agreement
  for trivial fixes).

Thanks for helping keep Sery Link and its users safe.
