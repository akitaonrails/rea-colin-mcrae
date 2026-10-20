# Static subsystem map

This is a recovery backlog, not a claim that every imported API executes. Each
row begins with shipped-artifact observations and names the evidence needed to
define a clean compatibility interface.

| Boundary | Static anchors | Recovery question | Planned owner |
| --- | --- | --- | --- |
| launcher/video | `Game.exe`, COM interfaces, `CreateProcessA`, `HDPath`, `AVIPath` | What selects/plays intro video, and what exact conditions start `Rally.exe`? | `launcher` application |
| data roots | `HDPath`, `CDPath`, `AVIPath`, `WAVPath`, registry APIs | Which paths are required, and how are relative names normalized? | asset resolver |
| stored compression | zlib 1.1.3 strings; corpus gzip + `CMPR` envelopes | What bounds/errors/ownership does the loader use? | format crate |
| window/event loop | User32 window and message APIs | Which event ordering, focus, and pause rules affect simulation? | platform interface |
| graphics | DirectDraw, GDI palette APIs, image/surface source-path strings | What surface formats, palette rules, scaling, and flip timing are observable? | video interface + renderer |
| input | DirectInput plus WinMM joystick APIs | How are keyboard/joystick states sampled, calibrated, and mapped? | input interface |
| clock | performance counter, WinMM periodic timers, `timeGetTime` | Which clocks drive rendering, physics, menus, and audio? | clock/scheduler interface |
| sound effects | 16 `QSWaveMix*` imports, WAV assets; shipped DLL exposes 62 APIs | What channel, volume, frequency, position, and completion semantics matter? | audio mixer interface |
| music/video | WinMM MCI plus AVI/WAV path keys | Is playback file-, track-, or device-based, and what lifecycle is required? | media interface |
| persistence | `save\\cmRally.cfg`, registry reads/writes | What is mutable state, its layout, defaults, and failure behavior? | storage module |
| localization | language-file paths and ordered text tables | What indexing, encoding, fallback, and formatting rules apply? | localization module |
| models/scenes | C3D and stage-bundle path strings/files | What layouts and cross-file references build a drivable scene? | formats + scene loader |
| multiplayer | two DirectPlay ordinal imports | Which protocol/session behavior is actually reachable and safe to preserve? | deferred network interface |

## First focused queries

1. Complete the `Game.exe` registry-path helper and launcher state transition.
2. Trace `Rally.exe` entry and `HDPath`/`CDPath` references to recover source-root
   selection and path normalization.
3. Trace zlib wrapper entry points and one representative `CMPR` consumer.
4. Resolve mode-enumeration ordering, failure cleanup, and the later DirectDraw
   flip cadence now that creation and the first image/surface chain are mapped.
5. Trace QMixer initialization and one sound-load/play path.

Each result should update [EVIDENCE.md](EVIDENCE.md) and create or refine one
owned interface. Broad bulk decompilation is deliberately out of scope.

## Implemented contracts

- `cmr-formats::stored_asset`: bounded raw/gzip-`CMPR` storage decoding.
- `cmr-assets::AssetIndex`: safe Windows-style logical lookup against reviewed
  metadata.
- `cmr-core::startup`: loading/legal palette and bitmap selection for the four
  language directories present in the observed corpus.
- `cmr-formats::{bitmap,riff_palette}`: bounded decoding of the recovered
  startup BMP/RIFF-PAL subset, plus the exact 256-entry legacy palette contract.
- `cmr-core::surface`: backend-neutral indexed surfaces with explicit identity
  or recovered nearest-color index mapping against the active palette.
- `cmr-core::startup_surface`: end-to-end verified import, selected asset load,
  bounded decode, and indexed-surface construction with contextual errors.
- `cmr-core::blit`: recovered destination clipping, fast/stretch dispatch, and
  color-key flag planning without a platform graphics dependency.
- `cmr-core::display`: backend-neutral indexed display initialization with
  typed windowed/fullscreen topology, shared front/back palette ownership, and
  the recovered visible-front-then-back startup copy transaction.
- `cmr-core::source_store`: shared checked ownership for persistent indexed
  sources, including append-only logical keys, transactional upload rollback,
  explicit release, unwind cleanup, and reverse-order teardown; the font and
  fixed-slot root-bitmap pools are its first persistent resource clients.
- `cmr-core::root_menu_bitmap_raster`: fixed-slot surface preparation,
  transactional persistent upload, pool-to-visual binding, and checked
  per-command bitmap submission without reordering non-bitmap frame work.
- `cmr-core::root_menu_frame`: canonical transcript generation and strict
  sequential routing through recovered bitmap/footer executors; root C3D is
  planned at its transcript position and consumed once by the always-run
  terminal scene, followed by latch cleanup, the depth-gated deferred palette
  commit, and the distinct presentation boundary.
