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
  tests, the `sdk/` drift check, the family rules (`sdk/scripts/launcher-check`,
  input `conformance`: errors fail, warnings are annotations), the package
  build and the allowlist check. Don't turn `conformance` off to get a
  green build: fix the error, or ask the owner for a waiver line in
  `.rea-check-waivers`.
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

### Native Linux launchers

A launcher for a game's native Linux build (DiRT Rally) calls
`launcher-ci` twice, with a `concurrency-key` each:

```yaml
  linux-packages:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@vX.Y.Z
    with:
      concurrency-key: linux
      rust-target: ""                     # host build: the native launcher
      package-command: packaging/package.sh "dist/<Id>Launcher-$REA_VERSION_NAME"
      # package-dir, package-allowlist, artifact-*: as above
  windows-tests:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@vX.Y.Z
    with:
      concurrency-key: windows-tests
      rust-target: i686-pc-windows-gnu    # the shared code under Wine
      wine-tests: true
      audit: false                        # the Linux job runs these
      sdk-check: false
      conformance: false
      shell-tests: ""
```

and the `release` job `needs: [linux-packages, windows-tests]`.

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

## 4. Before every push

`sdk/scripts/prepush-scan` scans the commits a push would publish and the
tree for secrets with a pinned gitleaks, and prints only paths, lines and
commit ids, never the matched text. A finding stops the push until the
owner decides. CI scans again; the local scan keeps a secret from ever
reaching GitHub.

## 5. Cutting a release (only when the owner says so)

1. `main` is green in CI, and `sdk/scripts/launcher-check` has no errors.
2. `sdk/scripts/release X.Y.Z`: bumps the crate that `launcher-release`
   checks (its `package:` input), dates the CHANGELOG section, runs
   `cargo test --locked` after the bump (what the tag's CI runs), checks
   the release notes, commits `chore: release vX.Y.Z` and makes the
   annotated tag. It never pushes.
3. `sdk/scripts/prepush-scan`, then push `main` and the tag when the owner
   says so.
4. Watch it gently: one `gh run view` every minute or two, not `gh run
   watch` in a tight loop. Frequent polling trips GitHub's secondary rate
   limit and the command exits non-zero, which looks like a failed release.
   Before you call a release failed, check `gh release view vX.Y.Z` and the
   run's conclusion. The release job checks the tag against the crate
   version, writes `SHA256SUMS`, attests provenance, takes the notes from
   that CHANGELOG section and publishes the zip and AppImage.
5. Verify: `gh release view vX.Y.Z` lists the zip, the AppImage and
   `SHA256SUMS`; README download links point at `releases/latest` without a
   hard-coded version.

6. **Clean up** once the release is published (`STANDARDS.md`, "Clean up
   after yourself"): preview with `sdk/scripts/clean`, then `--yes`; report
   the space freed.

If the release job fails after the tag was pushed, fix forward with a new
patch version; never move a published tag. The fix-forward release's notes
include the unpublished version's section: pass `notes-since: vX.Y.Z`
(the tag that never published) to `launcher-release`, or
`notes-since: latest-release`, for that one release; it runs
`release-notes.sh --since`.
