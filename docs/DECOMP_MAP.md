# Decompilation / recompilation map

This is the human-readable progress ledger for the reconstruction. It tracks
test-backed compatibility work, not the percentage of functions mechanically
decompiled. Stable IDs should be kept when an item is refined or split.

Status markers:

- `[x]` implemented and covered by repeatable tests/checks;
- `[~]` useful evidence or implementation exists, but the compatibility slice
  is incomplete;
- `[ ]` not implemented;
- `[?]` blocked on evidence or an explicit product decision.

## A. Provenance and user-owned data

- [x] `A01` Identify the canonical official EU `Rally.exe` by exact digest.
- [x] `A02` Inventory the supplied installation without committing payloads.
- [x] `A03` Record executable variants and keep region/patch roles explicit.
- [x] `A04` Define the reviewed 5,162-file installed-source profile.
- [x] `A05` Verify paths, sizes, storage encodings, and hashes before import.
- [x] `A06` Reject traversal, case collisions, symlinks, and special files.
- [x] `A07` Import verified user data atomically into ignored `runtime-data/`.
- [x] `A08` Recheck imported asset size/hash on every load.
- [x] `A09` Decode bounded raw and gzip/`CMPR` stored assets.
- [~] `A10` Support additional legitimate disc/install editions and languages.
- [ ] `A11` Define migration/versioning for future imported-data schemas.

## B. Executable and platform contracts

- [~] `B01` Map `Rally.exe` functions, globals, imports, and call clusters.
- [~] `B02` Map `Game.exe` launcher/video lifecycle and executable handoff.
- [~] `B03` Map `qmixer.dll` ABI and the reachable imported subset.
- [x] `B04` Recover legacy window/message/focus/presentation decisions.
- [x] `B05` Recover deterministic timing and scheduler rules as pure plans.
- [x] `B06` Recover keyboard/joystick state and frontend input rules.
- [x] `B07` Define indexed display/backend contracts and shared contract tests.
- [x] `B08` Implement the deterministic software indexed-video oracle.
- [ ] `B09` Implement a native host clock and main event loop.
- [ ] `B10` Implement native keyboard/joystick polling.
- [ ] `B11` Implement native window creation and presentation.
- [ ] `B12` Integrate focus, pause, restore, and shutdown end to end.

## C. Configuration, text, and startup

- [x] `C01` Admit the exact-size configuration record without losing bytes.
- [x] `C02` Recover volatile clearing, typed fields, and the save gate.
- [x] `C03` Implement safe exact-record configuration writing.
- [x] `C04` Recover registry language/hardware/path value policies.
- [ ] `C05` Provide the portable settings/path backend.
- [~] `C06` Recover complete profile/save formats beyond known config fields.
- [x] `C07` Parse frontend and game localization tables.
- [x] `C08` Load localization through the verified asset store.
- [x] `C09` Parse legacy font atlases, metrics, and glyph records.
- [x] `C10` Implement byte-text measurement, wrapping, and glyph layout.
- [x] `C11` Upload/rasterize all 16 frontend fonts through shared sources.
- [x] `C12` Recover startup art/palette selection and indexed presentation.
- [ ] `C13` Run startup through a native executable and window.

## D. Root-menu vertical slice

- [x] `D01` Recover root navigation, transitions, idle, confirm, and exit state.
- [x] `D02` Recover root animation endpoints and transition schedules.
- [x] `D03` Recover root bitmap/object identities and source paths.
- [x] `D04` Load required/optional root bitmaps in stable slots.
- [x] `D05` Prepare and persist root indexed surfaces without per-frame upload.
- [x] `D06` Recover root chrome, backdrop, center, and footer transcript order.
- [x] `D07` Execute root bitmap and footer commands through the software backend.
- [x] `D08` Preserve full frame command order and typed failure boundaries.
- [x] `D09` Load all six optional root C3D objects in stable slots.
- [x] `D10` Parse all three C3D counted record groups losslessly.
- [x] `D11` Resolve group-0→group-1 and entry24→group-2 numeric references.
- [x] `D12` Resolve entry24 `+2/+4/+6` references to 32-byte entries.
- [x] `D13` Bind parsed C3D slots and per-object angle state to root frames.
- [~] `D14` Decode the root C3D geometry/material/resource contract.
- [~] `D15` Render root C3D pixels in the compatibility renderer.
- [~] `D16` Recover and implement final compose/present ownership.
- [ ] `D17` Run the complete root menu interactively in a native window.

## E. General renderer and scene data

