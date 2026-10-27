# PS1 reference-edition plan

The Windows official-EU build remains the reconstruction target and the only
authority for Windows compatibility claims. The PlayStation release is a
separate reference edition: it can reveal shared data formats, state-machine
shapes, constants, naming, and observable game behavior, but it cannot prove
that the Windows executable uses the same implementation.

This lane is an analysis accelerator, not a second product target. It does not
change the reconstruction order in [ROADMAP.md](ROADMAP.md) or contribute to
the completion counters in [DECOMP_MAP.md](DECOMP_MAP.md).

## Tool decisions

| Role | Selected tool | Use here | Boundary |
| --- | --- | --- | --- |
| Windows static authority | REA with Ghidra | Establish Windows observations and retain Evidence IDs | Existing workflow remains authoritative |
| Windows discovery cross-check | SuperGhidra | Suggest PE function starts and cross-check selected Windows procedures | Never promote a SuperGhidra-only result |
| PS1 disc inventory | `dumpsxiso` from `mkpsxiso` | Preserve ISO9660/LBA/XA/track metadata and locate executables/overlays | User-owned extraction only; payload stays ignored |
| PS1 static loading | Ghidra with `ghidra_psx_ldr` | Load PS-X EXE, overlays, GTE instructions, and PsyQ-shaped objects | External pinned installation; generated database stays ignored |
| SDK separation | `psx_psyq_signatures` | Identify likely PsyQ library code before counting game code | A signature match is a candidate until reviewed |
| Binary/overlay splitting | Splat | Produce a reproducible section/overlay map and GNU-style MIPS assembly | Generated target-derived output stays under `.work/` |
| Selected matching work | local `m2c`, `maspsx`, and local/self-hosted decomp.me | Draft and byte-compare only high-value PS1 functions | Never upload proprietary code or bytes to a public service |
| PS1 runtime oracle | PCSX-Redux | Controlled GDB/Lua tracing, memory watchpoints, and frame/input observations | Shipped code executes only in an explicitly scoped experiment |
| Experimental native oracle | PSXRecomp | Test whether a user-owned PS1 disc can provide a quick native comparison build | Alpha accelerator, not compatibility evidence or project foundation |
| Synthetic hardware probes | PSn00bSDK | Build redistributable tests for GPU/GTE/controller assumptions when needed | Not a drop-in PsyQ replacement and not a source of game behavior |

SuperGhidra's current project integration is PE-oriented. It remains useful for
the Windows build, but is not part of the PS1 baseline unless a bounded probe
first demonstrates correct PS-X EXE and overlay handling. REA may drive the
PS1-configured Ghidra installation after the loader is validated, but the
provider/tool identity must be distinct from the Windows analysis profile.

## Local data layout

No developer path or payload is committed. Use explicit variables:

```sh
export CMR_PSX_DISC=/path/to/user-owned/cmr.cue
export CMR_PSX_TOOLS=/path/to/local/psx-tools

mkdir -p .work/psx runtime-data/psx-reference
```

- `.work/psx/`: tool versions, hashes, manifests, Ghidra/REA snapshots,
  Splat configuration/output, traces, reports, and comparison notes;
- `runtime-data/psx-reference/`: extracted or converted user-owned disc data;
- repository: only source-owned scripts, schemas, synthetic fixtures, concise
  evidence conclusions, and paths relative to the selected source root.

Tool repositories and binaries stay external. Pin every adopted tool by exact
release or commit and record its license before making it a reproducible
dependency. Do not vendor Sony SDK material, retail BIOS images, disc license
sectors, PsyQ libraries, or generated game code.

## Current bounded result

P0 identifies a USA four-track release with volume/serial `SCUS_94474`. The CUE
SHA-256 is `17cf17c55ccd870f50d9e92a7810ddf900538d615839dab77286ab09a4d06eee`;
track SHA-256 values, in CUE order, are
`746036fa7262f2a76b0a2114b69d5ccd9cf8c85c15401b76437be106bfcb2467`,
`087581da4d904e75cf657e113c43d11ce378af8f475e133c77fb271abc685f2c`,
`e62e25091e7ddc51a9e902b01c548b5d3d6a9204df40afbdbd7ecfa94fc322bc`,
and `2ad74da65166e7c7237d98d11d5c4dbb1befda2b6164fa354a928ccc9c89d93b`.
Only hashes and metadata are tracked.