- `cmr-core::root_menu_c3d_scene`: backend-neutral depth-clear policy,
  view/viewport setup, BeginScene, optional prepared-batch submission, world
  reset, opaque ordered scene tail, EndScene cleanup, and selector-cache
  disposition, with checked prepared-pipeline provenance.
- `cmr-core::root_menu_c3d_visibility`: persistent six-slot raw visibility
  latches set only by successful prepared-frame planning and cleared after
  every terminal scene attempt before later frame-completion work.
- `cmr-core::window`: pure compatibility reducer for recovered Win32 message
  handling, activation-driven pause/restore state, queue dispatch decisions,
  and fullscreen-flip versus windowed-vblank presentation planning.
- `cmr-core::presentation`: platform-neutral execution of the recovered native
  presentation plan, including fullscreen sample-before-Flip invocation and
  windowed client-origin/vblank sequencing with subsequent-call VGA fallback;
  native window creation remains outside this contract. A bounded static
  search found no separate steady-frame windowed copy owner; any portable copy
  is therefore an explicit host policy rather than attributed legacy behavior.
- `cmr-core::deferred_palette`: recovered runtime-base logical mapping and
  current/pending palette state, plus the depth-8-only 256-entry terminal
  commit into an adapter and both observed software caches, now joined to the
  root terminal-frame orchestrator.
- `cmr-video::SoftwareIndexedBackend`: deterministic source-owned front/back
  framebuffers for exact-size indexed copies, recovered index-zero keyed glyph
  copies, and the bounded branch-zero root C3D software oracle; unrecovered
  stretch, alternate C3D profiles, and other color-key ranges are explicit
  boundaries rather than guessed behavior.
- `cmr-video::software_root_menu_session`: callback-scoped production assembly
  of the verified imported root resources, initialized software display,
  recovered disk-palette bootstrap, persistent source pools, C3D runtime state,
  and repeated complete headless root frames without a self-referential owner.
- `cmr-core::config`: opaque exact-size legacy configuration admission,
  recovered volatile-byte clearing and save gate, plus safe registry-value
  parsing and caller-owned exact-record writing without assigning meaning to
  unknown bytes.
- `cmr-core::localization`: shared five-language identity, distinct frontend
  and in-game path policy, bounded byte-oriented table splitting, and safe
  indexed lookup with the recovered diagnostic fallback.
- `cmr-core::localization_assets`: verified import loading for the recovered
  fixed-width frontend and in-game text tables with path-aware failures.
- `cmr-core::input`: platform-neutral keyboard and WinMM-compatible joystick
  masks, rising-edge state, raw menu-key repeat, frontend combination policy,
  and focus-gated sampling. OS polling and force feedback remain backend work.
- `cmr-core::timing`: validated pure plans for recovered logical rates, timer
  startup/drift, compatibility buckets, focus gating, and unsigned deadlines.
- `cmr-formats::font`: bounded fixed-layout indexed FON atlas/trailer decoding
  plus byte-text measurement and backend-neutral glyph layout plans.
- `cmr-core::font_assets`: verified loading and stable style identity for the
  16 recovered frontend font atlases in original initialization order.
- `cmr-core::font_raster`: backend-neutral indexed atlas conversion and
  index-zero keyed execution of precomputed glyph plans with diagnostics, plus
  transactional lifetime ownership for the persistent 16-font upload pool and
  an identity-neutral prepare/execute seam that rejects fatal text work before
  backend effects.
- `cmr-core::root_menu_footer`: phase-gated localized footer layout and indexed
  execution into the caller-selected active frame target.
- `cmr-core::frontend`: checked first/root-screen navigation, transitions,
  idle/exit effects, and opaque target IDs.
- `cmr-core::championship_setup`: pure three-row car/gear setup state with
  explicit eligibility fallback, animation gates, profile commits on both
  forward/back transitions, and a backend-neutral preview transcript.
- `cmr-core::championship_assets`: verified-store admission of the 14 canonical
  car C3Ds and palettes plus 11 deduplicated manufacturer-logo bitmaps, with
  typed source-relative failures and no bundled payloads.
- `cmr-core::championship_bitmap_raster`: explicit palette-mapped preparation
  and transactional shared-store upload for the 11 unique aligned logo
  surfaces, preserving manufacturer/car aliases.
- `cmr-core::championship_setup_visual`: validated backend-neutral title/row/
  preview frame planning with exact recovered geometry, order, fonts, and
  paired visible-callback palette cache behavior.
- `cmr-core::championship_setup_frame`: whole-frame preparation and ordered
  execution for recovered setup text and 2D bitmaps, with source preflight,
  success-only cache commit, and typed deferred unrecovered commands.
