# Playbook: change the SDK, bump the launchers

## When a launcher needs something the SDK lacks

1. Don't copy SDK code into the launcher and don't edit its vendored `sdk/`.
   Keep the launcher's local code for now and open an issue (or PR) in
   `akitaonrails/rea-launcher-sdk` describing the gap: which launcher, what
   it needs, file:line.
2. In the SDK, make the change generic: game knowledge comes in as data or a
   trait implementation, a new behaviour is opt-in until every launcher has
   verified it. Add tests (host and `scripts/wine-test`), update the crate
   README (and its MIGRATION notes if behaviour differs), `CHANGELOG.md`.
3. Run `scripts/dev-check` (it includes `launcher-check`'s tests and a
   fresh scaffold passing it). Release: bump the workspace version,
   CHANGELOG section, tag `vX.Y.Z`, push with the tag.

## Releasing an SDK version

1. **Version.** Patch (`v0.2.1`): fixes only, no behaviour or API change.
   Minor (`v0.3.0`, pre-1.0): API or behaviour changes, each marked
   **Migration** in `CHANGELOG.md` with *who is affected, what to do, how to
   verify*.
2. **Canary.** The `compat` workflow builds and tests every launcher against
   the SDK commit (its SDK dependency patched to the checkout, `sdk/`
   re-vendored, Wine tests included). It must be green, or every red
   launcher explained in the release notes with the migration that fixes it.
3. **Tag** `vX.Y.Z` on `main` and push; the `release` workflow publishes the
   notes from the CHANGELOG.

## Upgrading a launcher (avoid regressions)

**Only when it pays.** Bump a launcher only when the new SDK version
fixes something or adds a feature that launcher uses (security fixes
always count). Read the CHANGELOG between the pinned tag and the new one
and name the items that matter for this launcher in the bump commit; if
there are none, skip the launcher and close its `sdk-update` pull
request.

1. **Baseline.** On the launcher's `main`: full check, `sdk/scripts/wine-test`,
   and the golden snapshots (`STANDARDS.md`, "Golden snapshots") all pass.
   If the launcher has no snapshots yet, add them *first*, at the old SDK
   version, in their own commit.
2. **Branch** `sdk-update/vX.Y.Z` (the weekly `sdk-update` workflow opens it
   as a pull request automatically); it is merged into the default branch
   once CI is green.
3. **Bump** with `sdk/scripts/sdk-bump vX.Y.Z`: Cargo tags and lockfile,
   re-vendored `sdk/`, workflow `@vX.Y.Z` references. Commit
   `build: bump rea-launcher-sdk to vX.Y.Z` — including the minimal API
   changes needed to compile (every commit must pass the checks); larger
   Migration items get their own commits in step 4.
4. **Read every Migration item** between the old and the new version
   (`sdk-bump` prints them). Apply each in its own commit.
5. **Remove workarounds** the new version makes unnecessary (local copies,
   shims noted as "until the SDK has …").
6. **Compare snapshots.** Any diff must be explained by step 4 or 5; an
   unexplained diff is a regression — fix it or report it to the SDK
   (`CONTRIBUTING.md`), don't regenerate the snapshot.
7. **All checks** again: fmt, tests, clippy (Windows target), shell tests,
   `cargo deny`, `sdk-sync --check`, `wine-test`, release build links, and
   `sdk/scripts/launcher-check` (the new version's: rules that waited for
   this SDK version are errors now). Fix every error; a rule the owner
   decided to waive gets a `rule: reason` line in `.rea-check-waivers`.
8. **Runtime notes.** Behaviour changes that need the real game go into
   `docs/RUNTIME_NOTES.md` as pending checks; run them headless when the game
   is available.
9. **CHANGELOG** "Unreleased": `Updated rea-launcher-sdk to vX.Y.Z` plus every
   player-visible change. Merge when CI is green; release a launcher version
   when the owner says so.

## When a launcher repeats a mistake

A mistake found in one launcher that a file could show (a missing test, a
hand-made rule the SDK has a helper for, a missing document entry) becomes
a `scripts/launcher-check` rule in the same SDK change: the reason and the
fix in its `--explain` text, a case in `scripts/tests/launcher-check.sh`,
and, when it needs a new SDK API, `SINCE` set to that version so older
launchers see a warning until they bump. Run it read-only against every
launcher and tune false positives; never hide a real one. Only what can't
be checked goes into STANDARDS or a playbook.

## Auditing the launchers for drift

Every few releases, compare the launchers feature by feature (gamepad,
settings window, config, runtime, Linux helper, packaging, CI) the way
`docs/AUDIT.md` did: find which launcher does each thing best, bring the
others up to it, and move anything now duplicated into the SDK.
