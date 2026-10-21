# Controlled original-vs-reconstruction comparison

This document defines how the reconstruction's behavior is checked against the
original Windows release of Colin McRae Rally (roadmap item `I12`). It is a
methodology and a status map, not a claim that the two are bit-identical today.

The project's default is **static analysis**: the shipped binary is not executed
unless a runtime experiment is explicitly scoped, isolated, and documented (see
[AGENTS.md](../AGENTS.md)). Every comparison technique below is chosen to respect
that boundary and to use only source-owned, redistributable fixtures - never
copied game payloads.

## Comparison levels

The reconstruction is compared to the original at five levels, matching the
reconstruction order in `AGENTS.md`. Each level has a concrete, repeatable check.

### 1. Artifact identity (established)

Every claim is anchored to an exact artifact. The canonical reference is the
official EU updater payload (`.work/official-eu/Rally.exe`, PE32 i386, image base
`0x400000`, SHA-256 `0eb6ea96…`); the installed EU-derived executable, the
official US payload, and the unofficial xdelta result are tracked comparison
variants. Identity is verified by recomputing the SHA-256 of the developer's own
copy and matching it to the hash recorded in [EVIDENCE.md](EVIDENCE.md). No
payload bytes are committed; only paths relative to a user-selected `--source`
root, sizes, hashes, and format metadata appear in generated manifests.

### 2. Format parity - differential fixtures (repeatable now)

Recovered file formats (`cmr-formats`) are checked by round-tripping and by
decoding tiny synthetic fixtures whose shape reproduces the real format without
copying any game data. A parser is considered to match the original when:

- it accepts a well-formed synthetic fixture and reproduces the recovered field
  layout, and
- it rejects every malformed or truncated variant with a field-specific,
  offset-bearing error (the original's acceptance gate is modeled as the
  rejection boundary).

A developer who owns original media can run the **optional** differential step:
point a decoder at a real asset under `--source` and confirm the parse succeeds
and the re-encoded bytes are identical. That byte-exact check is gated on owned
media and is never part of the committed suite.

### 3. Behavioral parity - recovered formulas as oracles (repeatable now)

Behaviors recovered as decompiler **pseudocode** are reconstructed as pure Rust
with the recovered constants, offsets, bit layouts, and formulas encoded
directly. The unit tests then assert those exact values - they are the standing
comparison against the original's observed logic. Examples already in the suite:

- fixed-point and geometry math (`cmr-core::fixed_point`, physics),
- the championship data model and progression (`championship_model`,
  `championship_progression`): offset mapping, standings-rank formulas, record
  offsets, mode predicates,
- persistence (`settings_store`, `profile_store`) and its
  [round-trip suite](../crates/cmr-core/tests/persistence_round_trip.rs),
- input/options encoding (`options_screen`, `control_bindings`,
  `input_calibration`),
- audio mixing and the recovered trigger logic (`audio_*`).

Where a formula is **not** recovered, the reconstruction records it as an
unknown rather than inventing one, and the test tracks the state without
asserting a fabricated result (see the fade-volume curve in `music_player`, the
collision impact bit-expression in `H03`).

### 4. Persistence parity - byte-exact round-trips (gated on owned media)

The save/settings records (`settings_store`, `profile_store`) round-trip through
the recovered load/validate/finalize path and recover to shipped-defaults or
empty-slot behavior on every corruption, truncation, or absence. The remaining
step - loading a file **written by the original binary** and reproducing it
byte-for-byte - is a differential fixture gated on an owned original save
(`F07`). It is documented here so a developer with such a save can perform it,
but the save is never committed.

### 5. Observational parity - controlled runtime checks (scoped, opt-in)

Runtime comparison (capturing the original's output for a fixed input and
diffing it against the reconstruction) is permitted only as a scoped, isolated,
documented experiment. The supporting harness exists:
`cmr-test-support::image_compare` provides tolerance-aware RGB comparison
(exact, RGB565 round-trip, dithered) for frame-level checks, and the SDL and
software backends are held to the **same** shared contract tests so a modern
backend is measured against the compatibility baseline rather than against a
fresh guess. No such runtime capture is committed; this level is a procedure, not
a current automated result.

## Running the repeatable comparison

Levels 2 and 3 run with the normal gate and need no original media:

```sh
cargo test --workspace
```

The Windows-portability cross-check (`I04`) and the CI gate (`I05`,
[.github/workflows/ci.yml](../.github/workflows/ci.yml)) extend the same checks
to a second target and to continuous integration.

Levels 1, 4, and the byte-exact half of level 2 are **optional** and require the
developer's own `--source` media or saved games; they are described above so they
can be reproduced, and they never add committed fixtures.

## What is not yet compared

- Byte-exact persistence against original-written saves (gated, `F07`).
- A displayed end-to-end run against the real user corpus on Linux and Windows
  (gated, `I03`/`I04`).
- Frame-level rendering parity for the full 3D workload (depends on the GPU
  backend, `I08`/`I09`).
- The intentional, permanent differences between the reconstruction and the
  original - portable paths instead of the Windows registry, SDL instead of
  DirectX, a software mixer instead of QMixer, no CD/MCI music path. Those are
  not defects to be closed but deviations to be declared; they are catalogued in
  the supported-edition and known-deviation matrix (`I13`).
