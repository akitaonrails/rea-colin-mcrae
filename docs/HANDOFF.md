# Reconstruction handoff

Updated: 2026-10-28 (four-wheel owned-data probe)

## Resume point

Work resumes from the `main` commit whose subject is:

```text
test: probe the compiled four-wheel car on owned AU1 data
```

Start by confirming the checkout and progress footer:

```sh
git status --short
git log -8 --oneline
tail -n 12 docs/DECOMP_MAP.md
```

This milestone wires the recovered static collision pass into the live planar
windowed session and corrects the confirmed keyboard/gamepad steering inversion.
It does **not** establish a finished or fully accurate game.

## What is now implemented

- `RaceState::resolve_static_collision_after_step` builds the exact authored
  primary cell and live car hull/projection after integration, runs the recovered
  kernel, and publishes depenetrated position, alternate velocity, next-tick
  linear/angular corrections, history, and persistent `+0x680/+0x688` flags.
- `GameplaySession::try_advance_with_static_collision` snapshots the complete
  pre-step race state. Collision failure restores it before FSM, checkpoint,
  or simulation-tick publication.
- The windowed app uses that path whenever its parsed static scene is present.
  It currently supplies `NoStaticDamageOwner`, explicitly leaving the recovered
  mutable damage-object implementation open.
- Pending linear correction `+0x3cc` is consumed once by the next translation
  step and cleared.
- Angular `+0x3d8` lifecycle per tick: the collision pass accumulates from zero;
  the integrator `FUN_0040a4a0` decays it in place by `+0x48` and uses it; the
  per-car mechanics driver `FUN_00407690` then clears `+0x3cc..+0x3e0`
  (`rea.official-eu.mechanics-tick-driver`). `RaceState::finish_mechanics_tick`
  reproduces the clear at the end of every mechanics-tick method. An earlier
  milestone wrongly concluded "never cleared" from the integrator alone; that
  is corrected, along with the two defects it left (the four-wheel decayed
  write-back, and the live planar session compounding angular impulses).
- A recovered moment-overflow trigger now runs in the four-wheel angular pass:
  only while `+0x680 == 0`, wrapping `|x| + |z|` of the decayed moment strictly
  above `0xbb80000` sets `+0x680/+0x688` and snapshots `+0xbc..+0xc4`. That
  tick's angle conversion already uses the out-of-control form (no half-delta).
  The momentum order was also corrected to `qmul(-qmul(combined, inertia), dt)`
  (the old form was one LSB off whenever the time-step product had low bits).
  The live planar windowed path still does not run this pass.
- The low-speed position suppression now tests the speed recomputed from the
  new velocity (the native test runs after `+0xec` is updated), and the
  four-wheel step gates it on `+0x742 > 0`, not the wheel flags. The angle
  deadzones and resting flag `+0x700` are now wired into the four-wheel step:
  `Controls::raw_steer` carries `+0x2fc/+0x300` from the input map, the step
  applies the instruction-confirmed low-speed cut (`|speed_ec| < 0x147`),
  publishes `FourWheelState::upright_404` from the start-of-tick body axis and
  `resting_700` from the translation's recomputed speed, and the deadzones run
  inside `apply_angular_orientation_pipeline` between angle conversion and the
  control-axis resets (`impl.steer-cut-deadzone-wiring`).
- The in-race mapper `FUN_00449580` owns the shipped camera-change input: a
  rising edge on device bit `DAT_0055aeb0 >> 16` (default `0x40`) calls the view
  cycle `FUN_00434210` (`rea.official-eu.camera-cycle-binding`).
- The explicit planar `CarControls -> Controls` bridge now preserves the
  recovered `right - left` command. Keyboard, analog, and SDL adapter tests pin
  right-positive, left-negative, and cancellation.

The collision milestone builds on these immediately preceding commits:

```text
a94b0fd feat: add lazy static damage ownership
014607f feat: consume pending static linear correction
14063c9 feat: retain live collision state across ticks
4e71941 feat: publish live collision contact count
ef78049 feat: derive canonical collision response state
2d52d72 feat: recover static response live inputs
330ff19 feat: publish compiled collision setup state
5fb7c36 feat: recover live primary collision cell
```

## Verification state

Green at handoff (2,356 tests, 36 ignored):

```sh
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Focused collision, session rollback, keyboard/analog, SDL gamepad, app build,
and full-workspace tests pass. Logs from the last run are ignored under:

```text
.work/four-wheel-probe-workspace-test.log
```

The authenticated AU1 test:

```sh
CMR_RUNTIME_DATA="$PWD/runtime-data/installed-eu-derived-assets-v2" \
  cargo test -p cmr-app \
  imported_au1_live_input_moves_the_world_and_advances_checkpoints \
  -- --ignored --nocapture
