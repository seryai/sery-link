# Release process

How to cut a new Sery Link release and ship it to existing users via
the auto-updater.

## One-time setup (maintainer, done once)

The updater needs a signing keypair so users can verify that every
update genuinely came from us. The public key is baked into the app
(`src-tauri/tauri.conf.json → plugins.updater.pubkey`); the private
key must stay off the repo and be passed to CI as a secret.

**If a keypair hasn't been generated yet:**

```bash
pnpm tauri signer generate --password "" --write-keys ~/.tauri/sery-link.key
```

Then set the following GitHub repository secrets (Settings → Secrets
and variables → Actions):

| Secret | Value |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | `cat ~/.tauri/sery-link.key` — the full file contents |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | The password you set (empty string is fine if you passed `--password ""`) |

For signed macOS builds, also add:

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Base64-encoded `.p12` developer certificate |
| `APPLE_CERTIFICATE_PASSWORD` | Password for the `.p12` |
| `APPLE_SIGNING_IDENTITY` | The `Developer ID Application: ...` string from your cert |
| `APPLE_ID` | Apple ID used for notarization |
| `APPLE_PASSWORD` | App-specific password (appleid.apple.com → Sign-In and Security → App-Specific Passwords) |
| `APPLE_TEAM_ID` | Your Apple Developer team ID |

For signed Windows builds, configure the certificate per Tauri's
[Windows code-signing docs](https://tauri.app/v2/distribute/sign/windows/).

### Back up the private key

**Losing the private key means no more updates can ever ship.** Back it
up in a password manager (1Password, Bitwarden, etc.). If you ever lose
it, every user has to manually install the next release because their
existing install won't trust any new signature.

## Cutting a release

Releases are triggered by pushing a tag matching `v*.*.*`.

```bash
# 1. Make sure main is clean and up to date
git checkout main
git pull --ff-only

# 2. Bump the version in three places:
#    - package.json
#    - src-tauri/Cargo.toml
#    - src-tauri/tauri.conf.json
#    - CHANGELOG.md (move [Unreleased] → [x.y.z] with today's date)

# 3. Commit the version bump
git commit -am "release: v0.5.0"

# 4. Tag and push
git tag v0.5.0
git push origin main v0.5.0
```

The push triggers `.github/workflows/release.yml`, which:

1. Builds Sery Link on macOS (arm64 + x64), Windows, and Linux in parallel
2. Signs each artifact with the updater private key
3. Uploads everything to a **draft** GitHub Release named `Sery Link v0.5.0`
4. Generates `latest.json` (the updater manifest) and attaches it

Once the workflow finishes (~10–15 minutes):

1. Open the draft release at `https://github.com/seryai/sery-link/releases`
2. Spot-check that all platform artifacts are attached + `latest.json`
3. Download one binary on your own machine, run it, confirm it launches
4. Click **Publish release**

As soon as it's published, every existing install will see the new
version on its next update-check (Tauri updater polls at startup and
then every ~24h depending on plugin config).

## Verifying the updater works

After publishing, smoke-test the update path:

1. Install the *previous* version on a clean machine (or VM)
2. Launch it
3. You should see an "Update available" dialog within a few seconds
4. Accept → it should download, verify signature, and restart into the new version

If the dialog doesn't appear, check:

- The `endpoints` URL in `tauri.conf.json` resolves (paste it in a
  browser; you should get the `latest.json` contents, not a 404)
- The `pubkey` in `tauri.conf.json` matches `~/.tauri/sery-link.key.pub`
- `latest.json` in the release has signature strings for each platform
- Nothing is blocking outbound HTTPS from the client machine

## Skipping the updater for a release (advanced)

If a release shouldn't be auto-pushed to existing users (e.g., a beta
cut or a release with a known regression):

1. Publish it as a **prerelease** on GitHub
2. Upload `latest.json` with a distinct filename (e.g., `beta.json`)
   or don't upload it at all

Users on the stable channel continue pointing at `latest.json` from
the latest *non-prerelease* release, so they won't pick up the beta.

## Yanking a bad release

If a published release has a serious regression:

1. Mark the GitHub release as **prerelease** (or unpublish). This
   removes it from `/releases/latest` so new installs can't pick it
   up.
2. Re-point `latest.json` at the previous stable. The easiest way is
   to re-upload the prior release's `latest.json` to the *current*
   release page and re-publish.
3. Cut a new patch release with the fix as soon as you have one. Users
   who already auto-updated to the bad version will then be pulled
   forward to the fixed version on their next check.

For incident response beyond a simple unpublish, contact
`security@sery.ai`.
