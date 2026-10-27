# Reconstruction handoff

Updated: 2026-10-28

## Resume point

Work resumes from the `main` commit whose subject is:

```text
feat: wire live static collision session
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
  step and cleared. Angular `+0x3d8` is published but is not yet consumed/cleared
  by the live orientation path.
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

Green at handoff:

```sh
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Focused collision, session rollback, keyboard/analog, SDL gamepad, app build,
and full-workspace tests pass. Logs from the last run are ignored under:

```text
.work/handoff-workspace-test.log
.work/handoff-clippy.log
```

The authenticated AU1 test was run before the steering correction:

```sh
CMR_RUNTIME_DATA="$PWD/runtime-data/installed-eu-derived-assets-v2" \
  cargo test -p cmr-app \
  imported_au1_live_input_moves_the_world_and_advances_checkpoints \
  -- --ignored --nocapture
```

It ran for about 187 seconds and reached the final steering assertion without a
collision error. The only failure was left steering producing heading delta
`+40` instead of expected `-40`. The one-boundary sign fix is covered by focused
tests, but this long authenticated test was not rerun. Do that when the runtime
budget permits; do not represent it as already green.

## Highest-priority remaining work

1. Recover and wire angular `+0x3d8` consumption/clear into the exact orientation
   pipeline. Do not apply it as a guessed planar heading increment.
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