The static tool baseline is:

- Ghidra 12.1.2 with Java 27;
- `ghidra_psx_ldr` release `2026.07.02`, commit
  `edb847d962081cdbae1a8fc231fa1fa05cf873c5`, release-asset SHA-256
  `a1609adc35547d0ba770155c1b19fe86d1d210a74ffdd83644b6968e9064f17e`;
- Splat 0.50.1 at commit `7548b9771e6f8418507b23434187fb07741af6d4`
  with `spimdisasm` 1.42.4;
- PsyQ signature data at commit
  `e9e46e7e133ef275a79bfce650924f98edb086bc`.

The loader successfully imports `GAME.EXE` as `PSX:LE:32`, entry
`0x800bae1c`, text load address `0x80013000`, and initial GP `0x800bae04`; the
artifact SHA-256 is
`927e473a04962fde5d7bb99d2bfbcedee84997e7f5496cd3ed04a44fe077a900`.
Splat accounts for the complete 739 KiB executable and reports 1,302 call-target
candidates, but its initial giant mixed code/data region is not a trustworthy
function map. P1 therefore remains partial until Ghidra-derived boundaries seed
the split and PsyQ matches are reviewed. The loader and PsyQ-signature
repositories do not currently publish a license in their checked-out roots, so
they remain local-only and are not redistributed.

The first cross-edition comparison found the same high-level replay object shape:
a header, deterministic seed region, four-byte control slots, and a final
two-byte field. Sizes differ by edition (`0x2c`/`0x2630`/5,400 ticks on PS1;
`0x40`/`0x151ec`/7,500 ticks on Windows). This supports a neutral Windows
trailing-field contract but does not identify that field's semantics or prove
bit-exact control equivalence. See `psx.usa.replay-object-shape` in the evidence
ledger.

## Evidence model

Every cross-edition claim has two independent sides:

```text
Windows observation (REA Evidence ID)
             +
PS1 observation (artifact digest + tool/profile + address/range)
             |
             v
cross-edition inference with confidence and remaining differences
```

Use these labels in notes and the evidence ledger:

- `observed/windows`: established from the identified Windows artifact;
- `observed/psx`: established from the identified PS1 artifact;
- `inferred/cross-edition`: a proposed shared contract supported by both;
- `decision`: reconstruction design chosen from the evidence;
- `unknown`: editions disagree or coverage is incomplete.

Never copy a PS1 constant, layout, function boundary, or behavior into the
Windows implementation merely because it looks plausible. First locate an
independent Windows consumer or make the PS1-derived value an explicitly
provisional hypothesis. PS1 runtime observations do not establish Windows
runtime behavior.

## Execution phases and gates

### P0 — identify the reference artifact and pin tools

1. Hash the complete user-selected CUE/BIN set without copying it.
2. Record region, serial, track layout, sizes, and source-root-relative paths.
3. Pin `dumpsxiso`, `ghidra_psx_ldr`, PsyQ signatures, Splat, and PCSX-Redux by
   exact version/commit; record Ghidra and Java versions separately.
4. Review licenses before adding download/setup automation.

Gate: the PS1 artifact and every analysis tool are reproducibly identified;
no extracted payload or absolute source path is tracked.

### P1 — recover the static disc and executable map

1. Use `dumpsxiso` to generate a local disc-layout manifest and, only when
   necessary, extract into `runtime-data/psx-reference/`.
2. Locate the boot executable, overlays, archives, and likely shared data files.
3. Load the executable with `ghidra_psx_ldr`; validate load address, entry point,
   GTE decoding, and at least one known call/reference manually.
4. Apply PsyQ signatures and report matched, ambiguous, and unmatched ranges.
5. Configure Splat so every executable/overlay byte belongs to an explicit
   section. Keep its assembly and target-derived configuration ignored.

Gate: executable and overlay coverage is accounted for by address and byte
range, with SDK candidates separated from game-code candidates. No decompiler
pseudocode is treated as source.