- `cmr-core::championship_car_frame_plan`: exact-source, actor-scoped planning
  for one selected mode-8 car. It validates the complete topology/runtime join
  before advancing preview state, composes the recovered Q16 hierarchy only
  when visible, and owns active-level Type-2 batches in depth-first attachment order.
  Backend submission, palette transition, and pixels remain separate.
- `cmr-core::c3d_type2_fog`: pure owned-copy pre-draw mutation for the optional
  Type-2 word-5 path. It explicitly carries the retained process-global gate,
  thresholds, distance/center inputs, and x87 conversion mode; disabled state
  is caller-selected and is not inferred from frontend entry.
- `cmr-video::c3d_software_raster` and `SoftwareIndexedBackend`: one explicit
  mutation-disabled, actor-RGB-disabled, unified-indexed COPY/no-blend car
  profile. It copies sampled indices directly, maps reviewed-car missing
  selectors to index zero, rejects every unsupported runtime-state tuple, and
  preserves the root renderer's separate palette-quantizing policy. A
  backend-neutral pre-scene hook rejects absent/unsupported car state before
  depth clear, and the full terminal test covers draw, group-4 commit, and
  captured presentation.
- `cmr-core::frontend_c3d_resources`: one frontend-session prepared arena with
  the exact 55-resource mode-8 car prefix followed by six fixed optional root
  slots, preserving cross-domain IDs, aliases, rollback, and diagnostics.
- `cmr-core::championship_frame_assets`: verified bounded loading of R_INFO,
  the 15 ordered Chequered preview frames, and FCHE, with explicit R_INFO
  mapping and transactional shared-source upload.
- `cmr-core::source_store::UploadedIndexedBitmap`: shared backend-neutral
  uploaded-bitmap identity and aligned extent used by root resources and ready
  for later frontend-specific pools.
- `cmr-core::indexed_bitmap_raster`: identity-neutral preflight and execution
  for persistent indexed bitmaps, including exact source-bound checks after
  legacy destination clipping.
- `cmr-core::root_menu_assets`: typed, verified-profile paths for localized
  root chrome, center icons, and normal/`g_` menu objects without asset bytes.
- `cmr-formats::c3d`: bounded, byte-preserving admission of the observed
  `CM3D` header and all three proven counted layouts, including raw group-0
  `u16` values, typed borrowed group-1 32/24-byte entries, and group-2
  label/blob pairs. The typed entries preserve raw bytes/offsets while exposing
  recovered signed Q16.16 component groups, local entry ordinals, the signed
  resource ordinal, byte-exact corner tuples, and unknown scalar fields,
  plus checked borrowed group-0→group-1, entry24 `+2/+4/+6`→same-record entry32,
  and group-1-entry24→group-2 ordinal resolution without assigning target
  content semantics. Group-2 resources may be classified on demand as opaque
  `BM` bytes, checked borrowed `ANIM` frame references/thresholds, or unknown
  bytes without forcing universal bitmap decoding. A checked selected-record
  iterator resolves animation frame ordinals to generic group-2 records while
  preserving source offsets and later results after invalid ordinals. Runtime
  conversion and batching remain separate.
- `cmr-core::c3d_batch`: checked, backend-free topology planning over one
  group-1 record and caller-prepared selectors. It preserves maximal
  consecutive selector runs and source entry order, deduplicates local entry32
  ordinals by first use within each run, and emits compact three-index streams
  while retaining borrowed typed entries. Its owned neutral draw command then
  carries a checked ordinal, explicit Type-1/Type-2 branch, selector, compact
  runtime vertices/side vectors/indices, and numeric primitive/flag constants;
  root commands remain lossless metadata adapters.
- `cmr-core::root_menu_c3d_resources`: policy-driven, backend-neutral
  preparation of embedded C3D bitmaps into an owned 2,048-entry arena. It
  accepts car or root identities plus explicit append/intern/material/layout
  policies, SHA-binds each prepared object to its exact admitted C3D bytes,
  preserves selectors and raw DIB metadata, and rolls back failed objects.
  Root fixed-slot preparation remains a parity-tested adapter.
- `cmr-core::c3d_topology`: crate-private single-object topology join that
  verifies source SHA-256 and selector arena before borrowing ordered group1
  records, resolving checked group0 references, and preserving consecutive
  selector runs and compact first-use ordinals. Public car/root APIs choose the
  source/prepared pair rather than exposing an arbitrary join.
- `cmr-core::root_menu_c3d_topology`: combined fixed-slot root preparation and
  metadata/error adapter over the domain-neutral topology seam.
