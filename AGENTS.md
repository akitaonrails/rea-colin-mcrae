# Project rules

This repository builds a self-contained compatibility package that runs the
user's own copy of the 1998 PC Colin McRae Rally on modern Windows and on
Linux (Proton/Wine). It is not a distribution of the game.

## Non-negotiable boundaries

- Never commit, vendor, embed, or publish original executables, DLLs, media,
  archives, fonts, palettes, tracks, car data, or other game assets, or
  anything copied out of them.
  Exception (owner decision, 2026-10-31): the HD art pack (`cmr-hd`'s
  redrawn and upscaled 2D art, see `docs/UPSCALING.md`) is published as a
  release asset (`hd-pack-*`) and bundled with releases. It is never
  committed; `packaging/hd-pack.txt` pins its URL and digest.
- The user's game folder is read only. The launcher builds a separate runtime
  folder from it.
- Never commit a developer's absolute game path. Commands and code take an
  explicit path (or `CMR_GAME_DIR` in examples).
- Put local experiments, traces and investigation output under `.work/`.
- Running the user's original game binaries is allowed (owner decision,
  2026-11-02): run them from a local folder outside the repository and record
  notable runtime observations in `docs/RUNTIME_NOTES.md`.

## Product rules

- **Self-contained.** The user installs this package and points it at the
  game. Never require third-party patches, wrappers, codec packs,
  compatibility modes or manual registry edits; whatever the game needs is
  bundled and configured by us.
- **Windows first, easy on Linux.** Ship Windows binaries that also run under
  Proton or Wine with no extra setup.
- **Reuse open source.** Where a maintained open-source component already does
  the job (graphics wrappers, controller layers, decoders), bundle and
  configure it instead of writing our own. Check licences before bundling and
  record each component with its licence and source in `THIRD_PARTY.md`.
- **Game fixes stay in memory.** Game-specific fixes are applied at run time
  from our hook library, keyed to the exact executable by hash, never by
  patching files on disk.
- The faithful, unchanged game is the default. Enhancements (resolution,
  widescreen, frame rate, HD art) are opt-in settings. Exception (owner decision,
  2026-10-30): the pad button layout defaults to Modern.

## Code and commits

- General solutions first. Before patching, find the single place a whole
  class of cases goes through (a dispatcher, a drawing primitive, an
  invariant) and fix it there. Do not add targeted patches one call site or
  one screen at a time; if one is unavoidable, say why.

- Rust for our own code. Keep `unsafe` and Win32 FFI in narrow modules.
- Unit-test the logic that can run on the host (edition detection, runtime
  folder layout, configuration, patch tables) before each milestone commit;
  record what was checked by running the game.
- Use Conventional Commit subjects. Commit at working milestones.
- The parked source reconstruction (code and its documents) is kept outside
  this repository as reference material (tag `recomp-parked-2026-11-02` in
  the developer's local history); cite it, do not bring it back here.
