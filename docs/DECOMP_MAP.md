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

- [~] `E01` Recover bitmap/palette/surface rules used outside startup/root UI (ordinary `0x81` embedded-palette 8-to-RGB555/RGB565 conversion, packed palette-entry-zero loader key, aligned stored-row handling, non-8-bit deferred-palette skip, matching-format fast keyed/opaque composition, the Selection-specific literal-zero keyed Stretch call contract plus an explicitly source-owned center-nearest oracle, and the exact no-driver-`Load` `0x90` ARGB4444 queued-font source conversion implemented; general/driver-exact stretch pixels, 4-/24-bit conversion, paletted/device-mask font branches, active-device format ownership, and alternate-font final pixels remain).
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
- [~] `E16` Recover and implement championship car transform, visibility, and runtime mutation (transform state, exact-source topology, mode-8 CPU geometry, actor hierarchy/frame planning, shared-arena bindings, full software terminal-frame indexed-COPY integration, explicit-profile standalone indexed session, all disabled/enabled combinations of Type-2 word-5 plus actor-RGB word-4 submission, exact alternate render-depth selection, capability-derived alpha/depth-clear contract, filter-request cache outcomes, raw/validated legacy pixel-format masks, retained non-alpha texture selection, source-owned selected-format conversion-before-filtering, independent logical-framebuffer composition, effect-free whole-frame backend preflight, a standalone owned split/logical-RGB backend, an explicit-palette completed-indexed-frame bridge, ordinary non-font embedded-palette RGB555/RGB565 source conversion, matching-format fast keyed/opaque logical composition, a transactional 27-source R_INFO/logo/Chequered RGB16 pool, exact effect-free setup-command identities/order shared with indexed execution, indexed-upload-free alternate setup preparation, source-owned Selection conversion and center-nearest Stretch execution at the recovered call boundary, two-pass stale-safe source-ordered alternate bitmap execution, combined same-target setup plus selected-car/empty-scene terminal ordering with coordinated state and non-indexed palette skip, verified-import standalone alternate session ownership with repeated-frame state retention, persistent 16-style ARGB4444 alternate-font source ownership, exact queue capacity/record/flush plus championship tuple/fade behavior recovered, effect-free lane-neutral semantic queued-glyph producer inputs in setup/glyph order, alternate-session localization/font ownership with success-only complete transcript publication, exact byte-3 runtime gate, caller-selected three-lane FIFO routing with independent 2,047-record capacity/drop behavior, recovered native record-field meanings, exact zero-transform/no-viewport quad carriers and mode-dependent UVs, complete source/geometry plus scene/backend preflight, and unconditional in-scene lane dispatch after world reset with semantic outcome counts; native record packing/count clearing, optional transform/UV-swap and device-state execution, driver `Load` equivalence, effective filtering/blending, fallback/textured font pixels, driver-exact Stretch sampling, native byte layout/pitch, final alpha pixels, presentation remain).

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