### P2 — build a cross-edition correlation map

Prioritize areas likely to survive the port between editions:

1. container and asset formats;
2. frontend/race state transitions;
3. fixed-step timing, race rules, checkpoints, and results;
4. car/track coordinate systems and fixed-point math;
5. controller actions, camera modes, HUD semantics, and audio event identities.

For each candidate, record the Windows evidence, PS1 address/range, similarities,
differences, confidence, and the next falsifying check. Prefer data-flow and
call-shape correspondence over string or numeric coincidence.

Gate: at least one currently open Windows question is narrowed by corroborated
PS1 evidence and converted into a focused source-owned test or a better-bounded
Windows REA query. Otherwise the PS1 lane remains informative but non-critical.

### P3 — use matching-decomp tools selectively

Use Splat output as input to local `m2c` for a draft only when control flow is
hard to understand in Ghidra or when a shared algorithm is a serious candidate.
Use `maspsx` and a local/self-hosted decomp.me only when byte matching the PS1
function would answer a named question. Do not pursue a percentage-complete PS1
decompilation; it would compete with the Windows deliverable.

Gate: every selected function has a written question, a bounded input range,
and a result that changes or verifies a Windows reconstruction task.

### P4 — controlled PCSX-Redux observations

Runtime work requires a short experiment document before launch containing:

- exact disc and emulator identities;
- the behavior or memory value being observed;
- initial state, input script, frame/time bound, and expected outputs;
- files the experiment may write and the cleanup procedure.

Prefer interpreter/debug mode when execution breakpoints are required. Export
only concise derived traces, addresses, counts, hashes, and screenshots that do
not reproduce proprietary payloads.

Gate: the observation is repeatable from the documented initial state and is
clearly labelled `observed/psx`; it is not silently substituted for Windows
evidence.

### P5 — optional PSXRecomp proof of concept

Run this only after P1 has produced a trustworthy executable/overlay map. Keep
the generated project entirely under `.work/psx/psxrecomp/` and require the
user's disc at setup/runtime. Compare boot, menu, one controlled race segment,
input, and frame timing against PCSX-Redux before using it as an oracle.

Gate: a repeatable build and smoke test pass without tracked game code/assets.
Failure or framework gaps do not block the Windows reconstruction.

## Test strategy

The existing unit-test rule applies unchanged:

- new disc/overlay/manifest parsers use pure bounds-checked code and synthetic
  redistributable fixtures;
- each promoted shared contract gets focused normal, malformed, truncation,
  boundary, and edition-disagreement tests where applicable;
- local owned-media integration checks are ignored/opt-in and emit only
  non-payload diagnostics;
- modern backends continue to pass the established Windows-derived contracts;
  PS1 behavior cannot weaken them;
- a PS1-derived hypothesis is not marked complete in `DECOMP_MAP.md` until its
  Windows compatibility item satisfies the normal exit gate.

## First bounded experiment

The first pass should stop after P0/P1 and answer four questions:

1. Which exact PS1 CMR edition is available?
2. Does `ghidra_psx_ldr` correctly recover its main executable and overlays?
3. How much code is identified as PsyQ versus game-specific?
4. Do the disc files expose recognizable relatives of any still-open Windows
   race/menu data contracts?

Only after reviewing those results should the project choose the first runtime
trace or matching-decomp target.

## Upstream tool references

- [`mkpsxiso` / `dumpsxiso`](https://github.com/Lameguy64/mkpsxiso)
- [`ghidra_psx_ldr`](https://github.com/lab313ru/ghidra_psx_ldr)
- [PsyQ signature database](https://github.com/lab313ru/psx_psyq_signatures)
- [Splat](https://github.com/ethteck/splat)
- [`m2c`](https://github.com/matt-kempster/m2c)
- [`maspsx`](https://github.com/mkst/maspsx)
- [decomp.me source](https://github.com/decompme/decomp.me)
- [PCSX-Redux](https://github.com/pcsx-redux/pcsx-redux)
- [PSXRecomp](https://github.com/RetroPortingToolKit/psxrecomp)
- [PSn00bSDK](https://github.com/Lameguy64/PSn00bSDK)
