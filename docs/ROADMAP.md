# Reconstruction roadmap

The project advances by evidence-backed vertical slices. A subsystem is not
“done” because pseudocode exists; it is done when a clean implementation has a
repeatable compatibility check at the strongest practical boundary.

## Architecture direction

Keep recovered game behavior independent of the host platform:

```text
user-owned installation
        |
        v
verified importer ---> ignored runtime-data/
                            |
                            v
formats/resources --> game simulation and state
                            |
              +-------------+-------------+
              |                           |
      compatibility backend         modern backend
      (recovery oracle)              (SDL3 platform + wgpu)
```

The compatibility backend exists to establish the original contract. The
modern backend is allowed to differ internally, not observably where the
compatibility specification applies.

## Milestone 0 — provenance and safe workspace

Deliverables:

- exact hashes, sizes, PE identities, and analysis-tool versions;
- read-only inventory of installation contents and patches;
- asset exclusion rules and importer design;
- evidence ledger format and initial unknowns;
- subsystem map and ordered recovery backlog.

Exit gate:

- another developer can reproduce the inventories without copying source media
  into Git;
- observations are attributable to exact artifact hashes;
- reference edition/region ambiguity is explicit.

Reference policy: use the official EU updater payload as the canonical clean
code artifact. Keep the installed EU-derived executable and both supplied
xdelta outputs as comparison variants. Treat the official US payload as a
separate regional target; its differences are too broad to model as resources
alone. This policy may expand when original-disc evidence becomes available.

## Milestone 1 — executable map and data contracts

Recover both executables before assigning their roles. Build a function map from
entry points, imports, strings, references, and call clusters. Prioritize
boundaries that reveal stable contracts:

1. startup, command line, registry/configuration, and executable handoff;
2. filesystem lookup, case/separator rules, gzip/`CMPR` unwrapping, and
   archive/resource loading;
3. frontend state, localization, save/config persistence;
4. timing, input, rendering, audio, CD/music, and networking;
5. car/track loading, physics state, AI, race rules, replay, and results.

Initial static evidence identifies `Game.exe` as a video/launcher frontend that
starts `Rally.exe`. Reconstruct it as a separable launcher/presentation concern;
do not let its DirectShow-era lifecycle contaminate the game core.

Deliverables:

- versioned function/symbol map linked to REA Evidence IDs;
- format specifications with bounded parsers and synthetic fixtures;
- compatibility interfaces for clock, input, video, audio, storage, and network;
- reviewed reconstruction obligations with owners and verification methods.

Exit gate:

- each high-priority boundary has an observation, a bounded inference, or a
  named unknown with a next query;
- no format parser depends on game payloads in the test suite.

## Milestone 2 — first compatible vertical slice

Implement the smallest path that exercises real contracts end to end:

- locate and validate user data;
- parse configuration and localization;
- create a window and deterministic clock/input loop;
- render one frontend state using imported resources;
- exit cleanly while preserving expected configuration behavior.

Preferred implementation language is Rust unless recovered ABI/layout evidence
makes a narrow C/C++ module materially safer. Do not translate entire decompiler
listings mechanically; recover named domain behavior and explicit data layouts.
Every completed recovered behavior lands with deterministic unit tests in the
same milestone. Shared backend contracts supplement, rather than replace,
focused boundary and failure tests; no later subsystem may depend on an
untested compatibility rule.

Current implemented portion: reviewed asset-store loading now reaches startup
art and fixed-width frontend/game localization tables; configuration has an
opaque exact-size load model and a tested caller-owned write boundary; indexed
startup presentation runs against the deterministic software backend. The
first/root frontend state, fixed-layout font parsing, byte-text layout, input,
window-loop, timing, and root-menu visual rules are pure tested contracts. All
16 font styles and the four present root-menu artwork rows resolve through the
verified import; the loader's distinct Italian row is explicitly absent from
the reviewed corpus. Root bitmaps now reach fixed-slot indexed surfaces without
backend mutation, and both persistent font and root-bitmap pools now use a
tested shared source store without monopolizing the backend. Transaction,
fixed-slot, and mixed-pool tests cover 33 coexisting sources, optional upload
holes, isolated required rollback, repeated-frame reuse, and global teardown.
Host clock/input, native window execution, root visual-command execution, the
rest of root-frame composition, and presentation remain before the vertical-
slice exit gate. Root-footer text reaches a tested software back-buffer pixel
through localization, the shared store, and the persistent font pool. Root
visual transcripts now name fixed bitmap slots directly; opaque handles remain
only for the still-unimplemented C3D path.

Exit gate:

- clean build from source on the declared toolchain;
- importer succeeds for a recognized source and fails safely for wrong,
  incomplete, or modified sources;
- deterministic tests cover the vertical slice;
- a documented controlled comparison bounds remaining differences.

## Milestone 3 — playable compatibility baseline

Add subsystems in dependency order:

1. menus, profiles, saves, options, and localization;
2. resource decoding, renderer, audio mixer, and input mapping;
3. car and track scene loading;
4. fixed-step physics and collision;
5. race sequencing, timing, penalties, AI, co-driver, replay, and results;
6. multiplayer only after its original protocol and security boundary are known.

Exit gate:

- a complete offline race can be started, driven, finished, saved, and replayed
  using user-imported data;
- compatibility checks cover deterministic state traces, file round trips, and
  representative rendered/audio outputs with documented tolerances;
- all required obligations for that path are closed or explicitly unsupported.

## Milestone 4 — modern platform backends

Replace legacy boundaries one at a time while retaining the compatibility lane:

| Legacy boundary | Candidate modern backend |
| --- | --- |
| DirectDraw / GDI | Provisional: wgpu hosted in an SDL3 window; compare SDL_GPU before lock-in |
| DirectInput / WinMM joystick | Provisional: SDL3 keyboard/joystick; optional mapped gamepad after compatibility |
| QMixer / WinMM audio | Provisional: SDL3 audio with a reconstruction-owned mixer |
| Win32 timing/window loop | Provisional: SDL3 events plus a monotonic host clock |
| DirectPlay | explicitly designed transport after protocol recovery |
| Registry/install paths | portable configuration and data discovery |

The rationale, alternatives, compatibility constraints, and benchmark gates are
tracked in [`MODERN_BACKEND_RESEARCH.md`](MODERN_BACKEND_RESEARCH.md). SDL is a
platform-layer candidate, not a mandate that every subsystem use SDL APIs.

An original-compatible audio lane can begin with a narrow shim for the 16
`QSWaveMix*` APIs the game imports. The shipped DLL's complete 62-export surface
is an ABI oracle, but unreferenced exports are not automatically reconstruction
requirements.

Exit gate:

- modern backends pass the same contract suite;
- no original DLL is required for the supported portable build;
- deviations from legacy behavior are intentional and documented.

## Verification budget

Use the least duplicated evidence that can establish each claim:

| Claim | Primary evidence owner | Minimum proof |
| --- | --- | --- |
| artifact identity | provenance tooling | size + SHA-256 + parsed PE metadata |
| file format | format module | bounds tests + synthetic fixtures + local corpus scan |
| state transition | game subsystem | deterministic trace comparison |
| persistence | storage subsystem | byte/field round trip and negative cases |
| rendering | renderer | scene/state trace, then image comparison with tolerance |
| audio | mixer | event trace, then deterministic sample/hash window where possible |
| backend parity | platform interface | shared contract suite on both backends |
| playable path | integration owner | controlled end-to-end run with imported data |

Static analysis cannot prove runtime behavior. Runtime comparisons will be
introduced only when their environment, side effects, reset procedure, and
capture authority are explicit.

