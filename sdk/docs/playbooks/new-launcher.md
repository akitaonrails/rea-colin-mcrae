# Playbook: build a new launcher

For a coding agent asked to make an old PC game plug-and-play with this SDK.
Read `docs/STANDARDS.md` first; it is binding. This playbook is the order of
work. Each phase ends with something committed and checked.

## Inputs (ask for any that are missing)

| Input | Example |
|---|---|
| Game and edition | "Colin McRae Rally 3, PC, v1.1" |
| Game folder (the player's installed copy, **read-only**) | `/mnt/games/CMR3` |
| Launcher id (short, lower case) and env prefix | `cmr3`, `CMR3_` |
| New repository folder | `~/Projects/rea-colin-mcrae-3` |
| Where the SDK checkout is | `~/Projects/rea-launcher-sdk` |

Never copy anything out of the game folder into the repository. Investigation
output goes in the new repo's `.work/` (ignored by Git).

## Phase 1 — Research (before any code)

Search online for what the community already solved (`STANDARDS.md`,
"Research before building"): PCGamingWiki's page for the game, widescreen
and frame-rate fixes, unofficial patches, input fixes, ProtonDB and WineHQ
AppDB reports, GitHub projects for the game. For each find: what it fixes,
licence, maintained or not, exact game version it needs.

Write `docs/RESEARCH.md` with the findings and a decision per item
(bundle / configure / download on request / not used, with the reason).
Show the owner the list of problems the launcher will solve and the
community components it will use before going further.

## Phase 2 — Know the game (REA)

Use the REA MCP tools on the game's files (read in place):

- **Identity:** SHA-256 of every executable and DLL (`inspect_artifact`,
  `binary_overview`); record each known build. Unknown builds get refused
  later, so collect every edition the owner has.
- **Platform:** `inspect_pe_imports` / `inspect_native_api` — renderer
  (D3D7/8/9, OpenGL, DirectDraw → dgVoodoo2, DXVK), input (DirectInput →
  Xidi; XInput), audio (DirectSound, EAX → DSOAL), DRM, CD checks.
- **Files it writes:** config files, saves, registry keys
  (`search_strings` for `.ini`, `.cfg`, `Software\\`, save names). These
  decide the runtime `Policy` (game-written files) and the config the
  launcher generates.
- **Only if nothing in Phase 1 fixes a problem:** find the one place a whole
  class of cases goes through (a camera matrix, a frame-time clamp, a mode
  table) with `procedure_pseudo_code`, `xrefs`, `read_bytes`; verify condition
  codes against the disassembly, not the pseudocode. Design the patch as
  byte sites with original and patched bytes (`rea_launcher::patch`),
  following `docs/playbooks/binary-patching.md`.
- **Old-game trouble spots:** check each one from `STANDARDS.md`
  ("Robustness and support"): timers on fast CPUs, core affinity,
  DirectSound3D/EAX, CD audio, intro-video codecs, aspect tables, focus loss.
- **Runtime evidence:** run the original game from a scratch copy outside
  the repo, headless: `gamescope --backend headless` with a private
  PipeWire null sink, a throwaway Wine prefix under `.work/`. Note what
  breaks.

Record findings in `docs/RESEARCH.md` (static) and `docs/RUNTIME_NOTES.md`
(what ran, what was seen), keeping observations, inferences and decisions
apart.

## Phase 3 — Scaffold

```sh
<sdk>/scripts/new-launcher --id cmr3 --prefix CMR3 --game "Colin McRae Rally 3" \
    --exe Rally_3PC.exe --out ~/Projects/rea-colin-mcrae-3
```

This creates the repository shape from `STANDARDS.md` with the SDK as a
pinned dependency, `sdk/` vendored, `AGENTS.md`, CI calling the SDK
workflows, packaging, and a launcher that builds and passes its checks.
`git init`, commit, and run the checks before changing anything.

## Phase 4 — Build, in this order

Commit after each step, only when the checks pass.

1. **Edition** (`edition.rs`): the SHA-256 table from Phase 2
   (`rea_launcher::identify`). Unknown → refuse with the digest.
2. **Runtime** (`runtime.rs`): a `Policy` — which files may be hard-linked
   (large read-only data), which the game writes (never overwritten once
   present), which the launcher owns. `sync`, `check_separate`, saves
   `Mirror` with recorded state.
3. **Components** (`docs/playbooks/community-components.md`): each community component from Phase 1 pinned in
   `packaging/*.pins` with SHA-256, installed through the `Ledger`, licence
   in `THIRD_PARTY.md`. Wine DLL overrides via `rea_launcher::wine`.
4. **Configuration**: `launcher.cfg` keys (`rea_launcher::cfg`), generators
   for the game's own files using the INI editor that keeps the player's
   values. Unset = keep the game's value.
5. **Patches** (only from Phase 2): exact-build byte sites, tested against
   golden bytes, applied to the runtime copy only.
6. **Looks**: Original (as shipped), a recommended look from the screen,
   Steam Deck.
7. **Settings window**: use a design skill (`frontend-design`, then review
   with `web-design-guidelines`) on top of `rea-launcher-ui`. Follow the
   accessibility and "no confusing options" rules. Only options whose
   effect you have seen in the game. Research the window's imagery online
   as part of the design (`STANDARDS.md`, "Give the window the game's
   identity"): freely licensed photos of the real driver, car or place
   first, official art only with the owner's decision, else an original
   illustration; credit each image in `THIRD_PARTY.md` and in the window.
8. **CLI**: one `apply(key, value)` for file keys and flags; `--help`.
9. **Linux helper** `linux/<id>-play` from the recipe in
   `sdk/shell/README.md`; tests in `linux/tests/`.
10. **Packaging**: pins, allowlist, AppImage via the SDK builder.

## Phase 5 — Verify

- Unit tests for edition, policy, patches, config and INI output.
- End-to-end tests for every window option and CLI flag
  (`STANDARDS.md`, "Settings window and CLI"); window tests headless with
  `egui_kittest`.
- `scripts/wine-test` for the Windows-target tests; link the release build.
- Hostile paths (spaces, apostrophes, accents) and upgrade tests that read
  every file format an earlier release wrote.
- `--check` on every known edition and on an unknown file (refused with its
  SHA-256 and the issue-template link).
- The platform list in `STANDARDS.md`: Steam Deck Game Mode, Proton without
  the helper, multi-monitor, HiDPI/4K, ultrawide — record each as verified
  or not yet in `docs/RUNTIME_NOTES.md`.
- The real game, headless, through the launcher: each look, each option you
  ship. Record exactly what was and was not verified in
  `docs/RUNTIME_NOTES.md`. Don't open windows on the owner's desktop unless
  a check needs eyes or headless would prove nothing — then tell the user
  what will open and why, and wait until they're ready
  (`STANDARDS.md`, "Visible when it matters").

## Phase 6 — Document and release

- README: what problems the launcher solves (one line each), install on
  Windows / Steam Deck / Linux, the options, credits to every community
  project, "built on rea-launcher-sdk", and a Contributing section routing
  shared-code issues to the SDK repository. Follow
  `docs/playbooks/readme.md` (outline, rules, final humanizer pass).
- `THIRD_PARTY.md`, `CHANGELOG.md`, `AGENTS.md` up to date.
- GitHub Actions build the zip and AppImage and publish releases: check
  and adapt the generated `.github/workflows/build.yml` following
  `docs/playbooks/ci-and-releases.md`. Push only when the owner says so;
  release (tag) only once CI is green and the owner asks.

## Maintain it

Contributions to the launcher's own repository (issues, pull requests,
dependency bumps, releases) are handled with the maintenance skills the
SDK README recommends ("Maintaining your launcher's repository":
`iss-audit`, `pr-audit`, `github-resolution`, `pr-bump`, `pr-post-audit`,
`security-audit`, `release`) — adapted to the maintainer's setup. SDK
upgrades follow `sdk-change.md` ("Upgrading a launcher").

## Phase 7 — Give back

If Phase 2 produced a fix the community lacks and it is verified in the
game, draft a contribution (`docs/playbooks/contribute-back.md`). Never post
without the owner's approval.
