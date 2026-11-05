# Playbook: reverse engineer, test and apply a binary patch

Lessons from CMR3, CMR04, Screamer, Sega Rally Revo and OutRun. Read
`STANDARDS.md` first.

## 0. Do you need a patch at all?

In order of preference:

1. A **community fix** already does it (SilentPatch, Tweaks, a widescreen
   fix): bundle or configure it.
2. A **setting** does it: the game's INI, registry, command line, a wrapper
   option (dgVoodoo2, DXVK `dxvk.conf`, DOSBox config).
3. A **data change** does it: a resolution or aspect table, a constant.
4. Only then a **code patch**, as small as possible.

## 1. Find the one place (REA)

- Start from evidence: strings (`search_strings`), imports
  (`inspect_pe_imports`), a known value (screen width, 0.75 aspect,
  frame-time constant) searched with `search_bytes`.
- Walk callers and callees (`xrefs`, `procedure_callers`, `get_call_graph`)
  until you find the **chokepoint** a whole class of cases goes through
  (Screamer: the camera rotation matrix for Hor+, the `smoothed_dt` clamp
  for timing; SRR: the aspect table). Patch there once, not at every caller.
- **Pseudocode is a hint, not the truth.** Check the disassembly for
  signedness and condition codes (`jl`/`jg` vs `jb`/`ja`, `movsx` vs
  `movzx`, `imul` vs `mul`). One wrong branch silently disables a code path.
- Follow the data end to end: a value fixed at one site may be recomputed
  or clamped later (`trace_feature`, `trace_native_values`).
- Keep notes in the launcher's `docs/RESEARCH.md`: observation (what the
  bytes say), inference, decision, unknowns — kept apart.

## 2. Design the patch

- Smallest change: a constant, a jump, a table entry. Prefer changing data
  over code.
- Need room for new code? Use a code cave: zero-filled bytes inside an
  executable section, checked (`patch::check_caves`), never overlapping
  another site or (for LE/DOS images) a relocation fixup.
- Write each site as **original bytes → patched bytes** at an exact address,
  for each exact build (SHA-256). Where builds differ, a byte signature that
  must match exactly once (`patch::signature_toggle`) — never "first match".
- Note the **expected SHA-256 after patching** for each build; the
  launcher verifies it before writing.

## 3. Organise it in code

One declarative table per game (`src/patches.rs`), built with
`rea_launcher::patch`:

- `PatchSet`: name, what the player sees change, the editions it applies to,
  its sites (virtual address or file offset; `rea_launcher::pe` converts),
  caves, expected result hashes.
- The table is the **single source of truth**: the docs, the README and any
  PCGamingWiki draft get their offset tables from `PatchSet::describe()`,
  never retyped by hand (Screamer's hand-written docs drifted from its code).
- Keep the LE (DOS/4GW) parser and similar format code in the launcher until
  a second game needs it.

## 4. Test it, in three layers

1. **Unit (always, host):** golden bytes for every site from a tiny
   synthetic buffer; all-or-nothing on a mismatch; already-patched input is
   accepted unchanged; unpatch from pristine; caves and overlap checks.
2. **Real executable (env-gated):** `<GAME>_GAME_DIR=… cargo test real_game
   -- --ignored` applies every set to every edition found and checks the
   result hashes. Never commit the executable or bytes copied from it.
3. **The game (headless):** run the patched runtime copy under
   `gamescope --backend headless`, capture what proves the effect
   (screenshot at 21:9, frame-time log, a physics run compared with the
   unpatched game). Record it in `docs/RUNTIME_NOTES.md`.

**Record dead ends too.** Screamer's frame-rate unlocks all broke the
physics; that is written down so nobody ships one again.

## 5. Apply it

- Only to the launcher's private runtime copy, and always **from the
  pristine bytes** of the game's original (toggling a fix off needs no
  restore step).
- Identify the build first (`identify`), refuse unknown builds with their
  SHA-256, apply all sites or none, verify the result hash, then write with
  `fsx::install` (never through a hard link).
- Expose a fix as an option only after layer 3 showed its effect; describe
  it in plain words in the window.

**Never into the running game.** Write fixes into the runtime copy's file
(`patch` for bytes in place, `pe::edit::add_section` for a fix's own code
and data) and load a hook DLL through the runtime copy's import table
(`pe::edit::add_import`, the address table where the fix reads it). A
launcher that starts the game suspended and calls `VirtualAllocEx`,
`WriteProcessMemory` or `CreateRemoteThread`, or hooks the game's thread
from outside, looks like a trojan to antivirus heuristics: CMRLauncher 0.9.0
was flagged on a player's Windows (Quiver feedback, 2026-11-04).
`launcher-check` rule `remote-injection` fails on those calls.

## 6. Share it

A verified patch the community lacks becomes a draft for PCGamingWiki
(`contribute-back.md`): file offsets with original → new bytes from
`describe()`, the exact build's SHA-256, a link to the repository.
