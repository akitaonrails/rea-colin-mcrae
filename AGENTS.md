# Colin McRae Rally launcher — instructions for coding agents

This repository makes the player's own copy of **Colin McRae Rally**
(1998, PC) run on modern Windows and on Linux (Proton/Wine, AppImage) with
a settings window, sensible defaults and no manual steps. It is not a
distribution of the game. It is built on
[rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk), at
the tag in `sdk/VERSION`.

**Read `sdk/docs/STANDARDS.md` before any work; the playbooks are in
`sdk/docs/playbooks/`** (vendored from the SDK at the tag in `sdk/VERSION`).

## Always

- The game's files are read-only: never committed, copied into the repo or published.
- Everything outside this repository is the user's and read-only (game installs,
  other repos, Wine prefixes, saves, home configs, containers, the SDK checkout)
  unless the user asked for or approved that specific change: back up first,
  never delete by a broad glob, ask when unsure.
- Headless by default; open a visible window only when a check needs a person, and say so first.
- Never print, log or commit credentials.
- Push, tag, release or post only with the user's go-ahead.
- Research community fixes first (`docs/RESEARCH.md`); patch only what nobody solved.
- Never edit `sdk/`; SDK gaps and bugs go to the SDK repository as issues or PRs.
- Clean up after a release or a long session: `sdk/scripts/clean`, then `--yes` (log scratch in `.work/.task-created`).

## Layout

| Path | What |
|---|---|
| `crates/cmr-launcher/src/edition.rs` | Known `Rally.exe` builds (SHA-256), the unknown-edition refusal |
| `crates/cmr-launcher/src/fixes.rs` | The in-memory fixes F1–F27 as byte pieces and caves, per edition |
| `crates/cmr-launcher/src/config.rs` | The option table `config::OPTIONS`: `launcher.cfg`, flags, `--help`, template |
| `crates/cmr-launcher/src/plan.rs` | What a launch does: fixes off, values filled into them, registry; option tests end to end |
| `crates/cmr-launcher/src/form.rs`, `window.rs`, `look.rs` | The settings window's state, looks and pad layouts, drawing, palette; headless window tests |
| `crates/cmr-launcher/src/runtime.rs`, `hdpack.rs` | The runtime copy's `Policy` and saves mirror; the bundled HD art pack's install |
| `crates/cmr-launcher/src/main.rs`, `ui.rs`, `win32.rs` | Windows wiring: window, `launcher.log`, the suspended start, memory writes, hook library load, WinMM joysticks |
| `crates/cmr-hooks` | `cmrhooks.dll`, loaded into the game (no_std on Windows; tests on the host) |
| `crates/cmr-hd`, `crates/cmr-hdpack`, `tools/hd-redraw` | The HD art pack's tools and format (`docs/UPSCALING.md`) |
| `crates/cmr-launcher/tests/snapshots/` | Golden snapshots; accept an intended change with `CMR_UPDATE_SNAPSHOTS=1 cargo test --test snapshots`, then read the diff |
| `linux/cmr-play`, `linux/tests/` | The Linux helper (sources `sdk/rea-play.sh`; gamescope scaler) and its tests |
| `packaging/` | `package.sh`, allowlist, the HD pack pin (`hd-pack.txt`), AppImage entry and icon, licences |
| `docs/RESEARCH.md`, `docs/RUNTIME_NOTES.md`, `docs/CONTRIBUTIONS.md` | Findings, verification, drafts to share |

The full check list is in `sdk/docs/STANDARDS.md` ("Code", "Testing");
here the Windows target is `i686-pc-windows-gnu`. MinGW, lld and Wine live
in the `gaming` distrobox (clippy for Windows needs MinGW's gcc too: cmr-hd
builds libwebp): run them as `distrobox enter gaming -- sh -c 'cd
/absolute/path && …'`. Windows-target tests: `sdk/scripts/wine-test
--locked --workspace` (it leaves out the hook DLL, `no_std` on Windows and
unable to build in the test profile: `[package.metadata.rea] wine-test =
false` in its Cargo.toml). Examples take the game
folder from `CMR_GAME_DIR`, never a developer's path.

## Colin McRae Rally: what is specific to this game

- **Fixes stay in memory.** The launcher starts `Rally.exe` suspended,
  checks every site against the executable's bytes, writes the fixes and
  loads `cmrhooks.dll`, keyed to the exact executable by SHA-256; nothing
  is patched on disk. Unknown builds are refused with their SHA-256 and
  the issue link.
- **General solutions first.** Before patching, find the single place a
  whole class of cases goes through (a dispatcher, a drawing primitive, an
  invariant) and fix it there; no fixes one call site or one screen at a
  time unless unavoidable, and then say why.
- **Editions:** the official EU v1.5 and the EU v1.5 from a community pack
  (already patched on disk with the equivalent of F1–F5).
- **Files the game writes:** its `save` folder (`cmRally.cfg`,
  `SAVESLOT*.cms`), mirrored with the saves folder (`Saved Games\Colin
  McRae Rally` by default) before and after each run; the registry key
  `HKLM\SOFTWARE\Codemasters\Rally\1.00` (32-bit view, or the HKCU
  VirtualStore), where the paths are ours and the game's own values are
  only seeded.
- **Sibling launcher:** keep this repository recognisably alike to
  `rea-colin-mcrae-3` (the same window look in its own colours, the same
  Linux helper), so improvements cross over.
- **Owner decisions:**
  - **2026-10-30** — the pad button layout defaults to Modern.
  - **2026-10-31** — the HD art pack (`cmr-hd`'s redrawn and upscaled 2D
    art, `docs/UPSCALING.md`) is published as a release asset
    (`hd-pack-*`) and bundled with releases; never committed;
    `packaging/hd-pack.txt` pins its URL and digest (`THIRD_PARTY.md`).
  - **2026-11-01** — the settings window starts a first run on the Steam
    Deck look, and the Linux scaler defaults to Smooth (`linear`).
  - **2026-11-01** — the HD art redraw scripts in `tools/hd-redraw` stay
    in Python; they make the pack and are not shipped.
  - **2026-11-02** — running the user's original game binaries is
    allowed, from a folder outside the repository; record observations in
    `docs/RUNTIME_NOTES.md`.
- The parked source reconstruction (code and documents) is kept outside
  this repository as reference material (tag `recomp-parked-2026-11-02` in
  the developer's local history); cite it, do not bring it back here.