- [~] `G01` Recover fixed-step world update and numeric representations (the main loop `FUN_0049db00` WinMain -> `FUN_004c1e00` frame driver -> `FUN_00443e90` HighLevelTimer tick driver -> `FUN_00443db0` state-machine tick is recovered; the FSM current-state enum `DAT_005590a8` dispatches a handler proc table at `0x4ee830`/`0x4ee840` (ids 2..0xc), with in-race/gameplay states `5..=10` gated by `4 < state && state < 0xb`; the fixed step is 25 Hz / 40 ms via `timeGetTime` integer-millisecond bucketing (`FUN_00443e30` = `before/0x28 != after/0x28`, `FUN_00443e60` spins to the next 0x28 bucket) with catch-up and render decoupled, no float dt accumulator; the in-race update is `FUN_0043afd0` "Mechanics" over `DAT_00550e90` active cars; numeric representation is signed 32-bit Q16.16 fixed-point (`0x10000`=1.0, canonical `(i64)a*(i64)b>>0x10` multiply, dt coefficient `0xa3d`~0.04) with 16-bit BAM angles into a 4096-entry trig table `DAT_0082d380` (`sin=table[a&0xfff]`, `cos=table[(a+0x400)&0xfff]`), floats only at `__ftol` input boundaries; primary car state base `DAT_00700300` stride 0x768, control/telemetry base `DAT_005fffc8` stride 0x464. Reconstructed the step constants/bucket/boundary/in-race gate in `cmr-core::sim_clock` and the Q16.16 type + BAM trig in `cmr-core::fixed_point`. The per-car physics substeps, exact struct field semantics, state-id names, and the float->fixed input scaling remain for G02/G03).
- [~] `G02` Recover vehicle controls, drivetrain, gears, and engine model (the vehicle-dynamics dispatcher `FUN_00407690` (called from Mechanics `0x43afd0`) sets current-car globals `DAT_0050c604`/`DAT_0050c5b4` and runs the callees: input smoother `FUN_004088c0`, gearbox `FUN_0041b5b0` -> auto `FUN_0041b810`/sequential `FUN_0041bb30`/mode-2 `FUN_0041bf00`, RPM+limiter `FUN_0040d0c0`, torque `FUN_0040ce40`, and the force/velocity integrators `FUN_0040a4a0`/`FUN_00409f90`/`FUN_0040d740`. Drivetrain state lives in the 0x464 control struct: gear s8 `+0x43a` (0..=6 fwd, 7 reverse), per-gear ratio `+0x1d0[g]` (0=neutral), inverse ratio `+0x1f0[g]`, upshift `+0x190[g]`/downshift `+0x1b0[g]` thresholds, final drive `+0x158`, redline `+0x15c`, engine-brake coef `+0x144`, throttle demand `+0x2d8`, RPM `+0x2e0`, limiter flag/cut `+0x444`/`+0x448`, trans mode `+0x3f0`, actuators `+0x2b8[4]`; car-physics torque at `car+0x530` split to wheels `+0x534..+0x540`, car-type byte `car+0x738` selecting the per-type param region `~0x50e170` (stride 0xcd0; max-torque `DAT_0050e1ac[type*0x334]`). Recovered formulas - auto-shift `rpm vs ratio*redline>>16`, rev limiter `clamp[0,redline]` + cut `min(margin,1)*0x51eb`, torque `throttle*maxTorque - engBrake*rpm^2`, drive `torque*ratio*finalDrive`, actuator dead-zone `0x7ae`/unit slew, display RPM `norm*6500+2000` clamped `[2000,0x2134]` - are reconstructed in `cmr-core::drivetrain`. The actuator element order, the torque->velocity integrators, and the ratio-table loader remain (G03)).
- [~] `G03` Recover suspension, tyres, grip, and surface response (the dynamics dispatcher `FUN_00407690` runs a solver chain whose confidently-recovered Q16.16 passes are: the lateral-grip constraint `FUN_0041b270` that strips the sideways velocity component each step (`perp = v - fwd*dot(fwd,v); v -= perp*K`, `K=0x51e`~0.02 normal / `0x28f`~0.01 handbrake); the slope-grip loss `FUN_00408f50` ramping from tilt `0xb333`~0.70 with slope `0x42ab5`~4.1667 capped at `0xe666`~0.90; and the integrator `FUN_0040a4a0` (`accel = force*invMass + gravity`, quadratic drag `drag = v*speed`, velocity `v += accel*0x3333`~0.2 with the 40 ms step folded into the coefficient). Per-wheel surface id `ctrl[0x42a+k*2]` indexes 8-byte grip records with recovered tiers `0x3d70`~0.24 (high) / `0x2666`~0.15 (low). Reconstructed the lateral-grip constraint, slope-grip loss, net-accel/velocity/drag integrator, and surface-grip tiers in `cmr-core::vehicle_dynamics` on a new `cmr-core::fixed_point::Vec3`. The friction-circle/slip-angle tyre saturation `FUN_0040d810`, the semi-implicit position update's half-accel term, the suspension damper magnitude source, and the numeric per-surface/per-car table contents (runtime-loaded game data) remain).
- [ ] `G04` Recover collision, damage, reset, and out-of-bounds behavior.
- [ ] `G05` Recover track checkpoints, timing, penalties, and finish rules.
- [ ] `G06` Recover opponent AI and traffic/race interaction.
- [ ] `G07` Recover co-driver call scheduling and playback events.
- [ ] `G08` Recover replay recording, playback, and file format.
- [ ] `G09` Complete a deterministic offline race state trace.
- [ ] `G10` Complete the start-drive-finish-save-replay vertical path.

## H. Audio, music, video, and networking

- [~] `H01` Identify the 16 reachable `QSWaveMix*` imports and DLL exports.
- [~] `H02` Recover WAV/sample loading, channels, priority, and lifetime (official-EU QSound init at `0x4957d0` recovers a 22050 Hz, 30-channel `QSWaveMixInitEx`/`Activate` session and a fixed wave-slot table; the load path `0x4958e0` builds a PCM `WAVEFORMATEX` from name/rate/bits/stereo and registers it with `QSWaveMixOpenWave`, and the play path `0x495a30` dispatches a slot/channel through `QSWaveMixPlayEx`; the weather/crowd ambience loader `0x41cd30` is fully recovered - six 11025 Hz/16-bit waves (`crowd`, `abduct`, then storm-gated `clap1/clap2/rumble1/rumble2`), encoded as metadata in `cmr-core::audio_weather_assets`; priority/lifetime flags, memory-vs-file payload source, the load/play wrapper variants, and the full sound namespace is catalogued in `cmr-core::audio_assets` (crowd/weather waves, frontend/replay music, menu UI sounds, car/accident/prangs/newsurfs/speech/rallschl groups); the menu UI sound loader `0x468f50` is also recovered - four 22050 Hz/16-bit mono waves (`FORWARD`/`BACKWARD`/`LEFTRIGH`/`UPDOWN.WAV`) on a dedicated menu channel, and its play helpers `0x467ef0`/`0x467dd0` (stop + one-shot play of slot `base+offset`, confirm=0/nav=3 carved) are reconstructed in `cmr-core::audio_menu`; the full bare wave-filename inventory is recovered (car engine model 12×6 + shared, pacenote calls, surface/collision, tutorial numbers, cheat/secret) and the car engine structure is in `cmr-core::audio_car_assets`; the car loader body `0x4872b0` is recovered (car-selection nibble→row switch remap, 14 waves at 11025 Hz/16-bit/mono in a fixed order, slots `+0..+7` plus a `DAT_00550c08==2` tail), encoded as `car_row_index_for_selector`/`CAR_LOAD_SEQUENCE`; the engine play tick `0x487e00` rev-band crossfade is recovered (per-layer looping channels, `((RPM-lo)*count)/(hi-lo)` band index with out-of-window muting, throttle-gated Top band), reconstructed in `cmr-core::audio_engine`; the surface and collision sound banks are recovered (collision/prang loader `0x469ac0`: slot/channel bases `DAT_00592ec8`/`DAT_00592ecc`, 5 channels at full volume `0x7d00`, 48 named `prangs`/`accident` waves; surface/skid loader `0x4892b0`: `newsurfs`, bases `DAT_0082a300`/`DAT_0082a304`, 4 looping channels, per-object-type `<type>_o/i/sko/ski<n>.wav` variants; impact play `0x469e80` and roll play `0x46a030`, with the roll ramp, variant naming, and bank constants encoded in `cmr-core::audio_surface`); the co-driver pacenote subsystem is recovered (loader `0x444160` loads a 40-wave bank at 11025 Hz/16-bit/mono, slot==index, from `<root>`+`<language dir>`+`codriver\`+filename with language `cfg[0x18]&7` over english/french/german/spanish/italian/helium; submit `0x444380`/sequencer `0x444450` play one component at a time one-shot on a single shared channel via `0x444730`→`0x495a30`, linking queued notes with `INTO`, fed by an 8-entry ring and depth-2 per-track FIFO; encoded in `cmr-core::audio_pacenote`); the music and countdown play paths are recovered (music `frontend.wav`/`replay.wav` loaded whole via the second loader variant `0x4959f0` - managed handle table, fixed format args vs. the base `0x4958e0` - and played looping by `0x443b20`/keep-alive `0x4c9d70`; the start-of-stage `5-4-3-2-1-GO` countdown preloaded by `0x46a740` into `countdwn\` slots and played one-shot `base+index` by `0x46af10`, cadence `0x439b10`; channel-setup helper `0x4958a0` shared by 15 callers; second master bus `0x47eba0`; encoded in `cmr-core::audio_music_cues`); only the QMixer priority/lifetime flag nuance and the memory-vs-file payload source remain).
- [~] `H03` Recover volume, pan, frequency, spatialization, and mixing rules (per-channel `QSWaveMixSetVolume` `0x495b70`, `QSWaveMixSetFrequency` `0x495bd0`, and 3D `QSWaveMixSetPosition` `0x495c30` wrappers recovered, with stop `0x495b10`/`0x495b30` and a channel-busy `IsChannelDone` predicate `0x495d10`; panning is 3D-position based over 30 channels; the ambience trigger `0x41cfe0` recovers the `QSWaveMixPlayEx` loop/one-shot flags (`0xffffffff`/`0`) and a 0..100 master-volume percentage layer (`0x47eb70` → `0x495b70`) with a crowd-volume clamp of 32000; the abduct trigger `0x41d270` completes the ambience group's shared-channel arbitration (crowd loops at slot offset 0, suppressed while the one-shot abduct `base+1` or thunder `base+2..5` holds the channel); the surface roll volume ramp `0x46a030` is recovered (`base/3 + (count-5)*base/0x2d`, holding at full for `count>=0x24`, in `cmr-core::audio_surface::roll_volume`); the collision impact volume/sub-wave scaling `0x469e80` is observed but its exact 32-bit bit-expression is not yet transcribed with confidence (documented in the evidence ledger); the `FUN_0047f9c0` volume curve, the post-scale units, and the position coordinate space remain).
- [x] `H04` Implement a deterministic reconstruction-owned audio mixer (`cmr-core::audio_mixer` defines the backend-agnostic `AudioMixer` compatibility interface and a pure deterministic `MixerSessionState` reproducing the recovered init guard, 22050 Hz/30-channel cap, incrementing wave-slot table, PCM `WAVEFORMATEX` build, missing-handle play no-op, and per-channel volume/frequency/3D-position/stop records as emitted `MixerCommand`s; a shared `check_audio_mixer_contract` is passed by an in-crate backend. The first concrete backend `cmr-core::audio_render::SoftwareMixer` is implemented - a portable, I/O-free PCM mixer over a `WaveSource` that renders active voices (one-shot/loop, per-channel volume gain, frequency->playback-step, nearest-neighbour resample) into an interleaved stereo buffer and passes the shared contract; volume/frequency/position realisation is a documented design decision since the QMixer scales are unknown. The PCM `WAVE` decoder `cmr-formats::wav::parse_wav_pcm` is implemented (bounds-checked RIFF/PCM mono-stereo 8/16-bit -> interleaved `i16`, malformed input rejected with field-specific errors, unknown chunks retained), bridged into the mixer by `cmr-core::audio_render::PcmWave::from_wav`/`from_wav_bytes`. The directory-backed `cmr-core::audio_render::FileWaveSource` is implemented (safe game-name -> source-root path mapping with traversal/absolute/symlink refusal, reads WAVs in place and decodes them), completing the WAV -> `WaveSource` chain. The SDL3 audio-device wrapper `cmr-platform-sdl3::audio::AudioHost` is implemented - it opens a 22050 Hz/stereo/s16 playback stream and pumps `render` from a shared `SoftwareMixer<FileWaveSource>` in the SDL callback, with the game driving the recovered subsystems against the shared mixer handle. The audio chain is complete end to end: recovered triggers -> `AudioMixer` commands -> software mix -> WAV decode -> SDL device. The device glue is runtime-only; all mixing/decoding/sourcing is contract/unit-tested).
- [ ] `H05` Recover music/CD/MCI selection and lifecycle.
- [ ] `H06` Recover intro-video selection/playback/skip and launcher handoff.
- [ ] `H07` Implement portable music and video backends.
- [ ] `H08` Recover force-feedback effect semantics.
- [ ] `H09` Recover reachable DirectPlay session/protocol behavior safely.
- [ ] `H10` Design and implement an optional modern network transport.

## I. Portable build and modernization

- [~] `I01` Maintain a clean Rust workspace and source-owned test fixtures.
- [x] `I02` Add the native application executable and runtime-data discovery.
- [~] `I03` Build and run the compatibility slice on Linux (headless synthetic complete-root-frame application tests pass and an interactive `--windowed-root-frame` SDL3 host loop is implemented; a displayed native-window run on the real user corpus and reviewed-import runs remain).
- [ ] `I04` Build and run the compatibility slice on modern Windows.
- [ ] `I05` Add CI for formatting, clippy, tests, Linux, and Windows builds.
- [x] `I06` Research and select SDL3 for portable host integration.
- [~] `I07` Implement SDL3 host window/input/audio/presentation boundaries (resizable window, integer-scaled nearest streaming presentation of one completed software frame, startup gamepad enumeration with idempotent hotplug tracking, renderer-reset/redraw handling, and an edge-triggered keyboard/gamepad input mapper that drives the recovered `FrontendInput` contract implemented behind `cmr-platform-sdl3`; the pure interactive controller (`cmr-core::root_menu_interactive` plus the `steady_root_render_input` bridge) composes `FrontendState` into per-tick render inputs with a minimal root/exit transition driver; and the SDL interactive loop (`run_interactive_root_frame`, exposed through the app's `--windowed-root-frame` via `run_windowed_root_frame`) feeds live input into the controller and re-presents each distinct session-rendered frame. The audio host is now implemented (`cmr-platform-sdl3::audio::AudioHost` opens a 22050 Hz/stereo/s16 SDL3 playback stream and pumps the shared `SoftwareMixer<FileWaveSource>` from the audio callback, with the game driving the recovered subsystems against the shared mixer handle). All four SDL3 host boundaries (window/input/audio/presentation) now exist; a displayed end-to-end run on the real user corpus is gated under `I03`/`I04`).
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
updated_utc=2026-10-21T16:41:08Z
milestone=reconstruct-the-grip-model-and-velocity-integrator
completed=52
partial=27
not_started=38
blocked=0
total=117
current_focus=G04-recover-collision-damage-reset-and-out-of-bounds-behavior
next_exit_gate=recover-the-tyre-slip-saturation-and-the-semi-implicit-position-update
<!-- PROGRESS-TAIL-END -->
