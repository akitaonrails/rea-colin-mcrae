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
      compatibility backend         native host / modern backend
      (recovery oracle)              (SDL3; GPU choice benchmarked)
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
Host clock/input, native window creation, and a host adapter for the recovered
presentation executor remain before the vertical-slice exit gate. Complete
root frames can now be composed and captured by the deterministic headless
backend, without assigning native display semantics to that test oracle.
Root-footer text reaches a tested software back-buffer pixel through
localization, the shared store, and the persistent font pool. Root visual
transcripts name fixed bitmap slots directly, and the bitmap executor now
submits their exact opaque/keyed plans with checked geometry and no per-frame
uploads. A sequential frame dispatcher preserves bitmap/footer/C3D/scene/
presentation order and typed failure boundaries. It stages one owned prepared
C3D plan at the root command, then consumes it once inside the terminal scene.
The scene runs even without a selected object and now owns checked viewport
setup, conditional depth clear, Begin/End cleanup, world reset, and the opaque
scene tail. The software backend now executes the bounded branch-zero plan into
the indexed back buffer with persistent bindings and depth, then atomically
captures an owned indexed frame and palette at the terminal hook. The separate
native presentation executor now preserves sample-before-Flip invocation and
the windowed vblank/fallback sequence behind a typed adapter. The recovered
deferred-palette state and depth-8 commit are tested and integrated, including
runtime group mapping, both software caches, sentinel behavior, typed failure,
and exact scene/cleanup→palette→presentation order. The prepared pipeline sets
the selected root visibility latch after successful planning and clears all
six after every terminal scene attempt. Native host adaptation remains a
boundary. A bounded static search found no steady-frame windowed copy owner in
the shipped build, so a working portable windowed copy must be explicit host
policy rather than attributed legacy behavior.
The software compatibility path now has a production, callback-scoped root-menu
session assembler. It consumes an initialized startup display and verified
import, applies the recovered zero-RGB then pending `disk.pal` bootstrap,
retains every root resource and mutable frame state across repeated renders,
and tears down persistent sources before returning the backend. Native window,
event-loop, and host presentation integration remain separate later work.
The six optional root-menu C3D files now load through fixed slots and a bounded,
byte-preserving parser. All three counted groups expose proven framing: group
0's raw `u16` values; group 1's typed, borrowed 32/24-byte entry views; and
group 2's raw label fields plus opaque length-delimited blobs. The 32-byte view
decodes eight signed little-endian Q16.16 components in their recovered groups.
The 24-byte view exposes its three local entry ordinals, signed resource
ordinal, three byte-exact corner tuples, and two still-unknown scalars. Checked
borrowed resolvers now implement the proven group-0→group-1, entry24
`+2/+4/+6`→local entry32, and group-1-entry24→group-2 numeric joins, including
the latter's `-1` sentinel, without making reference validity a structural
parse requirement. A separate checked topology planner now partitions only
consecutive equal prepared selectors, deduplicates local entry ordinals in
first-use order, and emits three compact indices per source entry. It retains
borrowed typed source views rather than pretending to produce render-ready
vertices. Runtime affine conversion, selector/resource preparation, associated
side-vector materialization, and resource-derived mapping-coordinate scaling
are now recovered below; general/non-root material execution and
backend-integrated pixels remain unrecovered.
Group-2 records now also classify complete
`BM` signatures as opaque bitmap bytes and expose strictly framed, borrowed
`ANIM` entries with their resource ordinals, raw advance thresholds, and
separate legacy low-byte behavior. Bitmap pixels remain undecoded here because
the general embedded resource set is not limited to the standalone 8-bit
subset. A selected animation record can now resolve each frame ordinal against
the same file's group-2 table through an exact-size borrowed iterator; an
invalid frame remains a typed result and does not hide later frames.
For the root-only corpus-backed subset, all six objects of either visual variant
can now prepare into an owned fixed-slot resource set. Every embedded bitmap is
strictly decoded while preserving source indices and its full embedded palette;
the exact mode-2 path installs no surface color key. Bounded-label/dimensions/pixels
matches share arena-scoped stable prepared IDs, entry24 selectors retain source
order and `-1` remains an explicit missing selector, and one bad optional object
rolls back only its new resources before later slots continue. Palette
attachment and backend upload remain boundaries for alternate hardware paths;
the software root path retains its sampled bindings directly. The prepared resource
also retains the raw low words of its originating DIB dimensions because the
legacy surface-layout path consumes those words rather than normalized decoded
extents.
Prepared selectors now feed the checked consecutive-run topology planner through
a combined constructor tied to the exact borrowed asset set. The resulting
fixed-slot plans preserve group order, source views, missing selectors, compact
first-use vertex ordinals, and preparation diagnostics without exposing an
unsafe arbitrary source/preparation join. Group-0 references now retain their
owning record and checked group-1 target, and the pre-scale mode-2 materializer
reproduces wrapping Q16 affine arithmetic, f32 field layout, the persistent
pass-through latch, opaque pre-mutation side vectors, source-order last-writer
corner mutation, missing selectors, and batch compaction. All reviewed root
records force pass-through. Per-slot loader state keeps the unresolved tuple
gate explicit. A second materializer derives the legacy surface layout from the
first interned resource occurrence, including wrapping width alignment,
quarter/half gates, and asymmetric square expansion. It applies vertical then
horizontal mapping-coordinate factors with one f32 store, shares first-resource
claims by source vertex ordinal across selector runs, preserves same-entry
duplicate multiplication, and only then compacts batches. The exact root corpus
has square resources and therefore takes the proven zero-padding/no-multiply
case under normal or quarter preparation. The software branch-zero backend now
owns sampled bindings and active-palette composition; alternate material
profiles and the unresolved tuple-source gate remain C3D boundaries.
The root transform oracle is now portable and deterministic. A source-owned
quarter-wave table reconstructs the exact 4,096-entry legacy trig table, while
square-root values use equivalent integer arithmetic. The fixed-point row
normalizer preserves wrapping sums, low-word products, signed shifts, the
complete-raw-angle zero branch, and the distinction between raw angles `0` and
`0x1000`. Root world matrices retain the recovered Q16 translation and convert
to exact f32 words; identity view, frontend projection, and checked viewport
words are available without a platform graphics dependency. The software root
path now consumes them through scene traversal, resource binding, rasterization,
and indexed back-buffer composition; other renderer paths remain separate.
Topology and both materialized runtime stages now also carry the same opaque,
unforgeable prepared-arena provenance. The planner can therefore reject data
mixed across independent preparations even when no concrete resource ID is
available to expose the mismatch.
The root-only frame planner now performs that join transactionally. It checks
the exact one-level root shape, resource selectors, compact batch payloads,
viewport, slot presence, variant, and provenance before changing an angle, then
owns source-ordered submission snapshots and the exact recovered transform and
numeric draw words. Concrete software binding, depth/scene execution, the pixel
oracle, and a headless terminal capture are now joined. The selected one-frame
visibility latch is now set transactionally and all six root latches are
cleared after the scene attempt. The depth-gated deferred-palette commit now
runs between that cleanup and presentation. Native presentation adaptation
remains next.
Prepared resources now map once into an arena-scoped opaque binding catalog.
The root batch executor consumes a plan, preflights the complete catalog join,
sets world state, and preserves the recovered resource-binding and persistent
selector-cache order across source batches and frames. Branch-specific
material/texture work remains inside the opaque backend binding. Bindings are
released in reverse order, and external render paths can explicitly invalidate
the cache. The software backend implements the bounded branch-zero binding and
headless frame capture and visibility-latch cleanup; native presentation
adaptation, alternate profiles, and a portable GPU implementation remain
separate.
The championship-car path now preflights a single-use actor plan against the
exact frontend binding catalog before any scene effect, then executes the
shared terminal C3D bracket with actor-scoped worlds, resource/default-zero
selector transitions, and Type-2 draw commands. The full championship terminal
owner now preserves inline callback order, coordinates root/car binding-cache
ownership, clears all 20 frontend visibility latches, commits the requested
palette, and presents last. Static recovery now separates the process-global
optional Type-2 word-5 pre-draw mutation from caller flag `0x08`, which affects
only a different world/spatial renderer in resolved code. A pure owned-copy
mutation contract models the retained gate, thresholds, distance/center tuple,
fixed truncating x87 qword conversion, and exact fixed-square-root lookup.
Each car frame now prepares fresh owned Type-2 words before terminal effects,
submits them after the recovered selector-binding decision even on cache hits,
routes the active mesh's exact all-`-0x10000` origin sentinel to the per-vertex
distance arm, then conditionally overwrites word-4 RGB while preserving alpha.
Frame planning records the renderer's list-head RGB owner for each sibling list;
the standalone session requires explicit source-ordered per-car actor tables
because it cannot inherit arbitrary process state. Count, ownership, and
profile/state disagreement are rejected before effects. The software backend
admits all four word-5/actor-RGB gate combinations for the otherwise identical
unified-indexed COPY/no-blend profile, copies texture indices directly, and keeps
missing selectors distinct as reviewed-car index zero. A public synthetic
asset-to-scene test pins resource/default/resource execution and exact indexed
color/depth output. Car-present scenes now invoke a backend-specific effect-free
preflight before any terminal pixel or state effect; an end-to-end software
car/palette/presentation test renders the car, commits a discriminating group-4
request, and captures the same indices under the new palette. A callback-scoped
software championship session now loads and retains all 14 car topology/runtime
pairs, explicit raster-profile provenance, compact car/FCHE palette layout,
shared bindings, and frame state; synthetic and reviewed-import tests cover
hidden-to-visible rendering, car switching, Next, presentation, and teardown.
The original device branch, alpha, word-5 mutation, and actor-RGB gates are
independent retained inputs. The alternate branch now has exact 16/24/32
selection priority and a bounded canonical-RGB MODULATE oracle, while its
runtime texture descriptors and framebuffer masks now have exact raw retention,
candidate classification, retained non-alpha selection, and separate role-safe
source-owned mask utilities. Canonical RGB can be converted into logical masked
framebuffer words without asserting byte order or pitch. A nominal source-owned
conversion stage now retains Palette8 storage or materializes RGB555/RGB565
logical words before the alternate oracle filters and modulates them. Driver
texture `Load` equivalence remains separate; a terminal oracle now packs only
depth-passing results through independent logical framebuffer masks and leaves
uncovered words unchanged. Native surface layout, effective filter, dithering,
alpha pixels, full-session integration, and presentation remain separate. A
standalone headless split backend now owns the logical target/depth lifecycle,
transactionally retains converted bindings, preflights every resolved batch
before effects, validates retained-cache coherence, and executes repeated
frames. Native layout, effective filter, dithering, alpha, indexed 2D
composition, alternate-session ownership, and presentation remain open. A
source-owned bridge can now expand a completed indexed frame through an
explicit palette into that logical target without replaying keyed/clipped 2D
work. It remains a deterministic integration oracle rather than a substitute
for source-by-source conversion. Focused static evidence now establishes the
ordinary `0x81` 8-bit-BMP path at a 16-bit destination: each source uses its
own embedded palette, discriminator byte `5` selects RGB555 while every other
value selects RGB565, the complete aligned stored rows are converted, and the
source key is packed embedded-palette entry zero. A tested nominal
`ordinary_bitmap_rgb16` implementation now preserves that complete logical
result and rejects malformed source state before returning output. DirectDraw
stretch, active-device format selection, and full source-ordered session
integration remain open. Matching-format fast opaque/keyed logical blits now
have effect-free preflight, bit-exact word copy, nonzero packed-key handling,
aligned-row bounds, and owner-scoped inactive-backend execution. Source-ordered
alternate-session integration remains separate. Alternate fonts are explicitly
separate: their initialized path prepares a `0x90` source, may invoke driver
`Load`, and queues a colored primitive. The exact no-driver-`Load` ARGB4444
decoder subset now has one persistent 16-style owner retaining aligned rows,
palette provenance, parsed glyph metadata, literal-zero keying, and exact-black
alpha selection. Queue-consumer inspection also pins three 2,047-entry lanes,
52-byte records, ordered lane flushing, primitive submission versus terminal
bit-`0x40` fallback, and the championship tuple/fade arithmetic. The setup
model now retains the raw four-byte source tuple and signed Q16 fade, including
the recovered terminal/inactive/active row selection, and a source-neutral
font preflight emits lane-neutral semantic glyph-producer inputs in
command/glyph order against the persistent ARGB4444 owner. A separate semantic
router now owns three independent typed lanes, accepts exactly 2,047 records
per lane, accounts for later silent drops, retains FIFO order, and applies the
recovered byte-3 gate after acceptance. That gate preserves the caller byte
only when effective renderer mode is not 7 and the recovered alpha-texture
capability bit is present; otherwise it forces `0xff`. Session callers select
the lane explicitly because ordinary championship lane 2 remains a strong
inference. Native queue packing/flush execution, driver conversion, tuple
channel meanings, blend semantics, and final pixels remain unresolved
alongside native layout and dithering. R_INFO, the 11
unique logo assets, and all 15 Chequered frames now also have one transactional
27-source logical-RGB16 owner built directly from decoded user assets. Stable
identity/path diagnostics and per-bitmap palette provenance are pinned. One
shared ordered kernel now prepares either rich indexed payloads or a
lightweight alternate transcript from exact source extents, with tests proving
full public-command parity across car, gear, and Next cold/warm frames. The
alternate adapter derives aligned extents from the 27-source RGB16 pool and an
optional nominal converted Selection source without indexed uploads. A
two-pass alternate executor rejects stale cache snapshots, preflights every
supported Fast bitmap plus Selection Stretch before
effects, and composes them in transcript order while returning text, NoOp, and
other deferred boundaries as exact typed outcomes. Selection uses the recovered
literal-zero key/call geometry and an explicitly source-owned center-nearest
policy because the legacy DirectDraw sampling kernel is not present in the
executable. The cache commit proves only that bounded ordinary-bitmap pass;
the alternate terminal now adds complete common-scene/car-batch preflight,
coordinates transform/latch/palette state inline, and runs the selected-car or
empty Next scene after setup on the same target. It explicitly skips the
non-indexed palette commit while preserving the pending request. Standalone
alternate session ownership now builds this complete retained slice directly
from a verified user import without an indexed backend or uploaded sources. It
loads and converts Selection once, executes it through the named source-owned
Stretch policy, validates caller-explicit RGB and raster policies before
callback entry, and preserves target/depth, binding cache, preview/transform/
latch/route/palette state across repeated car and Next frames. The session now
also loads and owns the selected frontend table and all 16 ARGB4444 font
sources, prepares every non-deferred setup Text command before terminal
effects, routes that slice from initially empty lanes through caller-explicit
lane and resolved byte-3 policy, and attaches the result only to successful
outcomes. It does not yet merge later deferred info/next-preview producers.
The publication guarantee covers the returned value, not rollback of setup or
scene effects after a later failure. Static follow-up now also resolves the
native record's optional viewport, angle/pivot, lane, tuple, and terminal fields;
the current championship producer supplies zero angle/pivots and no viewport.
A backend-neutral flush plan orders accepted records by lanes 3, 2, 1 with exact
lane depths and per-record textured/fallback dispatch. Pure preflight now
resolves every source and constructs the bounded zero-transform/no-viewport
quad with exact corner order, depth, RHW, diffuse/specular carriers, and
renderer-mode UV rule; optional backend validation still completes before any
frame effect. Fresh owner evidence places the unconditional dispatch inside the
common scene after identity-world reset and before its opaque tail/EndScene,
including an empty Next scene. The alternate terminal executes that semantic
dispatch and reports per-terminal counts, but no font pixel backend exists yet.
Native record packing/count clearing, optional transforms/UV swaps, device
state, filtering, blending, fallback pixels, driver-exact Stretch comparison,
native presentation, and the other deferred pixels are the next integration
seams.

