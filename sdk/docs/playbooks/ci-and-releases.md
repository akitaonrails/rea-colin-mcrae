# Playbook: GitHub Actions that build and publish the release binaries

Every launcher builds its Windows zip and Linux AppImage in GitHub Actions,
never on a developer's machine, and publishes them as a GitHub release when a
version tag is pushed. The SDK provides the workflows; the launcher only
calls them. `scripts/new-launcher` generates all of this; this playbook is
how to check and adapt it.

## 1. The workflow file

`.github/workflows/build.yml` in the launcher:

```yaml
name: build
on:
  push:
    branches: [main]
    tags: ["v*"]
  pull_request:
  workflow_dispatch:

concurrency:                 # one run per branch; never cancel a tag's release run
  group: build-${{ github.ref }}
  cancel-in-progress: ${{ !startsWith(github.ref, 'refs/tags/') }}

permissions:
  contents: read

jobs:
  ci:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.3.0
    with:
      rust-target: i686-pc-windows-gnu        # x86_64-pc-windows-gnu for 64-bit games
      wine-tests: true                         # run the Windows-target tests under Wine
      package-command: packaging/package.sh "dist/<Id>Launcher-$REA_VERSION_NAME"
      package-dir: dist/<Id>Launcher-*/
      package-allowlist: packaging/package.allow
      artifact-name: <Id>Launcher-packages
      artifact-paths: |
        dist/*.zip
        dist/*.AppImage

  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: ci
    permissions:
      contents: write        # create the release
      id-token: write        # build-provenance attestation
      attestations: write
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.3.0
    with:
      package: <id>-launcher                   # the crate whose version must match the tag
      artifact-pattern: <Id>Launcher-*
```

- Pin the SDK workflows to the same tag as the Cargo dependencies and
  `sdk/VERSION` (`sdk-bump` keeps all three in step).
- The `release` job must grant all three permissions even with `attest:
  false`: GitHub checks a called workflow's permissions before any `if:`.
- The launcher's checks all run in `ci`: fmt, tests, `cargo deny`/`audit`,
  shell syntax and `linux/tests`, Windows-target clippy and build, Wine
  tests, the `sdk/` drift check, the package build and the allowlist check.
- **Link with lld, for every Windows target** (i686 and x86_64): GNU ld 2.47
  leaves imports unresolved and the program crashes on that call, often
  before `main`. `.cargo/config.toml` sets `-C link-arg=-fuse-ld=lld` per
  target; `launcher-ci` fails when it doesn't (`sdk/packaging/check-lld.sh`)
  and checks every import of the built `.exe`/`.dll`
  (`sdk/packaging/check-imports.sh`; input `imports-check`, on by default).
  Run both by hand after changing the linker setup.
  Game-specific extra jobs (CMR3's vkBasalt build, Screamer's MSYS2 build and
  source archive) live beside `ci` and upload artifacts matching
  `artifact-pattern`.
- Optional workflows: `upstream-watch` (weekly, opens an issue when a pinned
  component publishes something new), `mirror` (copies mirrorable pins into
  the repo's `thirdparty` release), `sdk-update` (weekly SDK upgrade pull
  request). See `packaging/README.md`.

## 2. What the package must contain

`packaging/package.sh` assembles the zip folder: the launcher exe, the
bundled components fetched from the pins (`sdk/packaging/pins.sh`), the
Linux helper with `rea-play.sh` beside it, `README.md`, `THIRD_PARTY.md`,
`CHANGELOG.md`, both licences and the component licences. The AppImage is
built with `sdk/packaging/appimage-wine.sh` (portable Wine; `--native` for
native games). `packaging/package.allow` lists every file the zip may hold;
anything else, and any game file, fails CI.

## 3. Repository settings

- Actions enabled; default workflow permissions can stay read-only (the
  jobs ask for what they need).
- Branch protection on `main` requiring the `ci` job is recommended.
- No secrets are needed: `GITHUB_TOKEN` creates the release.

## 4. Cutting a release (only when the owner says so)

1. `main` is green in CI.
2. Bump the crate version in `Cargo.toml` (`cargo update -w` to refresh
   `Cargo.lock`), turn CHANGELOG "Unreleased" into `## [X.Y.Z] - date`.
   Commit `chore: release vX.Y.Z`.
3. `git tag -a vX.Y.Z -m "vX.Y.Z" && git push origin main vX.Y.Z`.
4. Watch it: `gh run watch` on the tag's run. The release job checks the tag
   against the crate version, writes `SHA256SUMS`, attests provenance,
   takes the notes from that CHANGELOG section and publishes the zip and
   AppImage.
5. Verify: `gh release view vX.Y.Z` lists the zip, the AppImage and
   `SHA256SUMS`; README download links point at `releases/latest` without a
   hard-coded version.

6. **Clean up** once the release is published (`STANDARDS.md`, "Clean up
   after yourself"): preview with `sdk/scripts/clean`, then `--yes`; report
   the space freed.

If the release job fails after the tag was pushed, fix forward with a new
patch version; never move a published tag.
