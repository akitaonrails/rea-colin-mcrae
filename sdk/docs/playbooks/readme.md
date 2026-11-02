# Playbook: the launcher README

Every rea launcher's `README.md` follows the same outline, so a player who
used one launcher finds their way in the next, and an agent knows what to
write and where. `templates/launcher/README.md` is this outline with
placeholders; `scripts/new-launcher` fills in the names and the agent
fills in the rest (`docs/playbooks/new-launcher.md`, Phase 6).

## Who reads it

1. **Players first.** They are not technical. They want to know whether
   the launcher fixes their problem, what to download, where to point it,
   and what to do when it doesn't work. The top half of the README is for
   them, from the title down to Uninstall.
2. **Developers second.** Building, testing, the SDK, contributing. The
   bottom half, from "For developers" down.

A player should be able to stop reading at Uninstall and have everything
they need. Nothing in the top half requires reading the bottom half.

## Tone

- **Player half:** plain words, short sentences, second person ("your game
  folder"). Say what the player sees, not how the code does it: "the game
  no longer stutters", not "patches the frame pacer at 0x4A1F30". No
  offsets, hashes, function names, crate names, env vars, Wine internals
  or formulas. Name a tool only when the player meets it (Steam, Proton,
  gamescope, the AppImage) or in a credit. Bold for the buttons and looks
  the player clicks (**Play**, **Browse…**, **Steam Deck**).
- **Developer half:** technical terms are fine, but still short. Deep
  reverse-engineering write-ups live in `docs/` and are linked, not
  pasted.
- **No promises.** Describe only what is verified in the real game
  (`docs/RUNTIME_NOTES.md`). Something built but not seen working is not
  in the README, or is in **Known limitations** as "not yet checked".
  Untested platforms are said to be untested. No marketing words
  ("seamless", "rock-solid", "blazing").
- **No stale facts.** No release versions in prose, captions, links or
  file names (they go out of date with the next tag); no "the screenshot
  predates…" notes (retake the screenshot); no references to versions
  that don't exist yet.

## The outline

Use these headings, in this order, with this exact wording (the game's
name goes only in the title). A section marked *optional* is left out
when it doesn't apply; every other section is always there, even if
short.

| # | Heading | Half | Optional |
|---|---|---|---|
| 1 | `# <Game> Launcher` + pitch | players | |
| 2 | Status note (a `>` quote, no heading) | players | when everything shipped is verified on every platform |
| 3 | Showcase video (no heading) | players | until the owner records one (leave the comment slot) |
| 4 | `## Problems this launcher solves` | players | |
| 5 | `## Screenshots` | players | |
| 6 | `## What you need` | players | |
| 7 | `## Download and install` | players | |
| 8 | `## First run` | players | |
| 9 | `## The settings window` | players | |
| 10 | `## Gamepad` | players | when the launcher does nothing for pads |
| 11 | `## Options` | players | |
| 12 | `## Where things live` | players | |
| 13 | `## Troubleshooting` | players | |
| 14 | `## Known limitations` | players | |
| 15 | `## Uninstall` | players | |
| 16 | `## For developers` | developers | |
| 17 | `## How it works` | developers | yes |
| 18 | `## Built on rea-launcher-sdk` | developers | |
| 19 | `## Contributing` | developers | |
| 20 | `## Credits` | both | |
| 21 | `## Legal` | both | |

Subheadings (`###`) are used only where listed below. No numbered
headings ("1. The game you need"): the numbers break when a section moves.

### 1. Title and pitch

`# <Game title> Launcher` (the game's full name as players search for it;
a trilogy names the trilogy). Then 2–4 sentences:

- what it does: plays **your own copy** of the game on Windows 10/11, the
  Steam Deck and Linux (Proton, Wine or the AppImage — native launchers
  say "native Windows and Linux builds");
- the promise: point it at your game, press **Play**; everything the game
  needs is bundled and set up; no mods, patchers, winetricks, DLL
  overrides or registry edits by hand;
- the safety line: your game folder is only read, never changed; the
  launcher plays from a private copy.

Then one line: "This project contains no game files: you need your own
copy of the game."

### 2. Status note

A `>` block, 2–4 lines: what was played end to end and where, what has not
been tested yet (native Windows, Steam Deck, Proton without the helper),
and a link to the issues asking for reports. No version number. Remove it
once the platform list in `docs/RUNTIME_NOTES.md` is all verified.

### 3. Showcase video

The owner records it. A poster image linked to the video, both hosted
outside the repository (the owner's S3 bucket, never Git, never Git LFS):

```markdown
[![<what the video shows, as alt text>](https://…/showcase-poster.jpg)](https://…/showcase.mp4)

*<One sentence: what is shown, on which platform. Click to watch (m:ss, with sound).>*
```

The caption gives the length and says what the viewer sees; no version.
List the video in `THIRD_PARTY.md` (it shows the game). Until there is one,
keep the template's comment slot so the owner knows where it goes.

### 4. Problems this launcher solves

The heart of the README: why a player wants this. A table:

```markdown
| Problem | What the launcher does |
| --- | --- |
| <what goes wrong on a modern PC, as the player sees it> | <the fix, as the player sees it; credit the community project inline> |
```

- One row per problem, one or two sentences per cell; 5–12 rows. Order
  from "won't start" to "nice to have".
- Every row is verified in the real game. Something not yet fixed is not a
  row: it goes to **Known limitations**.
- Credit the community project in the row that uses it ("…SilentPatch's
  widescreen fix, bundled").
- Close with the safety row if the game folder used to be patched in
  place ("Patching the game folder risks your copy → only read; private
  copy").

### 5. Screenshots

- A table of the settings window's tabs (`| Play | Picture | Controls |`,
  one image per cell), then optionally one gameplay image or a
  before/after pair (CMR3's Cinematic comparison is the model).
- Files in `docs/images/`, **JPEG, under ~1 MB each** (aim for 100–400 KB;
  resize the window shots to about 1600 px wide), named
  `launcher-<tab>.jpg`, `gameplay.jpg`, `before-after.jpg`. No PNG
  screenshots. A large before/after or gameplay image may be hosted next
  to the video instead.
- Alt text says what the image shows ("The Play tab with the game folder
  and the four looks").
- Screenshots show the current window. When the window changes, retake
  them in the same change.
- Game screenshots are fine (they show the launcher at work); never images
  extracted from the game's files. List them in `THIRD_PARTY.md`.

### 6. What you need

- The game, the edition(s) and the version the launcher supports, in words
  ("the PC release, updated to the official 1.1 patch"; "GOG or the
  original CDs"), and the file the player will recognise (`Rally_3PC.exe`).
- What to do if their copy is older (an official patch: link to where
  players get it).
- What happens with an unknown version: refused with its SHA-256 (or, if
  the game still runs unpatched, played without the fix) and a link to the
  repository's unknown-edition issue form. "Report the code, never the
  executable."
- SHA-256 tables do not belong here (they are in `docs/RESEARCH.md`);
  a `<details>` block is acceptable if players need to check by hand.

### 7. Download and install

Opening line with the download link, always the latest release, never a
tag:

```markdown
Download from the [latest release](https://github.com/<owner>/<repo>/releases/latest):
```

Asset names use `<version>`: `<Name>Launcher-<version>.zip`,
`<Name>Launcher-<version>-x86_64.AppImage`. Never `v0.1.0`. Then these
subsections, in this order (native launchers: Windows, Steam Deck, Linux
AppImage, Linux zip):

- `### Windows` — numbered steps: unzip anywhere (outside the game folder
  unless the launcher supports inside), keep its folders next to the exe,
  run it. Name the folders that must stay together.
- `### Steam Deck` — Desktop Mode: unzip (or keep the AppImage), Steam →
  *Add a Non-Steam Game*, Properties → Compatibility → force Proton (when
  the exe is added); back in Game Mode, the Steam Deck look is
  recommended automatically, the window works with the controller. Say
  whether Game Mode is verified.
- `### Linux: AppImage` — `chmod +x` and run; it brings its own Wine; the
  prefix path; gamescope used when installed; game folder is under `Z:`.
- `### Linux: Steam (Proton)` — the same non-Steam-game steps with the
  zip's exe; what Proton changes (its own DXVK, no helper).
- `### Linux: your own Wine` — `linux/<id>-play` from the zip, the four
  commands (window, `--play`, `stop`, `status`), and a link to
  `linux/README.md` for variables and a distrobox.

Every route ends at the same window, so don't repeat First run in each.

### 8. First run

Numbered steps, the same for every platform: **Browse…** to the folder with
`<GAME_EXE>` (under Wine your Linux files are drive `Z:`, e.g.
`Z:\home\you\Games\<Game>`), pick a look (which one is recommended),
press **Play** (or Enter, or Start on a pad). Then: how big the first copy
is and how long it takes, that later starts copy only changes, and any
one-time button (a download) with what it fetches, how big, from where,
and that nothing downloads unless pressed.

### 9. The settings window

- One short paragraph per tab: what is on it, in player words.
- A looks table: `| Look | Resolution | … | When to pick it |` with the
  family's names: **Original** (the game as shipped), the recommended look
  (the launcher's name for it), **Steam Deck**, and **Custom** (any change
  in the other tabs). Say which look the first run picks.
- How to drive the window with a pad and with the keyboard alone
  (d-pad/arrows move, A/Enter activate, LB/RB or Ctrl+Tab switch tabs,
  Start plays).
- That a greyed-out control says why.

### 10. Gamepad *(optional)*

What the launcher does for pads, a layout table
(`| On the pad | In menus | In a race |`), what the player may still have
to do once in the game's own menu, and pad tips that are verified (8BitDo
X-input mode, wake a wireless pad first). Engine internals behind it go to
`docs/`.

### 11. Options

Every option the launcher has, generated from the option table
(`rea_launcher::cfg` specs: key, flag, values, label, description) so
nothing is missing and nothing extra is listed. Check against
`<Launcher> --help` and the `launcher.cfg` template before each release.

- Opening: every option has the same name in the window, in
  `launcher.cfg` (underscores) and as a flag (hyphens); `launcher.cfg`
  location is in **Where things live**; the first run writes a commented
  file.
- One table, one row per option, in window order:

  ```markdown
  | `launcher.cfg` / flag | In the window | Values | Unset | What it changes |
  | --- | --- | --- | --- | --- |
  | `vsync` / `--vsync` | Picture: VSync | `on`, `off` | the game's (recommended: on) | Waits for the screen's refresh: no tearing. |
  ```

  "In the window" is `Tab: Label` exactly as the window says it;
  launcher-only keys (`runtime`) say "— (file and flag only)"; Linux-only
  keys (`scaler`) say so. "Unset" says what happens when the key is
  absent: the game's own value, or the launcher default where the owner
  decided so.
- Unset semantics in one sentence: unset keeps the game's value, and a
  value set and later unset goes back to the game's value unless the
  player changed it in the game since.
- Command-line behaviour: any option skips the window and plays; options
  override `launcher.cfg` for one run; only `--game` is remembered.
- The launcher switches, with the family's meaning: `--settings`,
  `--no-window`, `--check` (report in `launcher.log`, exit 0 ready /
  1 not), `--linux`, `--help`, and every `--download-<pack>`. Exit codes
  for scripts: 2 bad flag or value; with `--settings` 0 Play pressed,
  3 closed without Play, 4 no window possible.
- A `--help` dump in a `text` block is optional; if present it is pasted
  from the real binary, not typed.

### 12. Where things live

One table, platforms as columns:

```markdown
| What | Windows | Linux (AppImage, `<id>-play`) |
| --- | --- | --- |
| Settings (`launcher.cfg`) and log (`launcher.log`) | … | in the prefix, `drive_c/users/<you>/AppData/Local/<Name>Launcher/` |
| The launcher's copy of the game (runtime) | … | … |
| Your saves | … | … |
| Downloaded packs | … | … |
| Wine prefix | — | `~/.local/share/<id>-launcher/prefix` |
| `<id>-play`'s logs and lock | — | `~/.config/<id>-launcher` |
```

Then: a `launcher.cfg` next to the exe is used instead (portable); under
Proton everything is in Steam's `compatdata/<appid>/pfx`; and where the
game writes its saves — in a saves folder the launcher mirrors, or only in
the runtime copy (CMR4) — said plainly, because the uninstall depends on
it. Registry keys the launcher writes go here too.

### 13. Troubleshooting

Bulleted, bold symptom in the player's words, then what to do:

1. The unknown-version message, quoted as the launcher shows it, with the
   issue form link.
2. The 3–6 most likely real problems from testing (pad not seen, black
   screen on hybrid GPUs, crawls at 4K…).
3. **Anything else:** run `<Launcher> --check`, then open an issue with
   the [bug form](…/issues/new?template=bug-report.yml), attaching
   `launcher.log` (remove personal paths). Never attach game files.

Link only issue templates that exist in `.github/ISSUE_TEMPLATE/`.

### 14. Known limitations

What the launcher does not fix, what a player loses when a fix can't be
automated (`STANDARDS.md`, "Never a manual step"), and the platforms not
yet verified, with a link to `docs/RUNTIME_NOTES.md`. Short bullets. This
is where "still to do" items live — never in the Problems table.

### 15. Uninstall

Saves first, always:

1. **Back up your saves** — the exact folder(s), Windows and Linux; say if
   the uninstall steps below would delete them (a Wine prefix's Documents,
   a runtime copy holding the only saves).
2. Delete the launcher's folder (or the AppImage).
3. Windows: delete `%LOCALAPPDATA%\<Name>Launcher` (settings, log, private
   copy, downloads; give the size), and any registry key the launcher wrote
   (say when to keep it: if you still play the game without the launcher).
4. Linux: delete the prefix and `~/.config/<id>-launcher`, after copying
   the saves out. Proton: removing the Steam shortcut offers to delete its
   prefix, saves included.

End with: "Your game folder was never changed, so there is nothing to undo
there."

### 16. For developers

- One sentence on the toolchain: Rust (edition 2024), MinGW-w64 (i686 or
  x86_64) with lld, Wine for the Windows-target tests, `cargo-deny`; the
  `gaming` distrobox has them all. Why lld, if the launcher needs it.
- The check commands, the same list as `AGENTS.md` "Checks", in one `sh`
  block:

  ```sh
  cargo fmt --all --check
  cargo test --workspace --locked                 # host tests, the window headless
  cargo clippy --target <triple> --workspace --all-targets -- -D warnings
  cargo deny check
  for t in linux/tests/*.sh packaging/tests/*.sh; do sh "$t"; done
  cargo build --release --locked --target <triple>
  sdk/scripts/wine-test --locked --workspace      # Windows-target tests under Wine, headless
  sh <sdk>/scripts/sdk-sync --check .             # sdk/ matches the pinned SDK tag
  ```

- Moving to a new SDK: `sdk/scripts/sdk-bump vX.Y.Z` (Cargo tags, `sdk/`,
  workflow pins; prints the Migration notes) — `docs/playbooks/sdk-change.md`.
- Freeing disk space after a release: `sdk/scripts/clean` (dry run), then
  `sdk/scripts/clean --yes`.
- Golden snapshots: where they are, how to regenerate, "review every
  diff".
- The env-gated real-game test: `<PREFIX>_GAME_DIR=<folder> cargo test --
  --ignored` (read only).
- Packaging and CI in two sentences: components fetched at package time
  from pinned URLs and SHA-256 (`packaging/*.pins`); GitHub Actions (the
  SDK's reusable workflows) test, build the zip and the AppImage, and a
  `v*` tag publishes the release.
- Links: `AGENTS.md` (rules for contributors and agents),
  `docs/RESEARCH.md`, `docs/RUNTIME_NOTES.md`, `docs/CONTRIBUTIONS.md`, and
  any deep-dive in `docs/`.

### 17. How it works *(optional)*

4–6 numbered steps of what happens on Play (identify, copy, install
components, write config, start, sync saves). Useful for curious players
and reviewers; keep it plain.

### 18. Built on rea-launcher-sdk

Two or three sentences: which parts come from the
[rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk)
(runtime copy, saves, `launcher.cfg` and the option table, settings-window
widgets, `rea-play.sh`, packaging, CI), pinned by tag in `Cargo.toml` and
vendored in `sdk/` (the tag is in `sdk/VERSION` — don't repeat it in the
README), and which parts are this game's own.

### 19. Contributing

```markdown
- **About <Game>** (its editions, fixes, settings, `<id>-play`'s game
  settings, this README): issues and pull requests here.
- **Shared code** (the runtime folder, settings-window widgets, the option
  table, `rea-play.sh`, packaging, CI workflows, anything under `sdk/`):
  issues and pull requests at [rea-launcher-sdk](…/issues).
- **How to tell:** code under `sdk/` or from a `rea_launcher*` crate is the
  SDK's. If the same problem would happen with another rea launcher, it's
  the SDK's. Not sure? Open it here and it will be moved.
- Never patch `sdk/` or copy SDK code here: fix the SDK and bump the tag.
- Never attach game files (executables, saves, data) to an issue.
- Test reports are welcome: <the form>.
```

Build details belong in "For developers", not here.

### 20. Credits

A bulleted list, one line each: **who** (person or project), linked, and
what of theirs the launcher uses. It names:

- every community project the launcher bundles, configures, downloads on
  request or reimplements (`docs/RESEARCH.md` decisions other than "not
  used"), with its author;
- the window's imagery (author, licence) and the Barlow fonts (Jeremy
  Tribby, SIL OFL);
- Wine and Kron4ek's portable builds and the AppImage tools (Wine
  launchers), or the emulator (native launchers);
- the sources where the fixes were found (PCGamingWiki's page, forums);
- [rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk).

Then: "Full list with licences: [THIRD_PARTY.md](THIRD_PARTY.md)." Check
that every `THIRD_PARTY.md` entry has a credit line, and vice versa.

### 21. Legal

The game's copyright holder; "not affiliated with or endorsed by …";
contains no game files and only runs a copy you own; our code is MIT OR
Apache-2.0 (link both files); bundled parts keep their licences
(`THIRD_PARTY.md`); GPL components (DOSBox) say where the corresponding
source is.

## `linux/README.md`

Wine launchers keep a short `linux/README.md` for players who use the zip
on Linux, with these headings:

1. `# <Game> on Linux` — one paragraph: the launcher is a Windows program,
   run here through Wine or Proton; three ways to play.
2. `## The AppImage` — run it; prefix; gamescope.
3. `## Steam (Proton)` and `### Steam Deck` — the non-Steam-game steps.
4. `## Your own Wine` — `<id>-play` commands (window, `--play` with
   options for one run, `stop`, `status`), gamescope, a distrobox via
   `<PREFIX>_RUN`.
5. `## Environment` — the `<PREFIX>_*` variables table.
6. `## Where things are` — the Linux column of the main table plus the
   helper's state.
7. `## Troubleshooting` — Linux-only problems.

How the AppImage is built is developer material: the main README's "For
developers" section. Native launchers adapt the headings (`## The zip`
instead of Wine).

## Rules checklist

Before a release, check the README against this list:

- [ ] Headings and order match the outline; optional sections only left
      out when they don't apply.
- [ ] Download links go to `releases/latest`; asset names use
      `<version>`; no release or SDK version in prose or captions.
- [ ] Images are JPEG under ~1 MB in `docs/images/`, current, with alt
      text; the video and its poster are hosted outside the repository;
      both listed in `THIRD_PARTY.md`.
- [ ] Every option in the option table (and nothing else) is in the
      Options table, with the window label as the window shows it.
- [ ] Every claim is verified in `docs/RUNTIME_NOTES.md`; unverified
      platforms are named as such.
- [ ] Where the runtime, the prefix, the settings and the **saves** live,
      and how to uninstall with a backup first.
- [ ] Credits name every community project, image and font;
      "Built on rea-launcher-sdk" is there.
- [ ] Contributing routes shared code to the SDK repository.
- [ ] Issue form links point at templates that exist.
- [ ] The player half has no offsets, hashes, function names or env vars.

## Final step: humanize

The README is player-facing text (`STANDARDS.md`, "Write for people, then
humanize"). After the content is complete and checked, run the README —
and `linux/README.md` — through the `humanizer` skill. Wording only: the
facts, commands, paths, option names, links and tables stay exactly as
they are. Re-run the checklist afterwards if anything moved.

## Gap list: the five launchers

What each README must change to match this outline (read at their HEAD on
2026-11-02; Sega Rally Revo's working copy had uncommitted README edits,
which this list includes). "Rename" means the heading text only.

| Launcher | Structure and headings | Links and versions | Images and video | Missing or misplaced content |
|---|---|---|---|---|
| **CMR3** (`rea-colin-mcrae-3`) | Title "— play it today" → `# Colin McRae Rally 3 Launcher`. "The launcher" (screenshots) sits before Problems → move after, rename `## Screenshots`. Rename "Problems it solves". Fold "What it adds to the original game" into Problems rows / The settings window (it repeats them). Numbered "1. The game you need / 2. Download / 3. Play" → `What you need`, `Download and install`, `First run`. "Settings" → split into `The settings window` (looks table) + `Options`; "### Gamepad" → `## Gamepad`. "Where things are" → `Where things live` with Windows/Linux columns (prefix and `~/.config/cmr3-launcher` rows). "For developers" holds the SDK paragraph → own `## Built on rea-launcher-sdk`. | `releases/latest` ✓; asset names `…` → `<version>`. Bug link `template=bug.yml` exists, but the scaffold uses `bug-report.yml`/`unknown-edition.yml`: align the issue templates or the links. | JPEG ✓ (all < 410 KB). Video ✓. Before/after image ✓ (model). | Install has no Windows/Steam Deck/Linux subsections (Linux only via `linux/README.md`) → add all five. Options table ✓ (model, has "Unset"); reorder its columns to the outline's. For developers: add `cargo fmt`, `sdk-bump`, `clean`; `<sdk>/scripts/sdk-sync`. Credits: add Wine/Kron4ek, AppImage tools, the official 1.1 patch / DRM-free patcher authors, PCGamingWiki. Legal ✓. |
| **CMR4** (`rea-colin-mcrae-4`) | Title → `# Colin McRae Rally 04 Launcher` ✓ (capital L). Problems: grouped bullets → table; its "Still to do" HUD item → `Known limitations` (new section). "Verification status" → short status note + Known limitations. "Download" + "What you need" → `What you need` first, then `Download and install`. "Install and run" → `Download and install` with `### Windows`, `### Steam Deck` (missing; only a line under Proton), `### Linux: AppImage`, `### Linux: Steam (Proton)`, `### Linux: your own Wine`; add `## First run`. "Command line and `launcher.cfg`" → `## Options`. "Files, saves and uninstalling" → split `Where things live` + `Uninstall`. Add `## Screenshots`. | Windows step links `/releases` → `releases/latest`. Video caption and "Built on" name `v0.2.0` → drop. Options text says `widescreen` was "removed in 0.3.0", a version not in the CHANGELOG → reword. | `launcher.png` (650 KB) and `launcher-picture.png` (612 KB) → JPEG under `docs/images/`; retake (the "screenshots predate the Widescreen look's new name" note goes). `game.jpg` ✓. Video ✓. | Options: one table key/flag, window label, values, unset, what it changes (today a `--help` dump plus a partial key table without window labels). For developers ✓ (good: lld note, e2e tests) — add `sdk-bump`, `clean`. Credits ✓ (model). Legal ✓. |
| **Screamer** (`rea-screamer`) | Title "(`rea-screamer`)" → `# Screamer Trilogy Launcher`; pitch is technical (SVGA/Glide modes) → plain words. "### Problems this launcher solves" is H3 → H2, and a Problem / What the launcher does table. "Gameplay Showcase" heading → video under the pitch, no heading. "Key Features", "Engine Limitations…", "Gamepad & Button Mapping Shenanigans" are developer material in the player half (offsets, LE tables, `$F_{t-0.5}$`) → move to `docs/` (link from For developers); keep the controller layout table under `## Gamepad`. "Installation & Usage" mixes downloads and building from source → `What you need`, `Download and install` (Windows, Steam Deck, Linux: AppImage, Linux: zip), `First run`; building goes to `For developers`. "Documentation" → For developers links. "Where things live, and how to uninstall" → split `Where things live` + `Uninstall` (numbered, saves first). "License" → `Legal`. | `releases/latest` ✓; asset names `vX.Y.Z` → `<version>`. "Signed build-provenance", "every push is cross-platform tested" → For developers, or drop. | Six PNGs (330–400 KB) in `docs/screenshots/` → JPEG in `docs/images/`. Video ✓ (caption: add length). | Add `## The settings window` (looks table: Original 1995 / Enhanced / Steam Deck / Custom), `## Troubleshooting`, `## Known limitations` (the 50 Hz engine ceiling in one plain line), `## Screenshots`. Options table: add "Unset" and "What it changes" columns. For developers: fmt/clippy/deny/wine-test (`--target x86_64-pc-windows-gnu`) are in Contributing → move; add `sdk-sync --check`, `sdk-bump`, `clean`. Legal: add the trademark / not-affiliated line. Credits ✓; GPL source line ✓. |
| **Sega Rally Revo** (`rea-sega-rally-revo`) | Title ✓; the "Built on" line in the pitch → keep its own section instead. "### See it running" → video under the pitch, no heading. Lone Play screenshot under the pitch → `## Screenshots` table. "What it fixes" → rename `Problems this launcher solves` (table ✓). "Install" → `Download and install` with subsections in the canonical order (today: Linux first, bold paragraphs). "All options" → `Options`; its "### Command line" stays inside Options (no `###`, or keep as the `--help` block). "Where things are" → `Where things live`. "Building" → `For developers`. | Install links `/releases` → `releases/latest`. Picture screenshot note says it "predates v0.2", a version not yet tagged (latest v0.1.1) → retake, drop the note. | Three PNGs (≈140–155 KB) → JPEG. Video ✓ (model caption). | What you need: SHA-256 table in the player half → `docs/RESEARCH.md` or a `<details>`. Add `## Known limitations` (ultra-wide aspect not checked on a real screen; Championship 62.5 fps cap; native Windows frame limit), `## Built on rea-launcher-sdk`. For developers: add `sdk-sync --check`, `sdk-bump`, `clean` (snapshots paragraph ✓ model). Troubleshooting/Uninstall/First run ✓ (models). Credits ✓, Contributing ✓, Legal ✓. |
| **OutRun 2006** (`rea-outrun-2006`) | Title "— play it today" → `# OutRun 2006: Coast 2 Coast Launcher`. Trademark line under the pitch → `Legal`. "The problems it solves" → rename. "What's in the window" sits before install → after `First run` as `The settings window` (its look list → looks table). Numbered "1. … 4." headings → `What you need`, `Download and install`, `Options`, `Troubleshooting`. Steam Deck subsection is last → second. Add `## First run` (now inside Windows steps). "Settings" → `Options` (table ✓ model; add "Unset" column — it has defaults in Values). "Credits and licence" → `Credits` + `Legal`. Contributing carries lld/pins/CI paragraphs → move to `For developers` ("Building" → rename). | Status note "(v0.1.0)" and install steps `OutRunLauncher-v0.1.0.zip` / `…-v0.1.0-x86_64.AppImage` → drop/`<version>`; links `/releases` → `releases/latest`. "Built on" names `v0.2.0` → drop. | Five PNGs (≈300–320 KB) → JPEG. No screenshots table (images are inline per tab) — fine to keep per-tab images inside The settings window, but add `## Screenshots` with the Play tab or a gameplay shot. Video ✓. | Add `## Where things live` table (paths now scattered in Settings, Troubleshooting, Uninstall), `## Known limitations` (native Windows not play-tested; above-60 fps marked experimental), `## Gamepad` if the controls need more than the Controls tab text. For developers: add fmt/clippy/deny, `sdk-sync --check`, `sdk-bump`, `clean`. Credits: add the window's imagery, Barlow fonts, AppImage tools, PCGamingWiki, rea-launcher-sdk. How it works ✓ (optional section, keep). |

### `linux/README.md` gaps

| Launcher | Gap |
|---|---|
| CMR3 | Closest to the model. Title "Playing on Linux" → `# Colin McRae Rally 3 on Linux`; "With Steam (Proton)" → `Steam (Proton)`; "With your own Wine and gamescope" → `Your own Wine`; add `## Environment` (only `CMR3_PREFIX`/`CMR3_RUN` mentioned inline). |
| CMR4 | Only `cmr4-play` and AppImage sections → add Steam (Proton) + Steam Deck, Environment, Where things are, Troubleshooting. |
| Screamer | Native: AppImage / zip / Steam / Steam Deck ✓. Rename "Settings for the gamescope run" → `Environment` (or keep under Your own setup), title → `# Screamer Trilogy on Linux`. |
| Sega Rally Revo | "Linux helper" → `# Sega Rally Revo on Linux`; add `## The AppImage`, `### Steam Deck`, `## Where things are`, `## Troubleshooting`; the bullet list of internals (SDL bridge keys, ICD pinning, test seams) → For developers / `docs/RUNTIME_NOTES.md`. |
| OutRun 2006 | "Linux helpers" → `# OutRun 2006: Coast 2 Coast on Linux`; the AppImage section describes how it is built → player text (run it, prefix, gamescope), build details to the main README's For developers; add Steam Deck, Where things are, Troubleshooting; `OR_*` table → `## Environment`. |

Cross-cutting, outside the README but met while reading: issue template
names differ (`bug.yml`, `bug-report.yml`, `test-report.yml`,
`unknown-edition.yml`, some launchers lack an unknown-edition form); the
README links must follow whatever each repository has, and the family
should converge on the scaffold's `bug-report.yml` + `unknown-edition.yml`.
