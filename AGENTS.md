# Project rules

This repository reconstructs the Windows release of Colin McRae Rally from
shipped binaries and user-owned data. It is not a distribution of the game.

## Non-negotiable boundaries

- Never commit, vendor, embed, or publish original executables, DLLs, media,
  archives, fonts, palettes, tracks, car data, or other game assets.
- Read original media in place. Generated manifests may contain paths relative
  to a user-selected source root, sizes, hashes, and format metadata, but not
  payload bytes.
- Put local REA snapshots and investigation output under `.work/`.
- Put imported or converted user data under `runtime-data/`. Both directories
  are ignored by Git.
- Describe decompiler output as pseudocode, not original source. Separate
  observations, inferences, design decisions, and unknowns.
- Do not execute shipped binaries unless a runtime experiment is explicitly
  scoped, isolated, and documented. Static analysis is the default.

## Reconstruction order

1. Establish exact artifact identities and evidence provenance.
2. Recover file formats, ABI boundaries, state machines, and subsystem behavior.
3. Implement an original-compatible build against explicit compatibility
   interfaces.
4. Verify behavior with differential fixtures or controlled observations.
5. Add modern platform backends (such as SDL) without weakening the
   compatibility baseline.

Do not modernize behavior while it is still being recovered. A modern backend
must implement an already-defined project interface and pass the same relevant
contract tests.

## Code and commits

- Prefer Rust for new code when it does not obstruct binary-compatible layouts,
  calling conventions, or faithful translation. Isolate unavoidable C/C++ or
  assembly behind narrow interfaces.
- Keep parsers pure and bounds-checked. Preserve unknown fields and reject
  malformed or truncated input with useful offsets.
- Tests and fixtures must be source-owned and redistributable. Tiny synthetic
  fixtures may reproduce a format shape; do not derive fixtures by copying game
  payloads.
- Make milestone commits only after the relevant checks pass. Use Conventional
  Commit subjects.
- Update the evidence ledger and roadmap in the same milestone that changes a
  compatibility claim.

