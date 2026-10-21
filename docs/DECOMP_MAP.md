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
- [x] `C09` Parse legacy font atlases, metrics, and raw signed glyph records.
- [x] `C10` Implement byte-text measurement, wrapping, and glyph layout.
- [x] `C11` Upload and retain all 16 frontend fonts through shared sources.
- [x] `C12` Recover startup art/palette selection and indexed presentation.
- [ ] `C13` Run startup through a native executable and window.
- [x] `C14` Preserve unsafe shipped glyph metadata with safe per-glyph continuation.
- [x] `C15` Preflight and execute identity-neutral persistent-font text safely.

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
- [~] `D14` Decode root C3D geometry/resources and the type-1 lighting/material contract.
- [~] `D15` Render root type-1 C3D in the recovered branch-zero COPY profile; alternate Gouraud/modulate lighting remains open.
- [~] `D16` Recover and implement final compose/present ownership.
- [x] `D17` Assemble a production root-menu session from imported resources.
- [ ] `D18` Run the complete root menu interactively in a native window.
- [x] `D19` Bind the reviewed initial root C3D loader state without caller guesses.

## E. General renderer and scene data

- [~] `E01` Recover bitmap/palette/surface rules used outside startup/root UI (ordinary `0x81` embedded-palette 8-to-RGB555/RGB565 conversion, packed palette-entry-zero key, aligned stored-row handling, non-8-bit deferred-palette skip, and matching-format fast keyed/opaque logical composition implemented; keyed stretch, 4-/24-bit conversion, active-device format ownership, and alternate-font final pixels remain).
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
- [x] `E12` Prepare and transactionally upload deduplicated championship logo surfaces.
- [x] `E13` Preflight and execute identity-neutral indexed bitmap blits safely.
- [x] `E14` Admit and transactionally upload championship frame 2D assets.
- [x] `E15` Recover championship mode-8 C3D preparation and session ownership.
- [~] `E16` Recover and implement championship car transform, visibility, and runtime mutation (transform state, exact-source topology, mode-8 CPU geometry, actor hierarchy/frame planning, shared-arena bindings, full software terminal-frame indexed-COPY integration, explicit-profile standalone indexed session, all disabled/enabled combinations of Type-2 word-5 plus actor-RGB word-4 submission, exact alternate render-depth selection, capability-derived alpha/depth-clear contract, filter-request cache outcomes, raw/validated legacy pixel-format masks, retained non-alpha texture selection, source-owned selected-format conversion-before-filtering, independent logical-framebuffer composition, effect-free whole-frame backend preflight, a standalone owned split/logical-RGB backend, an explicit-palette completed-indexed-frame bridge, ordinary non-font embedded-palette RGB555/RGB565 source conversion, matching-format fast keyed/opaque logical composition, a transactional 27-source R_INFO/logo/Chequered RGB16 pool, exact effect-free setup-command identities/order shared with indexed execution, indexed-upload-free alternate setup preparation, two-pass stale-safe source-ordered alternate Fast-bitmap execution, and combined same-target setup plus selected-car/empty-scene terminal ordering with coordinated state and non-indexed palette skip implemented; ordinary stretch, standalone alternate session ownership, alternate queued-font pixels and driver `Load` equivalence, effective alternate filtering, native byte layout/pitch, alpha pixels, presentation remain).

## F. Menus, game state, and persistence

- [~] `F01` Recover remaining frontend screen graph and callbacks.
- [ ] `F02` Implement options, controller setup, and calibration screens.
- [ ] `F03` Implement profile creation/load/save/delete flows.
- [~] `F04` Implement championship, rally, time trial, and school menus.
- [ ] `F05` Recover event, country, stage, car, and difficulty data models.
- [ ] `F06` Implement results, records, unlocks, and championship progression.
- [ ] `F07` Verify all persistence round trips and failure recovery.
- [x] `F08` Model championship car/gear setup, transitions, commits, and preview transcript.
- [x] `F09` Admit all championship setup cars, palettes, and deduplicated logos from a verified user import.
- [x] `F10` Plan the validated championship setup frame with exact recovered 2D geometry and preview order.
- [x] `F11` Execute available championship 2D commands with typed deferred work.

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
updated_utc=2026-10-21T01:11:03Z
milestone=prepare-championship-alternate-setup-without-indexed-uploads
completed=50
partial=20
not_started=47
blocked=0
total=117
current_focus=E16-standalone-alternate-session-ownership
next_exit_gate=construct-and-run-repeated-alternate-terminal-frames-from-verified-import
<!-- PROGRESS-TAIL-END -->