- [~] `E01` Recover bitmap/palette/surface rules used outside startup/root UI.
- [~] `E02` Recover C3D 32-byte and 24-byte entry field semantics.
- [~] `E03` Recover group-2 resource variants and binding rules.
- [~] `E04` Recover object transforms, hierarchy, animation, and dirty state.
- [~] `E05` Recover camera, projection, viewport, clipping, and culling.
- [~] `E06` Recover primitive ordering, depth, transparency, and blending.
- [~] `E07` Implement a deterministic compatibility 3D renderer/oracle.
- [ ] `E08` Parse stage/track bundle and cross-file reference formats.
- [ ] `E09` Parse car models, liveries, wheels, and damage resources.
- [ ] `E10` Parse weather, particles, shadows, and environmental effects.
- [ ] `E11` Produce representative image-comparison fixtures/tolerances.

## F. Menus, game state, and persistence

- [~] `F01` Recover remaining frontend screen graph and callbacks.
- [ ] `F02` Implement options, controller setup, and calibration screens.
- [ ] `F03` Implement profile creation/load/save/delete flows.
- [ ] `F04` Implement championship, rally, time trial, and school menus.
- [ ] `F05` Recover event, country, stage, car, and difficulty data models.
- [ ] `F06` Implement results, records, unlocks, and championship progression.
- [ ] `F07` Verify all persistence round trips and failure recovery.

## G. Driving simulation and race systems

- [ ] `G01` Recover fixed-step world update and numeric representations.
- [ ] `G02` Recover vehicle controls, drivetrain, gears, and engine model.
- [ ] `G03` Recover suspension, tyres, grip, and surface response.
- [ ] `G04` Recover collision, damage, reset, and out-of-bounds behavior.
- [ ] `G05` Recover track checkpoints, timing, penalties, and finish rules.
- [ ] `G06` Recover opponent AI and traffic/race interaction.
- [ ] `G07` Recover co-driver call scheduling and playback events.
- [ ] `G08` Recover replay recording, playback, and file format.
- [ ] `G09` Complete a deterministic offline race state trace.
- [ ] `G10` Complete the start-drive-finish-save-replay vertical path.

## H. Audio, music, video, and networking

- [~] `H01` Identify the 16 reachable `QSWaveMix*` imports and DLL exports.
- [ ] `H02` Recover WAV/sample loading, channels, priority, and lifetime.
- [ ] `H03` Recover volume, pan, frequency, spatialization, and mixing rules.
- [ ] `H04` Implement a deterministic reconstruction-owned audio mixer.
- [ ] `H05` Recover music/CD/MCI selection and lifecycle.
- [ ] `H06` Recover intro-video selection/playback/skip and launcher handoff.
- [ ] `H07` Implement portable music and video backends.
- [ ] `H08` Recover force-feedback effect semantics.
- [ ] `H09` Recover reachable DirectPlay session/protocol behavior safely.
- [ ] `H10` Design and implement an optional modern network transport.

## I. Portable build and modernization

- [~] `I01` Maintain a clean Rust workspace and source-owned test fixtures.
- [ ] `I02` Add the native application executable and runtime-data discovery.
- [ ] `I03` Build and run the compatibility slice on Linux.
- [ ] `I04` Build and run the compatibility slice on modern Windows.
- [ ] `I05` Add CI for formatting, clippy, tests, Linux, and Windows builds.
- [x] `I06` Research and select SDL3 for portable host integration.
- [ ] `I07` Implement SDL3 host window/input/audio/presentation boundaries.
- [ ] `I08` Benchmark wgpu against SDL_GPU for the recovered 3D workload.
- [ ] `I09` Implement the selected portable GPU backend behind shared contracts.
- [ ] `I10` Remove requirements for original DirectX/QMixer/WinMM DLLs.
- [ ] `I11` Package binaries without copyrighted game payloads.
- [ ] `I12` Document a controlled original-vs-reconstruction comparison.
- [ ] `I13` Publish a supported-edition and known-deviation matrix.

## Update rules

Update this file in every milestone commit that completes or materially advances
an item. Add a new stable item when newly recovered work does not fit an
existing one. The footer must remain the final lines of this file so it can be
watched with `tail`; recompute its counters from the checklist instead of
estimating them.

<!-- PROGRESS-TAIL-BEGIN -->
updated_utc=2026-10-20T19:33:47Z
milestone=rasterize-software-c3d-fragments
completed=38
partial=18
not_started=48
blocked=0
total=104
current_focus=D15-render-root-c3d-pixels
next_exit_gate=native-root-menu-vertical-slice
<!-- PROGRESS-TAIL-END -->
