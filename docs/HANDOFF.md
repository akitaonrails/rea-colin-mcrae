# Reconstruction handoff

Updated: 2026-10-26

## Resume point

The repository is on `main`. The latest completed reconstruction milestone is:

```text
8a7970c feat: compose eight-lane post barrier
```

The next task is G03-AK13: reconstruct the call-free post-normal basis rebuild
at `FUN_00404440 [0x406b00, 0x406e30)`. Do not begin by changing the live game
session. Keep recovering and testing this initializer spine in order.

After opening the repository, first run:

```sh
git status --short
git log -8 --oneline
tail -n 12 docs/DECOMP_MAP.md
cargo test -p cmr-core --lib
```

The worktree was clean when this handoff was prepared.

## Latest completed work

The most recent milestones are:

```text
8a7970c feat: compose eight-lane post barrier
61d0e5b feat: reconstruct post-lane global normal
2ab57bd feat: load authenticated stage hpc runtime
43ffdc3 feat: parse stage hpc geometry
818ef49 feat: bind decoded geometry to eight-lane execution
e75a26d feat: compose decoded compiled lane provider
e799833 feat: add SuperGhidra REA cross-check bridge
72a5bb7 feat: reconstruct compiled lane record decoder
```

What these establish:

- Decoded stage `.hpc` geometry has a pure, bounds-checked parser that retains
  reserved fields and opaque trailing bytes.
- The reviewed generic asset profile already covers all 76 installed-EU `.hpc`
  sidecars. No HPC-specific extractor or profile is needed.
- `StageCompiledGeometry` loads a profile-relative stage identity through the
  authenticated `AssetStore`, owns the parsed snapshot, and supplies eight
  providers borrowing that snapshot.
- The recovered eight-lane dispatcher executes in ascending interleaved order
  with typed dispatch/config failure prefixes.
- `FUN_00408fe0 [0x408fe0,0x4095d8)` is reconstructed as the selected-quad
  two-normal transform, including intermediate publication and final overwrite.
- `execute_compiled_eight_lanes_then` invokes a typed post stage only after all
  eight lanes and their late config reads succeed. Dispatch/config failure skips
  post; post failure retains the complete lane success.

Current focused counts are `cmr-formats` 158 tests and `cmr-core` 1,232 tests.
The last full workspace test, strict clippy, portability check, legacy-backend
check, and SuperGhidra bridge test all passed.

## Exact next static contract

Fresh REA work identified the next call-free interval as
`[0x406b00,0x406e30)`. Use neutral vector/basis terminology; Gram-Schmidt and
physical-normal language are inferences.

Inputs:

- prior vector at state `+0x258/+0x25c/+0x260`;
- final AK11 vector at state `+0x324/+0x328/+0x32c`.

Exact stable-snapshot arithmetic:

```text
n = vector_324
s = vector_258

dot = wrap(qmul(n.x,s.x) + qmul(n.y,s.y) + qmul(n.z,s.z))
rejected = s - n * dot                 # qmul per component, wrapping subtract
u = legacy_normalize(rejected)
v = legacy_normalize(cross(u, n))      # exact operand order: u × n

state +0x258 = u
state +0x264 = n                       # verbatim copy, not renormalized
state +0x270 = v
```

`qmul(a,b)` is signed `i64(a)*i64(b) >> 16`, narrowed to the low `i32`.
Dot additions, projection subtraction, cross subtraction, and vector sums wrap.
Normalization is the existing generated-table `sqrt_q16` rule used by AK11:
zero length produces zero; otherwise use `floor(2^32/root)` followed by Q16
component multiplies.

Native state store order is:

```text
+0x264, +0x268, +0x26c
+0x258, +0x25c, +0x260
+0x270, +0x274, +0x278
```

Note that writing `+0x268` supersedes one of AK11's old selector words.

Suggested API:

```rust
pub const COMPILED_POST_LANE_VECTOR_BASES: [u16; 4] =
    [0x258, 0x264, 0x270, 0x324];

pub struct CompiledPostLaneBasisInputs {
    pub vector_324: [i32; 3],
    pub vector_258: [i32; 3],
}

pub struct CompiledPostLaneBasisEffects {
    pub vector_258: [i32; 3],
    pub vector_264: [i32; 3],
    pub vector_270: [i32; 3],
}

pub fn derive_compiled_post_lane_basis(
    input: CompiledPostLaneBasisInputs,
) -> CompiledPostLaneBasisEffects;
```