```

It was first run before the steering correction and failed only on left
steering (`+40` instead of `-40`). **It has since been rerun green** at
`b86736c`, after both the sign correction and the per-tick angular moment clear:
about 200 seconds, all assertions including steering (log
`.work/au1-authenticated.log`). It proves collision execution without error and
the steering sign on owned media; it still does not prove a scene object was
contacted (item 5 below).

## Highest-priority remaining work

1. (Done: steer cut, deadzones and `+0x700` are wired in the four-wheel step.)
   The four-wheel translation is now the integrator's own
   (`impl.four-wheel-native-translation`): no harness steering, drag, throttle
   push or lateral grip, so `speed_ec` is native. The control processor's
   pedal and steering halves are reconstructed and wired
   (`impl.pedal-response-processor`, `impl.steering-response-processor`):
   steering now rotates the lane source. The gearbox is reconstructed too
   (`impl.gearbox-processor`) and runs when `FourWheelIterationParams::gearbox`
   is `Some`; build it with `derive_compiled_gearbox_tables` and
   `compiled_drive_ratios`. Still open: the sources of the
   digital steer scalars `+0x44c..+0x45c` (block-copied; no direct store) and of
   the tables `DAT_004f74ac`, `DAT_0050e188`, `DAT_0050e4fc`. The car
   constants (mass, inertia, pedal scalars, `+0x408`) now derive from case
   words via `derive_compiled_four_wheel_constants`
   (`impl.compiled-four-wheel-constants`); the windowed session can obtain them
   from `SessionCompiledPose`'s direct cases and selector once it moves to the
   four-wheel path. `SessionCompiledPose::compiled_car` now derives those
   inputs, but the owned-data probe (`obs.au1-four-wheel-probe`, run with
   `CMR_RUNTIME_DATA=... cargo test -p cmr-app --release
   probe_compiled_four_wheel_car_on_au1 -- --ignored --nocapture`) shows the
   car cannot move yet: recover the per-car-type table block (`DAT_0050e170..`,
   stride `0xcd0`: `e1ac` engine response, `e170` gear-2 skip, `e188` steering,
   `e18c` wheel constant, `e4fc` base angle), wire `drive_torque_split` into the
   first-wheel forces, and recover gravity/ground contact in `FUN_0040d810`
   before item 2.
2. Move the windowed session from `GameplaySession::new_with_car` to the already
   recovered four-wheel path only after supplying its authenticated runtime
   inputs. The current planar path changes heading but does not evolve the full
   `MotionState::basis`, which limits collision and body-render accuracy.
3. Republish contact flags/count `+0x68c..+0x698/+0x674` per tick from live
   lane/hull state. The current values are the authenticated setup snapshot.
4. Implement the concrete mutable object behind `StaticDamageOwner`, including
   deformation/effect state, while preserving the synchronous ordered contract.
5. Add a controlled owned-media test that deliberately reaches a known static
   object and asserts contact, depenetration, history, and next-tick consumption.
   The latest AU1 driving test only establishes that collision execution did not
   error; it does not prove that a scene object was contacted.
6. Once four-wheel basis publication is live, render the car body from that basis
   rather than the static root plus planar heading adapter. Then recover the CMR1
   wheel-node/phase consumer and add animation tests. Previous review found that
   guessing wheel animation before this basis transition would compound errors.

The next `docs/DECOMP_MAP.md` focus is therefore
`four-wheel-window-path-angular-collision-and-car-animation`; counters remain
117 complete, 2 partial, 119 total because this materially advances I14 without
closing it.

## Evidence and boundaries

- Static collision recovery is documented in `docs/EVIDENCE.md` under
  `impl.atomic-live-static-collision-session` and the related producer/kernel
  records.
- The steering runtime observation is recorded as
  `obs.au1-steering-sign-regression`.
- Local REA and SuperGhidra output stays under `.work/`; do not duplicate shipped
  payloads or add absolute media paths.
- The runtime app still reads user-owned data through the explicit imported-data
  profile and source-root interfaces. Never commit original game assets.
- Keep observations, inferences, design choices, and unknowns separate. Static
  evidence does not establish runtime behavior.
- Update `docs/EVIDENCE.md`, `docs/ROADMAP.md`, `docs/SUBSYSTEMS.md`,
  `docs/DECOMP_MAP.md`, and `docs/CONVERSION_TIMELINE.md` with each milestone,
  and keep the progress footer as the final lines of `docs/DECOMP_MAP.md`.

## Progress monitor

```sh
watch -n 2 'tail -n 12 docs/DECOMP_MAP.md'
```