- `cmr-core::root_menu_c3d_runtime`: domain-neutral resource-scaled CPU
  materialization through recovered wrapping Q16/f32 conversion, explicit load
  policy, pre-mutation side capture, type-1/type-2 mutation, first-allocation
  surface-layout UV scaling, first-usable-resource visitation, and compact
  batches. Root mode 2 remains a parity-tested adapter; the championship path
  closes over its SHA-checked mode-8 policy and fixed `0x96969696` word.
  Backend submission, palette integration, and rasterization remain separate.
- `cmr-core::root_menu_c3d_transform`: deterministic root-only fixed-point
  transform words with source-owned quarter-wave trigonometry, integer-derived
  square-root normalization, exact raw-angle branch behavior, persistent Q16
  placement, f32 world/view/projection words, and checked viewport construction.
  Scene traversal, binding, drawing, and rasterization remain separate.
- `cmr-core::root_menu_c3d_frame_plan`: transactional exact-root validation and
  non-cloneable owned one-frame submission snapshots, retaining source batch
  order, prepared-arena provenance, exact transforms, and numeric draw words.
  Backend resource binding and terminal scene execution remain separate.
- `cmr-core::root_menu_c3d_execute`: generic arena-scoped opaque RAII binding
  catalogs, whole-command resource preflight, and a commit-after-bind,
  explicitly invalidatable selector cache, plus the root source-ordered
  world/bind/draw adapter. Missing selectors remain explicit until a backend
  implements the recovered material/texture-zero operation. One frontend
  catalog can own car and root resources; scene/depth/latch/presentation
  orchestration remains separate.
- `cmr-core::frontend_c3d_scene`: shared checked frontend C3D transaction for
  optional depth clear, identity view, viewport, BeginScene, one owned
  submission, identity-world reset, opaque tail, EndScene, and best-effort
  post-Begin cleanup. The root API remains a compatibility adapter.
- `cmr-core::championship_car_execute` and `championship_car_scene`: exact
  actor-world/resource-or-default/draw submission for a preflighted car frame,
  including explicit missing-selector material/texture-zero binding and a
  persistent selector cache. Prepared frames are tied to the exact binding
  catalog and their planned extent; uncertain scene completion invalidates the
  cache.
- `cmr-core::frontend_c3d_route`: session-scoped ownership of the shared C3D
  backend binding state. Root and championship terminal owners invalidate the
  executor they enter only on first use or a route switch, while same-route
  frames retain the recovered selector cache.
- `cmr-core::championship_terminal_frame`: complete preflighted championship
  setup boundary. It executes deferred transform/latch/palette callbacks inline
  with 2D work, always runs the common scene (including an empty Next preview),
  clears all 14 car then six root latches, commits the deferred palette, and
  presents. Typed failures preserve the exact completed prefix and suppress
  later terminal work.
- `cmr-core::root_menu_c3d_assets`: exact six-slot optional loading of normal
  or `g_` root-menu objects with ordered asset/header diagnostics and stable
  holes, plus explicit selection-slot mapping and six independent wrapping
  frame-angle states bound to the exact parsed asset.
- `cmr-core::root_menu_visual`: exact phase-gated root chrome geometry, Q16
  backdrop crop and center placement, and an ordered asset/object transcript.
- `cmr-core::root_menu_animation`: pure activation, wrap, tween-schedule, and
  transition-suppression state for the root screen.
- `cmr-core::root_menu_bitmap_assets`: verified, bounded loading of required
  and optional indexed root bitmaps with retained optional diagnostics.
- `cmr-core::root_menu_bitmap_raster`: fixed-slot identity preparation using
  the active palette and unchanged indices, with typed optional/required
  conversion policy and no backend side effects.
- `cmr-core::root_menu_frame`: ordered bitmap/footer dispatch plus parsed C3D
  slot binding and angle advancement; the software backend now executes the
  bounded root C3D branch and captures the composed indexed frame through a
  headless terminal hook. Native Flip and synchronization remain host
  boundaries; a portable windowed copy is reconstruction policy because the
  shipped steady-frame path exposes no observed copy owner.

The canonical binary maps registry language values 0–4 to English, French,
German, Spanish, and Italian startup paths. The supplied corpus has no Italian
directory, so this profile intentionally exposes only its four present
languages rather than fabricating or silently aliasing Italian assets.

## Binary roles and variants

- `Rally.exe` is the gameplay executable: PE32 i386, fixed image base
  `0x400000`, relocations stripped, Visual C++ 6-era linker.
- `Game.exe` is a VC5-era DirectShow/COM intro-video launcher.
- `qmixer.dll` is QSound 3D Audio Wave Mixer 3.00 middleware over waveOut and
  optional DirectSound.
- The canonical code reference is the official EU updater payload. The
  installed EU-derived executable, official US payload, and unofficial xdelta
  results are tracked comparison variants.

The fixed-base/no-relocation assumption is part of historical compatibility,
not an implementation requirement for a modern portable binary.