The workspace now also has its first application executable. `cmr-app`
requires a caller-selected imported runtime tree, embeds only reviewed profile
metadata, and renders one fixed official-EU English/Normal root frame through
the production software session. A versioned digest contract binds the complete
indexed plane and raw palette, while the CLI emits only path-free counters and
checksums. The application, existing video vertical test, and their failure
tests share one dependency-neutral generated import fixture. This completes
runtime-data discovery and the headless executable part of the vertical slice;
window creation, an event/input loop, SDL presentation, audio, and GPU work
remain. Before the SDL step, expose the completed software frame through a
narrow presenter consumer so the proven startup/session/render orchestration is
reused rather than copied.

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

Current G03 recovery has isolated the signed force-candidate selector and the
pure arithmetic behind the nonzero control `+0x420` correction gate inside
`FUN_0040d810`; its two lookup-free initial-candidate branches are also
reconstructed. `FUN_0048fba0` proves the dependent trig, atan, and square-root
tables are runtime-generated math rather than asset payloads. The exact
source-owned `legacy_math::{sin_q16,cos_q16,sqrt_q16}` compatibility helpers
now centralize trig and legacy square root, while mathematical
`Fixed16::sqrt` stays distinct. The atan table is reproducible, but its API is
deferred until the combined path's full quadrant and zero-input behavior is
recovered. The remaining physics exit work is that lookup-dependent candidate
consumer, direction/state accumulation, surface direction/projection and add,
and geometric qualification around `FUN_0040fbe0`; the two adjacent pure
scalars are recovered. Direction vectors, projections, and stateful
accumulation remain separated from unsupported physical labels until recovered.

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
| DirectDraw / GDI | First native slice: software oracle presented by SDL_Renderer; provisional native 3D: wgpu, benchmarked against SDL_GPU before lock-in |
| DirectInput / WinMM joystick | Provisional: SDL3 keyboard/joystick; optional mapped gamepad after compatibility |
| QMixer / WinMM audio | Provisional: SDL3 audio with a reconstruction-owned mixer |
| Win32 timing/window loop | Provisional: SDL3 events plus a monotonic host clock |
| DirectPlay | explicitly designed transport after protocol recovery |
| Registry/install paths | portable configuration and data discovery |