Minimum AK13 tests:

- exact vector bases, store order, and disjoint layout;
- orthogonal simple vectors with hard-coded generated-table results;
- nonzero projection removal;
- parallel and zero-vector degeneracy;
- exact `u × n` handedness;
- negative-odd Q16 truncation;
- wrapping dot, subtraction, cross, and `i32` extremes;
- `vector_264` is the verbatim AK11 final vector;
- unused old rows 1/2 cannot affect the result;
- orchestration consumes the AK11 final publication only after AK8 success.

## Following interval—keep separate

Do not merge pointer publication into the pure AK13 result. The next interval
`[0x406e30,0x406e9a)` reads the pointer at state `+0x2a0` and publishes the
three rows at destination strides `+0x00`, `+0x10`, and `+0x20`. Each row writes
three dwords; padding at `+0x0c/+0x1c/+0x2c` is untouched. Native code reloads
the global state and destination pointer between rows, so aliases, rebinding,
and progressive faults must remain explicit limitations.

At `[0x406e9a,0x406ea5)`, the caller reloads the global state and invokes the
already reconstructed `FUN_00409c10(state)`. `0x406ea5` starts a separate
state-adjustment phase.

## REA evidence

Authoritative evidence IDs for the current boundary:

- complete `FUN_00404440` dossier:
  `ev_91ece8af8128c4919dc5d495ae09da2e6e5d2a503b0055b468e49644bdceea98`
- slice entry:
  `ev_f4485461877bb44f85897abe56586884fd7351912fded41dd6a3a46ca61a09e2`
- first normalization `IDIV`:
  `ev_4f266cc6b83b6d7ae6f06dd03269980cae4327f11bd020f09909def80320d09e`
- following `FUN_00409c10` call edge:
  `ev_ca835a93b3dde6f990f08a908d6791085c74519c31837cee16703317422d2578`
- AK11 `FUN_00408fe0` dossier:
  `ev_3c3b901b71f69146ae7777233b27ab715eeed2be70e60f858afcdb03da22fa60`
- AK11 complete resolved callers:
  `ev_eb48f13b1fcd85bb168c624fae790cab45bc7180bc3de353ded9c1a21d9e3be7`

Local evidence exports and snapshots remain under ignored `.work/`. REA is the
authority; SuperGhidra remains an optional external discovery/cross-check tool.
Do not promote a SuperGhidra-only result into the evidence ledger.

## Required milestone procedure

For AK13 and every later completed reconstruction item:

1. Add focused unit tests, including malformed/boundary behavior where relevant.
2. Run focused tests and strict clippy while iterating.
3. Update `docs/EVIDENCE.md`, `docs/ROADMAP.md`, `docs/SUBSYSTEMS.md`,
   `docs/DECOMP_MAP.md`, and `docs/CONVERSION_TIMELINE.md` in the same milestone.
4. Keep the progress block as the final lines of `docs/DECOMP_MAP.md`.
5. Run the full gate:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets -- -D warnings
   cargo test --workspace
   python3 -m unittest scripts.tests.test_superghidra_probe
   scripts/check-portability.sh
   scripts/check-legacy-backends.sh
   ```

6. Commit only after the gate passes, using a Conventional Commit subject.
7. Pin the previous pending timeline/decomp-map milestone hash in the next
   milestone. AK12 currently needs to be pinned as `8a7970c` by AK13.

## Project boundaries to preserve

- Never track game executables, DLLs, media, archives, fonts, palettes, tracks,
  car data, or extracted payload bytes.
- Never track a developer-specific media path. Accept `--source` or use the
  documented `CMR_ORIGINAL_DIR` example variable.
- Keep REA/SuperGhidra output in `.work/` and imported data in `runtime-data/`.
- Static analysis remains the default; do not execute shipped binaries without
  an explicitly scoped and documented experiment.
- Describe decompiler output as pseudocode, not original source.
- Preserve observations, inferences, decisions, and unknowns separately.
- Do not modernize unrecovered behavior. Modern backends implement established
  compatibility interfaces and must pass their shared contract tests.

## Progress monitor

```sh
watch -n 2 'tail -n 12 docs/DECOMP_MAP.md'
```

At handoff, the progress footer reads 59 complete, 59 partial, 0 not-started,
0 blocked, 118 total. These are roadmap-item counters, not function counts.
