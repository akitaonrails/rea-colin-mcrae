# Playbook: build a new launcher

For a coding agent asked to make an old PC game plug-and-play with this SDK.
Read `docs/STANDARDS.md` first; it is binding. This playbook is the order of
work. Each phase ends with something committed and checked, and "checked"
always includes `sdk/scripts/launcher-check` with no errors: it knows the
mistakes earlier launchers shipped (no golden snapshots, a hand-made
audience, no instance guard, own fixes without drafts…). Treat its errors
as blocking; `--explain RULE` says why and how to fix. Only the owner
waives a rule (`.rea-check-waivers`).

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
workflows (and the weekly `sdk-update`), packaging, golden snapshots, the
SDK audience and instance guard, the issue forms, and a launcher that
builds and passes its checks, `launcher-check` included. `git init`,
commit, and run the checks before changing anything. Keep what the
scaffold wires (snapshots, `report::audience`, `instance::single`,
`WINDOW_TITLE`): replace the example options, never the wiring.

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
   golden bytes, applied to the runtime copy only. Each fix of our own gets
   its draft in `docs/CONTRIBUTIONS.md` in the same commit, cited from its
   `RESEARCH.md` row (STANDARDS "Contribute back").
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
- Every way of quitting, in the real game, recorded in the
  `RUNTIME_NOTES.md` "Quitting and leftovers" table: the game's menu,
  window close / Alt+F4, `<id>-play stop` and Ctrl+C, then the process
  table (STANDARDS "Clean exit, single instance"; see "Test notes" below).
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
  `docs/playbooks/ci-and-releases.md`. Push only when the owner says so
  (after `sdk/scripts/prepush-scan`); release (tag) only once CI is green
  and the owner asks, with `sdk/scripts/release X.Y.Z` after
  `sdk/scripts/launcher-check` passes.

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

## Patterns (from the launchers so far)

Not checkable, so written down. Each names the launcher to read.

**Native Linux port** (the game ships its own Linux build: DiRT Rally).
The launcher is a native Linux program for that build and a Windows
program for the Windows build.
- CI: two `launcher-ci` jobs (`docs/playbooks/ci-and-releases.md`, "Native
  Linux"): Linux packages with `rust-target: ""`, and the Windows-target
  tests with `rust-target: i686-pc-windows-gnu`, `wine-tests: true`.
- Errors before the window: `rea_launcher_ui::report::fatal` (a small
  message window with the issue link), never only `eprintln!`.
- One instance: `rea_launcher::instance::single`; the helper's session lock
  doesn't cover the binary started from a menu.
- Pads in the window: `rea_launcher_ui::pad::joystick::Joysticks` feeding
  `PadNav` (`/dev/input/js*`).

**Old native Linux ports** (2015–2018 Feral/Aspyr builds, DiRT Rally).
- Zen 3+ CPUs with protection keys fault on execute-only mappings: a small
  `LD_PRELOAD` library that adds read to exec-only `mmap`/`mprotect`
  (DiRT Rally's `dirtrally-exec-read`). Our own fix, so it gets a draft.
- Legacy libraries the distro no longer has (OpenSSL 1.0, libidn11…) are
  bundled in the game's own `game-libs/` and put on the game's
  `LD_LIBRARY_PATH` only, never on the launcher's own.
- glibc ≥ 2.37 no longer expands `$PLATFORM` in RPATH: point
  `LD_LIBRARY_PATH` at the runtime's `lib/x86_64`.
- Feral's pad list (`share/inputdevices.json`): newer pads are only a
  joystick until added there, in the runtime copy.

**Game source from an installer** (CMR2005: GOG's installer). GOG
installers hang under Wine, so the launcher unpacks the installer itself
with a bundled `innoextract`: identify the installer by the setup's
SHA-256 and part sizes, unpack in steps with a progress window (time
left), and on cancel or failure remove the partial folder. The unpacked
folder is then the game folder (read-only from then on).

**Portrait main monitor** (CMR2005, CMR4). Without gamescope, a game on a
portrait main monitor under Wine fails with "Adapter has no modes": run it
in a landscape Wine virtual desktop, and track `explorer.exe` of that
desktop, not the game, as the process to wait for.

**Saves through a registry path** (CMR2005). When the game reads its save
folder from the registry, point the value at the launcher's saves folder
(no mirror), and import old saves once, with a marker, so a save the
player deleted never comes back.

**Headless screenshots** for the README: `docs/playbooks/readme.md`,
"Screenshots, headless".

### Test notes

- gamescope has a file capability, so its `/proc/<pid>/environ` can't be
  read: match it by command line.
- A command a script starts in the background (`&`) has SIGINT ignored,
  and a shell can't undo that: a Ctrl+C test starts the helper in the
  foreground of its own process group, or from a program that resets the
  signal, then sends SIGINT.
- The launcher's window title must differ from the game's
  (`WINDOW_TITLE`, "… Launcher"): CMR3's tests and second start found the
  wrong window.
- `scripts/wine-test` keeps its prefix between runs: a registry test
  cleans up the keys it wrote.
- A test never depends on what this machine has installed (gamescope,
  xrandr, Wine): put a stand-in first on `PATH` or set the helper's
  `REA_*` seam. A Linux helper test passed on a desktop with gamescope
  and failed in the gaming box and on CI runners, which have none.
