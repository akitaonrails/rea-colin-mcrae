# Launcher standards

The rules every rea launcher follows. Each launcher's `AGENTS.md` points
here (`sdk/docs/STANDARDS.md`) and adds only what is specific to its game.

**What a machine can check, it checks.** `sdk/scripts/launcher-check` (run
by `launcher-ci`, input `conformance`, on by default) tests a launcher
against the rules below that can be read from its files: golden snapshots,
the SDK's audience and instance guard, a draft for every fix of our own,
the README outline, AGENTS.md, SDK pins, lld, the option tests, the quit
paths in `RUNTIME_NOTES.md`, the issue forms, DRM exceptions.
`launcher-check --explain RULE` gives the reason and the fix. Run it before
every commit and before a release; an error blocks both. An exception is
an owner decision, recorded as a line `rule: reason` in the launcher's
`.rea-check-waivers` (the check shows each waiver and its reason); an agent
never adds one on its own.

## Non-negotiables

1. **Never ship the game.** Never commit, vendor, embed or publish original
   executables, DLLs, media, archives, fonts, tracks, car data or any other
   game asset, or anything copied out of them. Tests use tiny synthetic
   fixtures, never bytes taken from the game.
2. **The player's game folder is read-only.** The launcher builds a private
   runtime folder from it and changes only that copy. Every write there is a
   whole-file swap (`rea_launcher::fsx`), never a write through a hard link,
   and the runtime may never overlap the game folder
   (`rea_launcher::paths::check_separate`).