The rationale, alternatives, compatibility constraints, and benchmark gates are
tracked in [`MODERN_BACKEND_RESEARCH.md`](MODERN_BACKEND_RESEARCH.md). SDL3 is
the selected host-layer direction; SDL_Renderer is only the first compatibility
presenter, not a mandate that native 3D use SDL_GPU.

Phase 4A is complete: `cmr-video-wgpu` and the Vulkan-only
`cmr-video-sdl-gpu` pass the same exact indexed-palette readback contract on the
locally available discrete NVIDIA and integrated AMD adapters. This establishes
the presentation primitive but does not select a winner. Phase 4B must feed
both candidates the same raw recovered C3D draw packets and benchmark a
caller-selected imported stage before I08 can complete or I09 can begin.

The Phase 4B correctness slice is also complete for the bounded opaque-COPY
stage profile: both candidates consume the same raw recovered C3D packets and
match exact readback on source-owned geometry, with shared tests for culling,
depth ordering, and palette mutation. The persistent prepared-scene
architecture now exists on both candidates: `PersistentRawC3dGpuBackend`
moves all allocation, palette mapping, and upload into a fence-completed
one-time preparation, and the hardware-proven timed path only encodes,
submits, and waits — no resource creation, upload, download, or mapping.

I08's decision lane is the correctness-gated `cmr-gpu-stage-benchmark`
(`cmr-gpu-conformance::stage_benchmark`). It reads one caller-selected imported
stage in place, composes the software stage frame as the oracle, prepares the
persistent scene once per candidate, and ranks only the steady-state
encode/submit/fence-wait interval. The gate is cross-backend byte equality plus
a documented 0.1%-of-frame nearest-neighbour sampling-tie budget against the
software oracle; a fixed-point hardware rasterizer cannot reproduce a float
software rasterizer at UV texel boundaries, and the installed-corpus worst case
is 189 of 307,200 pixels (0.062%) with the candidates identical to each other.
47 of the 60 installed EU stage scenes gate on the RTX 5090 and AMD 610M/RADV
paths; 12 see nothing at the placeholder origin camera and GR4 still hits the
unresolved 32x64 padded-layout surface case. The placeholder camera/palette and the origin
visibility limit are recorded in the evidence ledger. Captured steady-state
timings (median of three 240-frame runs after 32 warmup frames,
encode+submit / submit-to-completion in microseconds) show wgpu ahead on the
draw-heavy recovered stages: MC5 1235/98 vs 3110/91, NZ2 1296/88 vs 2253/72,
AU1 645/78 vs 1210/59 on the RTX 5090 (same ordering on RADV), while SDL_GPU
leads the 97-draw stage (66/75 vs 108/47). The CPU encode/submit interval,
which grows with prepared draw count, dominates. I09 therefore selects
**wgpu**, which also avoids SDL_GPU's external `glslangValidator` runtime
dependency, and wires it behind the shared contracts. The application's stage
and windowed-session commands now render through the persistent wgpu path by
default (`--renderer auto`), with the software compositor as the oracle and
automatic fallback: the scene is converted and uploaded once, each frame only
updates the camera VIEW matrix, and the GPU rasterizes into an 8-bit
palette-index target that the recovered indexed host presents unchanged.
Measured per 640x480 stage frame, the same workload composes in 79.6 ms
(software) vs 0.75 ms (GPU) on AU1, 67.9 vs 1.07 on NZ2, 101.4 vs 1.11 on
MC5, and 7.2 vs 0.16 on IN8. A zero-copy window presentation that skips the
index readback remains a host optimization.

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