3. **Everything outside the project is the user's, and read-only.** The
   agent may read anything it needs, but it writes, moves or deletes nothing
   outside the repository it works in — game installs, other repositories,
   Wine prefixes, saves, home-folder configs, containers, the SDK checkout —
   unless the user asked for it or approved it for that specific path.
   That includes toolchains and package managers: never `mise install/use`,
   `rustup default/toolchain install`, `cargo install` or a global tool
   install without approval — select with environment variables
   (`RUSTUP_TOOLCHAIN`), skip a missing tool with a warning and say what to
   install. Tests isolate themselves with per-process environment
   (`HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `CARGO_TARGET_DIR` pointing
   into the repo's `.work/` or `target/`), never the user's real config or
   data folders.
   Scratch work goes in the repo's `.work/` or a temp folder the agent
   created (and removes). When a change outside is approved: back the file
   up first, change only what was approved, never delete by a broad glob,
   and report exactly what changed. When unsure, ask. Destroying a user's
   files is the one mistake we can't undo. The same holds for the launcher
   at runtime: it writes only its own runtime copy, its settings, the
   player's saves folder, and the registry/prefix keys it owns.
4. **Never commit a developer's absolute game path.** Code and tests take an
   explicit path or a documented `<GAME>_GAME_DIR`-style variable.
5. **Local investigation goes in `.work/`** (ignored by Git): REA snapshots,
   traces, test prefixes.
6. **Third-party components are pinned.** Fetched at package time from URLs
   with SHA-256 (`packaging/*.pins`), never committed. Every component is
   listed with its licence and source in `THIRD_PARTY.md`; check the
   licence before bundling. A file with no licence is not bundled — the
   launcher downloads it on the player's request instead (OutRun's HD pack)
   — unless the owner decides otherwise and the decision is recorded in
   `THIRD_PARTY.md` (CMR3's HD UI; CMR4's dgVoodoo2 2.80, whose upstream
   archive is gone). Such files live in a mirror release, never in Git.
7. **Exact binary identity before patching.** Executables are identified by
   SHA-256 (`rea_launcher::identify`), or by a byte signature that must
   match exactly once, with the file's digest logged. Community hash lists
   (MD5 from a patcher's script) are fine as a cross-reference, next to the
   SHA-256 table. A patch located by a
   pattern is still applied only to identified builds. Unknown builds are
   never patched by guesswork: the patch is refused with the digest and the
   issue link, and the game still runs unpatched where that is possible. Byte patches check the original bytes
   (or the already-patched bytes) at every site and are all-or-nothing
   (`rea_launcher::patch`).

## Research before building

Before writing any fix for a new game, search online for what the community
already made, and build on it. Every launcher so far stands on someone
else's work:

| Launcher | Community work it builds on |
|---|---|
| CMR3 | SilentPatch (widescreen, fixes, ASI loader), Bekoha's HD UI, the official 1.1 patch |
| CMR4 | the community 16:9 widescreen fix, dgVoodoo2 (and it keeps a player's own DSOAL `dsound.dll`) |
| OutRun 2006 | OutRun2006Tweaks (fixes, input, framerate), the Soundtrack Fix, the community HD texture pack |
| Sega Rally Revo | the community frame-pacing fix |
| Screamer | dosbox-staging, GOG's layouts |
| all Wine games | DXVK, Xidi |

Where to look: PCGamingWiki (the game's page: fixes, widescreen, controller,
known issues), the Widescreen Fixes Pack / ThirteenAG and Silent's
projects, GitHub searches for the game's name plus "fix", "patch",
"widescreen", "tweaks", ModDB/Nexus, Steam and GOG forums, speedrunning and
fan communities, ProtonDB and the WineHQ AppDB for Linux quirks.

Record what you find in `docs/RESEARCH.md`: each project, what it fixes,
its licence, whether it is maintained, and the decision (bundle, configure,
download on request, or not used and why). Only reverse-engineer and patch
ourselves what nobody has solved; credit everything in `THIRD_PARTY.md` and
the README.

## Contribute back

When our own reverse engineering finds a fix the community doesn't have (a
widescreen or frame-rate patch, a crash cause, a Wine/Proton setting that
makes the game work), share it where players look:

- **PCGamingWiki**: the game's page (fix boxes, widescreen, controller,
  Linux sections). Describe the fix as edits a player can make — offsets
  with original and new bytes, INI keys, registry values — plus the exact
  executable it applies to (SHA-256, version). Cite our repository. Never
  attach or link game files.
- **Upstream projects** we build on (SilentPatch, OutRun2006Tweaks,
  dosbox-staging, DXVK, Xidi): an issue or pull request when the fix
  belongs in their code.
- **ProtonDB / WineHQ AppDB**: a report with the settings that work.

These are public posts under the owner's name, so the agent only drafts
them: write each draft in `docs/CONTRIBUTIONS.md` (target page or project,
text ready to paste, evidence, status) and tell the owner. The owner
reviews and posts. Only publish fixes verified in the real game.

**Every fix of our own gets a draft**, in the same piece of work that
ships it: a `RESEARCH.md` row decided **Our own** cites its draft id (C1…),
and a byte patch, hook DLL or preload library has one too. An unverified
fix is a draft marked "needs verification"; one that can't be shared says
`no draft: <reason>` in its row. DiRT Rally and CMR1 shipped their own
fixes with an empty `CONTRIBUTIONS.md` (`launcher-check`: `own-fix-draft`).

## Product rules

**Who it's for:** players who are not technical. They download one package,
point it at their game, and press Play. Every decision below follows from
that: if a step could be done by the launcher, the launcher does it.

- **Bundle first.** Everything the game needs — libraries, runtimes,
  wrappers, fixes, config — ships inside the package whenever its licence
  allows (check the licence; Microsoft's redistributables usually allow it).
  Bundled beats downloaded: it works offline, can't disappear upstream and
  is the version we tested.
- **Then a button.** Only when bundling isn't allowed (no licence, or the
  author forbids redistribution) or the item is huge and optional (an HD
  texture pack): one button in the window (and a `--download-…` flag) that
  fetches the pinned file, verifies it and installs it.
- **Never a manual step.** No "download X from this site", "unzip into the
  game folder", "install winetricks", "run this command". If a fix truly
  can't be automated, the launcher works without it and the README says
  what the player loses.

- **Zero manual steps.** The player never downloads, unpacks, copies or
  configures anything by hand that the launcher could do: components are
  bundled or fetched by the launcher (`docs/playbooks/community-components.md`),
  configuration is generated, optional packs download with one button.
- **Self-contained.** The player installs the package and points it at the
  game. No third-party patchers, winetricks commands typed by hand, DLL
  overrides, compatibility modes, codec packs or registry edits.
- **Windows first, easy on Linux.** A Windows build that also runs under
  Proton or Wine with nothing extra, plus an AppImage with a portable Wine
  (or a native Linux build when the game runs natively, like Screamer).
- **Reuse open source.** When a maintained project already fixes something
  (SilentPatch, OutRun2006Tweaks, Xidi, DXVK, dgVoodoo2, dosbox-staging),
  bundle and configure it. Write our own patch only when nothing exists.
- **The modern experience is the default; the original stays one click
  away.** Offer an "Original" look that runs the game as shipped, a
  recommended look picked from the player's screen, and a Steam Deck look.
- **The game's own settings stay the game's.** Write only the keys the
  launcher owns; keep comments, unknown keys and the player's choices. A
  game that needs a complete config gets it seeded once, when the file is
  missing; after that each launch writes only the keys whose option is set
  in `launcher.cfg`. When an option goes back to unset, the launcher
  removes or restores a key only while it still holds the value the
  launcher wrote (`rea_launcher::ini` owned values) — never a value the
  player chose in the game. Unset means "keep the game's value" — or, where the
  owner decided the launcher's defaults should always apply (OutRun's
  Coast to Coast look), "the launcher default", documented in the
  `launcher.cfg` template. The first run
  saves the recommended look's values explicitly, which is how the default
  becomes the recommended experience.
- **Saves are sacred.** Never delete a save, and never tell players to
  delete a folder that holds saves (a Wine prefix's Documents) without
  telling them to back up first. When the launcher keeps a saves folder
  apart from the runtime (a `saves` setting), mirror with recorded state
  (`rea_launcher::saves`): a save the player changed wins, the runtime wins
  only when it alone changed. When the game writes saves only in the
  runtime or in its own user folder, say where in the README and never
  overwrite them on sync (`Policy::game_written`).
- **No unverified options.** An option reaches the settings window only
  after its effect has been seen in the real game. A preset (the Steam Deck
  look) made only of individually verified values may ship before it was
  tried on the device; record the device check as pending. Record what was checked
  and what was not in `docs/RUNTIME_NOTES.md`.

## Robustness and support

- **Hostile paths.** Test install, game and save paths with spaces,
  apostrophes and non-ASCII letters (`C:\Users\José\Jogos d'Ação`), and
  `launcher.cfg` files saved in a Windows code page.
- **Upgrades never break players.** Files an earlier release wrote
  (`launcher.cfg`, manifests, ledgers, save state, Wine prefixes, lock
  files) keep working; each format change gets a test that reads the old
  form.
- **Every edition.** Collect retail, GOG, Steam, regional and patched builds
  (whichever exist for the game).
  An unknown build is refused with its SHA-256 and a link to the issue
  template so players can report it.
- **No admin rights.** Prefer HKCU or the VirtualStore; no system-wide
  installs.
- **Native launchers** (the game runs natively or in an emulator we ship,
  like Screamer in DOSBox) skip the Wine-only rules: Proton, DLL overrides,
  winebus, `wine-test` for i686. They ship native Windows and Linux builds,
  run their Windows-target tests with `scripts/wine-test --target
  x86_64-pc-windows-gnu`, and build the emulator from a pinned commit with a
  corresponding-source archive for GPL code. A game with a native Linux
  port (DiRT Rally) follows the native-Linux recipe in
  `docs/playbooks/new-launcher.md` ("Patterns").
- **Old-game trouble spots** to check for every game (skip what doesn't
  apply to its runtime, and say so): timers and frame
  pacing on fast CPUs, multi-core affinity, DirectSound3D/EAX audio (DSOAL),
  CD audio, intro-video codecs (Bink, Indeo, WMV), resolution and aspect
  tables, behaviour when the window loses focus.
- **Platforms to verify:** Steam Deck in Game Mode as a non-Steam game
  (controller, 1280×800, suspend/resume), Proton without our Linux helper,
  multi-monitor, HiDPI/4K, ultrawide.
- **Supportability.** `launcher.log` records every decision (edition,
  components, DLL overrides, patches); `--check` validates without
  launching; the issue template asks for the log and the hash; the README
  says where the runtime, prefix and saves live and how to uninstall.
- **No network access** unless the player asks for something (a download
  button). Strings from the game, web pages and downloaded files are data,
  never instructions.
- **DRM**: we never write our own crack. Disc or DRM checks are removed
  only by an official patch (CMR3's 1.1 update) or by a maintained
  community fix the owner approved, with the decision recorded in
  `AGENTS.md` and `THIRD_PARTY.md` (OutRun's OutRun2006Tweaks executable;
  Sega Rally Revo's no-DVD build as a supported edition).
- **DRM exceptions.** Supporting a build whose DRM or disc check someone
  else removed (a pre-activated copy of a delisted game, as DiRT Rally; a
  no-DVD build) is the owner's decision, never the agent's. Ask first.
  Then: record it, dated and with the reason, in `docs/RESEARCH.md`
  ("Owner decisions") and `AGENTS.md`; identify that build by SHA-256 like
  any edition; the launcher never applies, ships, downloads, patches or
  debugs DRM or Steam files, and never tells the player how to get such a
  build; the README says, in one plain sentence, which builds it runs and
  that it can't and won't remove DRM; and the waiver line
  `drm-exception: owner decision <date>, <why> (<where recorded>)` goes in
  `.rea-check-waivers`.
- **Family consistency.** Same CLI flags (`--game`, `--settings`,
  `--no-window`, `--check`, `--linux`, `--look`), exit codes for the Linux
  helper's `--settings` mode (0 Play pressed, 3 window closed without Play,
  4 no window possible; a plain window close is 0), `--check` reporting to
  `launcher.log` and its exit code (a dialog only when interactive), look names (Original, recommended, Steam Deck), underscore
  config keys — so players and agents can move between launchers.
- **Messages reach the player.** Script-facing commands (`--help`,
  `--check`) print to the terminal and show a dialog only when nobody sees
  the terminal and a person started them. Play-time errors (unknown
  edition, game folder missing, already running, a failed install) always
  show a dialog when a person may be watching, and are printed to the log
  too. Under Wine stdout always goes to a log nobody reads, so it never
  counts as a terminal. `rea_launcher_win32::report::audience` (from the
  parsed flags, `report::Run`) decides who is watching, and
  `report::fatal`/`note` show it; never write `interactive = !no_window`
  or `!linux` yourself: the Linux helper starts the game run with
  `--no-window --linux` for a player in front of the screen, and that
  mistake hid errors in CMR3, CMR4 and DiRT. A native Linux launcher shows
  its errors before the window with `rea_launcher_ui::report::fatal`.
- **Action flags and one instance.** An action flag (`--download-*`,
  `--check`, `--setup`) while another launcher instance is open is reported
  and exits non-zero, never a silent exit 0. `--check` takes the instance
  like any action. Decide it with `rea_launcher_win32::instance::is_action`
  (`ACTION_FLAGS` plus the launcher's own); never a literal `false`
  (`launcher-check` rule `action-flags`).
- **A bad command line** (an unknown flag, a bad value) prints the error
  and the usage and exits 2 (`report::BAD_COMMAND_LINE`). It is reported
  like a command (`report::Run::bad_command_line`): a modal dialog from a
  terminal would hang a script with a typo (rule `bad-command-line`).
- **A GUI program on Windows.** `src/main.rs` starts with
  `#![cfg_attr(windows, windows_subsystem = "windows")]`; commands reach
  the terminal through `console::attach_parent` (rule
  `windows-subsystem`). A console build opens a second, black window on a
  double click and exits 3 when its window is closed.
- **Owner checkpoints.** Stop and ask rather than guess on licence-less
  assets, official art, DRM, and anything public (push, release, posts).
- **Screenshots and video** of the game in the README are fine (they show
  the launcher at work, not game files to play); keep images under ~1 MB
  (JPEG), host videos outside the repository, list them in
  `THIRD_PARTY.md`.
- **Screenshots follow the window.** Any change to the settings window's
  look or layout retakes the README screenshots in the same piece of work:
  headless, the window under Wine (or natively) on a private virtual
  display the agent starts and stops itself, pointed at a real game install
  (read-only) so the Play tab shows a detected edition, saved as JPEG
  ≤ ~1 MB under `docs/images/` with the names the README uses.
- **README showcase.** A "problems this launcher solves" list, screenshots of
  the window, a slot for the owner's gameplay video, credits.

## Code

- Rust for our code (see "Rust first"); edition 2024. `unsafe` and Win32 FFI only inside
  `rea-launcher-win32`, one narrow `win32.rs` module, or a small hook crate
  at an FFI boundary the launcher doesn't own: a DLL injected into the game
  (CMR4's close hook) or a library linked into a third-party engine
  (Screamer's hooks in DOSBox).
- General solutions first: find the one place a whole class of cases goes
  through and fix it there.
- Use the SDK for everything it covers. Never copy SDK code into a launcher;
  if it lacks something, keep the local code and propose the change upstream.
- Host-testable logic gets unit tests before its milestone commit: edition
  detection, patch tables, runtime planning, config and INI editing, form
  state. Shell helpers get tests in `linux/tests/`.
- Conventional Commit subjects. Commit at working milestones, only after the
  full check passes (fmt, tests, shell tests, Windows-target clippy, deny).
- Every behaviour change gets a CHANGELOG "Unreleased" entry; every change
  that still needs a real-game check gets a line in `docs/RUNTIME_NOTES.md`.

## Settings window and CLI

The player meets the launcher through its window and its command line, so
these rules are firm:

- **Write for people, then humanize.** Everything a player reads — the
  README, window labels and descriptions, error messages, `--help`, release
  notes, issue templates — is plain language, then passed through a
  `humanizer` skill (strip AI tells: inflated claims, filler, "not X but Y",
  forced triads, dash-heavy prose) before it ships. Keep facts exact; the
  humanizer changes wording, never what the text says.
- **Design with a design skill.** Any change to the settings window's look
  or layout goes through a design skill (`frontend-design`, then a review
  against `web-design-guidelines`), working within the SDK theme
  (`rea-launcher-ui`). Don't hand-tweak colours, spacing or wording
  without it.
- **Give the window the game's identity.** Each launcher's window carries
  imagery that says which game it is: CMR3 shows a photo of Colin McRae,
  Screamer the trilogy's box art, OutRun a sunset coast with a red
  convertible (Screamer's box art is used by a recorded owner decision).
  As part of the design work, research images online and pick
  one or two that fit the window's hero area and palette, in this order of
  preference:
  1. Freely licensed photos of the real subject: driver, car, track or
     location (Wikimedia Commons and similar; CC BY, CC BY-SA, CC0, public
     domain).
  2. Official art (box art, press kits) only with the owner's recorded
     decision, as CMR3 recorded for its HD UI.
  3. An original illustration made for the launcher.

  Never take images out of the game's files. Record every image in
  `THIRD_PARTY.md` (file, author, licence, source URL), and show the credit
  in the window when the licence asks for attribution. Resize to what the
  window needs (JPEG, a few hundred KB at most).
- **Accessibility comes first.**
  - Text and controls meet WCAG AA contrast; each launcher keeps a test of
    its palette (`Palette::contrast_problems`).
  - Everything works with mouse, keyboard and gamepad alone, with a
    visible focus ring.
  - Readable text: body text at least 13 pt, nothing under 11 pt, and no
    small text over a photo without a solid backing; nothing conveyed by
    colour alone. Control borders reach 3:1 against their background.
  - A disabled control says why.
- **No confusing options.**
  - Every option has a plain-language label and a one-line description of
    what the player will see change.
  - An option that has no visible effect, overlaps another, or needs
    technical knowledge to choose is removed or folded into a look.
  - Defaults are the recommended experience; the player should be able to
    press Play without touching anything.
- **The window and the command line are equals.** Every window option has a
  CLI flag and a `launcher.cfg` key with the same name and values, and every
  flag that changes the game has a window control (launcher-only flags like
  `--check` or `--no-window` excepted). Location keys (`runtime`) may be
  launcher-only. Looks have a `--look` flag that sets all of the look's
  values. One-off actions (Test rumble, Remove pack) need no flag, but a
  download does (`--download-<pack>`). Options only the Linux helper reads
  (the gamescope scaler) are keys with a window control; the helper reads
  the key. For free-form values (any W×H, any frame cap) the window
  may offer a curated set, but it shows and keeps any value saved from the
  file or the command line. The game must be fully playable
  without the window (`--no-window`, saved settings plus flags), and fully
  configurable from the window without ever touching the command line.
  Declare the options once in one table (`rea_launcher::cfg` option specs:
  key, flag, values, label, description) and build the parser, `--help`,
  the template file and the window rows from it; a parity test fails when a
  key is missing from either side.
- **Every option is tested end to end.** For each window option and CLI
  flag, a test proves the wiring: control → `launcher.cfg` key → the game
  file, registry value or environment it changes, and the flag → the same
  key. Window behaviour is tested headless with `egui_kittest` (find
  controls by label, click, check the saved settings). CLI tests cover
  `--help`, every flag, bad values and their messages. When the window
  code only builds for Windows, the GUI tests run under `scripts/wine-test`
  (the harness works there, headless).

## Clean exit, single instance

Old games often ignore the ways modern systems ask a program to quit, and a
launcher that leaves processes behind clutters the player's system and
blocks the next start. CMR4 ignored Alt+F4 and window-close requests until
its close hook.

- **Every way of quitting works** and is tested for every game: the game's
  own Quit, Alt+F4 / the window's close button (`WM_CLOSE`), the Steam
  "Stop" button, `SIGINT`/`SIGTERM` to the Linux helper (`<id>-play stop`,
  Ctrl+C), and gamescope being closed. If the game ignores close requests,
  the launcher makes it honour them (a close hook in the game's message
  loop, as CMR4 does), never by asking the player to kill it.
- **Nothing is left behind.** After any of those, within a few seconds: no
  game process, no launcher, no `wineserver`, no gamescope, no emulator. The
  Linux helper runs the game in its own session and ends the whole group,
  then `wineserver -k` for its prefix; the Windows launcher waits for the
  game and closes every handle.
- **One instance.** A second launcher start doesn't open a second window or
  a second game: it brings the existing window forward (or says the game
  is running) — `rea_launcher_win32::instance::single` on Windows, with
  `AlreadyRunning::second_start(action, Window::Own(WINDOW_TITLE))` (exit
  0, or 5 when refused); `rea_launcher::instance::single` (a lock file) in a
  native Linux build, which the helper's session lock doesn't cover when
  the binary is started directly. Play is refused while the game from this
  runtime is still running (`process::find_running` on the runtime's exe).
- **The launcher's window title differs from the game's.** A shared title
  made CMR3's second start find the game's window, and test scripts that
  wait for the game's window found the launcher's.
- **Test it:** the shell tests cover stop/interrupt/lock with fakes; a
  real-game, env-gated test runs the game headless (`gamescope --backend
  headless`), sends each close request, and checks that the process list
  is clean afterwards. Record the results per game in
  `docs/RUNTIME_NOTES.md`.

## Clean up after yourself

Builds and tests leave gigabytes behind (`target/` with host, i686 and
x86_64 builds, Wine test prefixes, packages, scratch files, containers). After
a successful release — and at the end of any long session — the agent runs
the clean-up routine so the user's disk doesn't fill up:

1. **Preview first.** `sdk/scripts/clean` (dry run by default) lists what it
   would remove and how big it is: only build output inside the repository
   that Git ignores and doesn't track — `target/` (including
   `target/wine/`), `dist/`, and `.work/` entries the agent created during
   this task.
2. **Keep what matters.** Never remove investigation evidence the user may
   want (REA snapshots and notes in `.work/` from earlier sessions), the
   user's game installs, saves, prefixes, other repositories, or shared
   caches (`~/.cargo`, `~/.rustup`, Docker images the user already had).
3. **Remove** with `sdk/scripts/clean --yes`: explicit paths, never a broad
   glob, never `git clean -fdx`, never following a symlink out of the repo.
4. **Outside the repo**, remove only what this task created and recorded:
   temp folders, test containers and images pulled just for the task,
   throwaway Wine prefixes, leftover processes (see "Running the real game
   in a test").
5. **Report** what was removed, how much space it freed, and what was kept
   on purpose.

## Windows builds link with LLD

GNU ld 2.47 mis-links the import tables of Rust Windows binaries (i686 and
x86_64): some `kernel32` imports end up outside every import descriptor,
Windows never fills them, and the program crashes before `main`. It hit Sega
Rally Revo's and Screamer's builds, and Screamer shipped one. Every launcher
links its Windows targets with LLD (`.cargo/config.toml`,
`-C link-arg=-fuse-ld=lld`, in CI too, including MSYS2 jobs), and CI checks
that the built exe's imports are complete and runs it under Wine.

## Rust first

Rust is the language of the launcher and of the SDK. It runs the same on
Windows, under Wine and on Linux, cross-compiles to 32-bit Windows from
Linux, and its type system plus `cargo test` let one host machine check
nearly everything.

- Logic of any size — parsing, planning, patching, file sync, downloads — is
  Rust, in a library crate that builds and tests on the host.
- POSIX `sh` only for the thin Linux helper (`linux/<id>-play` on
  `rea-play.sh`) and packaging glue; keep it short and tested
  (`linux/tests/`, run under dash in CI).
- No Python, Node or PowerShell in what ships or in the build path. C or
  assembly only at an FFI boundary we don't own (a hook DLL, an emulator
  patch), behind a narrow interface.
- Keep `main.rs` and the window code thin: they call planning functions
  that live in the library. The audits found 400-line `run()` functions in
  `cfg(windows)` code that no test ever executed.

## Tests that catch regressions

What this family learned, bug by bug:

- **Reproduce first.** Every bug fix starts with a test that fails on the
  old code, then the fix. Check that the test really fails without the fix
  (temporarily revert it) — several "regression tests" here passed either
  way until someone checked.
- **Test behaviour, not the implementation.** CMR3's xdelta bug survived
  because the unit test asserted the wrong argument list. Where a real tool
  is involved, test against the real tool when it is installed (skip, don't
  fail, when it isn't): the xdelta3 round trip caught it at once.
- **Test the wiring, not only the parts.** Each layer can be correct while
  the whole is wrong. Test option → `launcher.cfg` → generated game file /
  registry / environment end to end (`plan()`), plus golden snapshots
  (below), plus headless GUI tests for the window.
- **Write tests as player stories** for anything with state: "the player
  restores an older save backup", "the session is killed before write-back",
  "saves copied from another PC with old timestamps", "a second launch while
  the game runs". The save-mirror regression only showed up in such a story.
- **Run Windows code on Windows semantics.** Case-insensitive names, `\\?\`
  prefixes, read-only attributes, rename over an open file: four SDK bugs
  appeared only under `scripts/wine-test`. Never rely on clippy alone for
  `cfg(windows)` code. Don't use `#[should_panic]` (a panic aborts the whole
  test binary under Wine); assert on a returned error instead.
- **Hostile inputs:** paths with spaces, apostrophes and accents; files in a
  Windows code page; CRLF; a BOM; truncated or malformed files with useful
  error offsets; old formats from every earlier release.
- **Fakes at the boundary, not mocks of our own code:** a fake HTTP server for
  downloads, fake `wine`/`gamescope`/`cargo` on `PATH` for shell tests,
  synthetic PE/LE images for patches, temp folders for file sync.
- **Shell is tested too**, under dash and busybox as well as bash, with
  globbing and quoting cases (`*` in a value, an apostrophe in a path).
- **Measure, then aim.** Run `cargo llvm-cov` now and then to find untested
  modules; library code should be close to fully covered, and every public
  function has a test. Don't chase a percentage on `main.rs` — make it thin
  instead.
- **Review independently.** After a batch of changes, a reviewer who did not
  write them reads the diff looking for regressions (this caught the
  newer-wins save loss and the Wine DPI window bug here).

## Golden snapshots (regression guard)

Every launcher keeps snapshot tests of everything it produces, so an SDK
upgrade or a refactor can't change behaviour silently:

- `--help` and the generated `launcher.cfg` template;
- for each look (Original, recommended, Steam Deck) and for a few option
  combinations: the game files it writes (INI/CFG bytes), registry values,
  environment (`WINEDLLOVERRIDES`, DXVK/vkBasalt variables), command-line
  arguments and the patch sets it would apply.

Keep the planning in a host-testable function (`plan(settings, game) ->
Plan`) and compare its output with files under `tests/snapshots/`, with
`rea_launcher::snapshot!` (it stores the crate version as `<version>`; a
snapshot that held the version broke three tag builds). The scaffold
ships `tests/snapshots.rs`; a launcher without it fails `launcher-check`. A
snapshot change is reviewed like code: it must match an intended change or
an SDK **Migration** item, never be regenerated blindly.

## Testing (soft rules)

- **Headless by default.** Whoever runs the agent — the SDK's owner or a new
  developer building their first launcher — should never see windows pop up
  out of nowhere. Automated tests, probes and game runs are headless: clear
  `DISPLAY`/`WAYLAND_DISPLAY` for Wine runs, switch Wine's crash dialog off
  in a private test prefix (`scripts/wine-test` does both), and run the real
  game in a headless compositor (`gamescope --backend headless`) with a
  PipeWire null sink. Prefer evidence that needs no eyes: logs, exit codes,
  the process table, files written, headless screenshots
  (`gamescope --backend headless` + a capture) the user can look at later.
- **Visible when it matters — and announced.** Open a window only when the
  check needs a person (judging how the settings window or the picture
  looks, a controller test, anything the user must click) or when a
  headless run would prove nothing (a bug that only shows on a real
  display, focus or multi-monitor behaviour). Tell the user first: what
  will open, why, roughly how long, and what to look for; wait until they
  are ready; close it afterwards and clean up its processes.
- **Exercise Windows code, not only lint it.** Link the i686 build and run
  the Windows-target tests under Wine with `scripts/wine-test`; clippy alone
  never runs `cfg(windows)` code.
- **Running the real game in a test** (allowed, from the player's own
  install outside the repo; never modify it):
  - opt-in only: the script does nothing and exits 0 unless a variable
    such as `<ID>_E2E=1` asks for it, so a loop over the test folder never
    starts the game (CMR3's e2e.sh opened a visible 4K window on the
    owner's desktop, 2026-11-02; `launcher-check` rule `real-game-gated`);
  - inside the container that has Wine/gamescope, with a finite timeout
    *inside* it and a supervisor that waits for the actual game process —
    never `timeout` around `distrobox enter`, never a live launcher piped
    into `head`: killing the outer wrapper orphans Wine children;
  - headless (`gamescope --backend headless`), a private PipeWire null sink,
    a throwaway Wine prefix under `.work/`;
  - record each task-started PID (name, start time, parent, container);
    after the run, check the process table by name **and** by command
    line/environment (`/proc/*/environ` for the test's `WINEPREFIX` or
    `STEAM_COMPAT_DATA_PATH`), including `Z` (defunct) entries — a defunct
    leader can still have live threads (`/proc/<pid>/task/*/status`);
    signal exact task PIDs (TERM, then KILL) until the whole thread group
    is gone. `wineserver -k` or a timeout exiting is not proof of cleanup;
  - never restart the container or stop another game to hide a leftover;
    report it.
  (From distrobox-gaming's AGENTS.md, which keeps these rules for the
  owner's machine.)
- **Secrets stay out of test output.** The user's environment may hold
  credentials (cloud keys, tokens). Start test processes with a scrubbed
  environment (`env -i` plus only what the test needs), never write or
  print `env`, `printenv` or `/proc/*/environ` contents — to find a task
  process by environment, test for one variable without printing values —
  and before finishing, scan scratch output for key patterns (paths only).
  Other processes' **command lines** are secrets too: another session's
  `docker exec -e …` carries the owner's keys, and a harness's
  `ps -eo pid,stat,args` printed them into a log (2026-11-02). Find
  processes with `pgrep -x NAME` (pids only) or `ps -o pid,stat,comm`;
  match `/proc/PID/cmdline` against our runtime path without printing it.
  Never `ps aux`/`-ef`/`-o args`, `pgrep -a`, or `cat /proc/*/cmdline`
  (`launcher-check` rule `process-listing`; harnesses in `.work/` too).
  A leak that stayed on this machine is scrubbed and reported; nothing
  secret ever goes into a commit, release, issue or post.
- **The real game stays outside the repository** and outside ordinary host
  tests. Env-gated tests (`<GAME>_GAME_DIR=... cargo test -- --ignored`)
  may use it.

## Repository shape

```
AGENTS.md  CHANGELOG.md  README.md  THIRD_PARTY.md  LICENSE-MIT  LICENSE-APACHE
Cargo.toml  Cargo.lock  deny.toml  .gitignore  (.cargo/config.toml when the linker needs it)
crates/<id>-launcher/      src/{main,config,form,ui,edition,runtime,...}.rs
                           (or the crate at the root; plus hook DLL crates if needed)
linux/<id>-play            sources sdk/rea-play.sh
linux/tests/*.sh
packaging/                 pins, allowlist, desktop entry, icon, package script
sdk/                       vendored by scripts/sdk-sync (do not edit)
docs/RUNTIME_NOTES.md      what was verified in the real game, and what wasn't
.github/workflows/build.yml  calls the SDK's launcher-ci / launcher-release
```
