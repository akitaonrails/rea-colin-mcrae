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
- `cmr-video::SoftwarePresentedFrame::digest_summary`: versioned canonical
  checksums for a complete indexed frame. The stream binds `CMRFRM01`,
  little-endian extent/pixel count, all 256 RGB-plus-flags palette entries, and
  row-major indices; component digests keep palette and pixel differences
  diagnosable. This identifies deterministic reconstruction output, not asset
  provenance or native display pixels.
- `cmr-app`: first native application boundary. It selects an external verified
  import only from `--runtime-data` or `CMR_RUNTIME_DATA`, embeds reviewed
  profile metadata but no payloads, initializes the software compatibility
  display/session, renders one fixed official-EU English/Normal root frame, and
  emits path-free counters and checksums. It is headless and one-shot; a shared
  frame/presenter seam must precede the SDL interactive host so this orchestration
  is reused rather than copied.
- `cmr-test-support`: non-publish synthetic imported-root fixture shared by the
  video vertical test and application integration test. It generates its own
  BMP/font/palette/C3D bytes, owns temporary source/import directories, and has
  no dependency on `cmr-video` or `cmr-app`.
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
  backend effects. This is the direct indexed branch; the initialized
  alternate `0x90` source preparation, queued primitive, and optional driver
  `Load` path is now statically separated but not yet implemented.
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
- `cmr-core::championship_bitmap_raster`: explicit active-palette-mapped
  preparation for the current 8-bit indexed output and transactional
  shared-store upload for the 11 unique aligned logo surfaces, preserving
  manufacturer/car aliases. This mapping is distinct from the recovered
  16-bit per-source embedded-palette conversion.
- `cmr-core::championship_setup_visual`: validated backend-neutral title/row/
  preview frame planning with exact recovered geometry, order, fonts, and
  paired visible-callback palette cache behavior.
- `cmr-core::championship_setup_frame`: whole-frame preparation and ordered
  execution for recovered setup text and 2D bitmaps, with source preflight,
  success-only cache commit, and typed deferred unrecovered commands. Its
  single ordered kernel now prepares either rich indexed payloads or a
  lightweight extent-only alternate stream. The public effect-free view
  preserves full text commands, exact bitmap source identities and blit
  decisions, and every deferred boundary without exposing indexed handles or
  pending cache state; exact-command parity tests bind both adapters.
- `cmr-core::championship_car_frame_plan`: exact-source, actor-scoped planning
  for one selected mode-8 car. It validates the complete topology/runtime join
  before advancing preview state, composes the recovered Q16 hierarchy only
  when visible, and owns active-level Type-2 batches in depth-first attachment order.
  Backend submission, palette transition, and pixels remain separate.
- `cmr-core::c3d_type2_fog`: pure owned-copy pre-draw mutation for the optional
  Type-2 word-5 path. It explicitly carries the retained process-global gate,
  thresholds, distance/center inputs, and x87 conversion contract; disabled state
  is caller-selected and is not inferred from frontend entry.
- `cmr-core::c3d_actor_rgb`: pure word-4 RGB packing that preserves alpha. The
  championship frame plan records the recovered recursive list-head owner for
  every draw, and preparation applies RGB after word-5 mutation while skipping
  the exact all-`-0x10000` origin sentinel path.
- `cmr-video::c3d_software_raster` and `SoftwareIndexedBackend`: explicit
  unified-indexed COPY/no-blend car profiles for all four word-5/actor-RGB gate
  combinations. It consumes owned prepared Type-2 words, copies sampled indices
  directly, maps reviewed-car missing
  selectors to index zero, rejects every unsupported runtime-state tuple, and
  preserves the root renderer's separate palette-quantizing policy. A
  backend-neutral pre-scene hook rejects absent, mismatched, or unsupported car state before
  depth clear, and the full terminal test covers draw, group-4 commit, and
  captured presentation.
- `cmr-video::c3d_software_raster` also retains the exact alternate renderer
  selection priority (`0x400`→16, `0x200`→24, `0x100`→32) and exposes a
  deliberately unintegrated canonical-RGB Type-2 MODULATE oracle. The
  `selected_texture_conversion_oracle` first materializes a nominal Palette8,
  RGB555, or RGB565 plane under an explicit no-dither policy; the compositor's
  type boundary requires that conversion before its named bilinear policy. A
  terminal source-owned compositor can pack depth-passing results through an
  independently selected logical framebuffer mask while preserving untouched
  words exactly. Driver `Load` behavior, native
  framebuffer layout, dithering, alpha, and backend presentation remain
  explicit boundaries.
- `cmr-core::root_menu_c3d_render_state` decodes the game's `0x00008000`
  capability-derived alpha gate, including its inverse pre-scene depth-clear
  decision and fixed source-alpha/inverse-source-alpha factors. It also records
  whether the requested branch filter is submitted or suppressed by the cache,
  preserving the distinction between a cached request and unknown effective
  device state. Alpha testing and alpha-enabled pixel composition remain
  unresolved and unintegrated.
- `cmr-video::legacy_c3d_pixel_format`: exact eight-dword legacy descriptor
  retention, evidence-bounded palette/RGB555/RGB565 candidacy, and separate
  safe mask validation. It retains separate palette and opaque-RGB callback
  slots and applies the explicit recovered palette gate without depending on
  renderer depth. Caller-supplied-mask pack/unpack is explicitly source-owned
  policy; driver conversion, byte layout, pitch, and dithering are not inferred.
- `cmr-video::ordinary_bitmap_rgb16`: the recovered ordinary `0x81` 8-bit-BMP
  source conversion, retaining complete aligned stored rows and each bitmap's
  embedded palette while packing exact discriminator-selected RGB555/RGB565
  logical words and the packed palette-entry-zero key. Its companion fast-blit
  module preflights matching-layout opaque/keyed logical copies, including
  aligned padding, before any write. A separate Stretch module preserves the
  recovered Selection rectangles, literal-zero key request, and layout checks,
  then applies an explicitly source-owned destination-cell-center nearest
  policy because DirectDraw sampling remains outside the executable. It is
  nominally separate from fonts, selected C3D textures, and framebuffer
  formats; native storage, driver-exact Stretch pixels, and the 4-/24-bit
  branches remain open.
- `cmr-video::software_championship_selection_rgb16`: nominal one-source owner
  that converts the exact common Selection bitmap once, retains its stable
  identity and media-relative path on failure, and prevents an ordinary
  info/logo/frame source from satisfying the Selection slot accidentally.
- `cmr-video::software_championship_ordinary_rgb16`: transactional fixed-shape
  ownership of converted R_INFO, 11 unique logo, and 15 Chequered sources,
  keyed by stable media-relative identities and retaining only contextual
  relative paths on failure. Selection Stretch and alternate fonts remain
  deliberately outside this pool.
- `cmr-video::software_queued_font_argb4444`: nominal persistent ownership of
  all 16 parsed fonts plus the exact aligned ARGB4444 source words produced by
  the bounded `0x90` decoder branch before optional driver `Load`. Exact RGB
  black alone receives zero alpha; other RGB receives alpha `0xf`, palette
  reserved bytes are ignored, and the installed source key remains literal
  numeric zero. This remains source preparation only: driver conversion,
  queue routing, blending, and final pixels stay outside the module.
- `cmr-core::font_raster::PreparedFontText` and
  `cmr-video::software_queued_font_glyphs`: source-neutral fatal validation
  plus lane-neutral semantic producer inputs for the alternate font path.
  Commands retain the raw four-byte tuple and signed Q16 fade; producer inputs
  retain the exact caller-side post-fade bytes, font style, source/destination
  rectangles, terminal `8` or `0x48`, command order, and glyph order without
  copying atlas ownership.
  `cmr-video::software_queued_font_queue` routes those inputs to a
  caller-selected typed lane, admits exactly 2,047 records independently per
  lane, tracks later silent drops, and applies the recovered byte-3 gate:
  preserve only when effective renderer mode is not 7 and the alpha-texture
  capability bit is set, otherwise force `0xff`. Safe out-of-atlas rejection
  remains a reconstruction policy. Native 52-byte packing, optional transform
  data, flush execution, filtering/blending, and pixels remain open.
- `cmr-video::software_queued_font_flush`: backend-neutral semantic flush plan
  for the current zero-rotation/no-viewport championship subset. It snapshots
  accepted records in recovered lane order 3, 2, 1 and per-lane FIFO order,
  attaches exact depths `0.999999`, `0.5`, and `0.0`, preserves the terminal
  split, resolves source owners, prepares exact bounded quad geometry, and can
  run a backend-specific validation pass before dispatch to an infallible
  typed sink. `software_queued_font_quad` supplies TL/TR/BL/BR corners, RHW,
  diffuse/specular carriers, and explicit renderer-mode UVs normalized against
  the aligned wrapper extent. The alternate terminal dispatches the prepared
  lanes after identity-world reset and before the common scene tail, including
  empty/Next scenes, and reports semantic counts. Native packing/count
  clearing, optional transforms/UV swaps, device states, blending, fallback
  pixels, and textured pixels remain outside this boundary.
- `cmr-video::software_championship_setup_frame`: two-pass execution of the
  lightweight prepared setup transcript. Its standalone preparation derives
  exact aligned extents from the owned RGB16 pool and optional nominal
  converted Selection source, with no indexed backend, uploads, fonts, or text
  table. Execution rejects stale preview-cache snapshots, preflights every
  effectful Fast bitmap and supported Selection Stretch before writes, and
  composes them in exact command order. Missing Selection at preparation stays
  typed deferred; losing the source after a Stretch was prepared is a typed
  preflight failure. Cache publication records completion of this bounded
  slice, not unresolved text/info/next-preview pixels.
- `cmr-video::software_championship_alternate_terminal`: whole-frame owner for
  the prepared RGB16 setup stream and optional selected-car scene on one split
  target. It preflights ordinary bitmaps plus the common scene and every car
  batch before writes, coordinates transform/latch/palette commands inline,
  runs a car or empty Next scene after setup, consumes car/root latches, and
  reports the non-indexed palette skip while retaining the pending request.
- `cmr-video::software_championship_alternate_session`: callback-scoped owner
  built directly from a verified `AssetStore`. It loads championship/frame/C3D
  assets, the selected frontend localization table, all 16 fonts, and the exact
  common Selection bitmap. It retains the font and 27-source RGB16 pools,
  converted split bindings, logical target/depth, executor cache, and all
  terminal state across repeated frames, and returns the owned headless backend
  after the callback. Each frame prepares every non-deferred setup Text command
  before terminal effects, routes it through caller-explicit lane/byte-3
  choices, and publishes the complete initially-empty bounded route for that
  non-deferred setup-text slice only in a successful outcome. RGB555/RGB565
  source-target and raster-state
  mismatches fail before callback entry. It creates no indexed backend, uploaded
  source, or native presentation surface. Selection is converted once and
  executed by the named source-owned Stretch policy; native queue flushing,
  glyph pixels, info text, and next-preview pixels remain deferred.
- `cmr-video::packed_rgb_target`: a nominal runtime-framebuffer format and
  logical masked-word target kept type-distinct from selected texture formats.
  It retains the complete raw surface descriptor and converts canonical RGB
  or a completed indexed plane plus explicit palette under validated masks,
  without exposing or inferring native byte order, surface pitch, dithering,
  palette provenance, or presentation. Completed-indexed-frame expansion is
  an integration oracle, not the recovered per-source bitmap conversion.
- `cmr-video::c3d_software_split_backend`: a standalone headless alternate
  backend that transactionally retains converted frontend bindings, owns an
  owner-scoped logical target plus depth buffer, preflights the complete frame
  including retained selector-cache coherence, and preserves color across
  depth-only clears. The championship alternate terminal now composes ordinary
  Fast bitmaps and the selected-car or empty scene in recovered order on this
  same target; full session ownership, native storage, and presentation remain
  separate boundaries.
- `cmr-video::software_championship_session`: callback-scoped standalone
  assembly retaining all 14 prepared car topology/runtime pairs, one shared
  binding executor, explicit raster-selection, optional source-ordered per-car
  actor-RGB tables, and 2D mapping policies, compact car/FCHE palettes,
  preview/transform/route state, and persistent uploaded sources. It validates
  every actor table against loaded topology, selects car resources only from
  the validated transcript, and releases every upload before returning the
  backend. The compact palette
  indices and supported software-oracle raster tuple are source-owned policy,
  not imported process-global state.
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
  optional DirectSound. The official-EU `Rally.exe` audio module lives at
  `0x4957d0`–`0x495a80`: `0x4957d0` initializes a 22050 Hz, 30-channel QSound
  session via `QSWaveMixInitEx`/`QSWaveMixActivate`; `0x4958e0` builds a PCM
  `WAVEFORMATEX` from (name, rate, bits, stereo) and registers it with
  `QSWaveMixOpenWave` into a fixed wave-slot table; `0x495a30` plays a slot on a
  channel through `QSWaveMixPlayEx`. Per-channel control wrappers follow at
  `0x495b70` (volume), `0x495bd0` (frequency), `0x495c30` (3D position),
  `0x495b10`/`0x495b30` (stop), and `0x495d10` (channel-busy query); panning is
  3D-position based over the 30 channels. See evidence
  `rea.official-eu.audio-wave-path` and `rea.official-eu.audio-mixer-controls`.
- The reconstruction-owned mixer interface for this surface is
  `cmr-core::audio_mixer`: a backend-agnostic `AudioMixer` trait plus a pure
  deterministic `MixerSessionState` that reproduces the recovered init guard,
  the 22050 Hz/30-channel cap, the incrementing wave-slot table, the PCM
  `WAVEFORMATEX` build, and the per-channel volume/frequency/3D-position/stop
  records as emitted `MixerCommand`s, all checked by a shared
  `check_audio_mixer_contract`. A QMixer-faithful backend and a portable/SDL
  audio backend both remain to be implemented against this interface. See
  evidence `impl.audio-mixer-contract`.
- The weather/crowd ambience loader `0x41cd30`/`FUN_0041cd30` registers six
  waves via the load wrapper at 11025 Hz / 16-bit: `sounds\crowd\crowd.wav` and
  `sounds\weather\abduct.wav` unconditionally, then `clap1`/`clap2`/`rumble1`/
  `rumble2` only at the storm weather level (`FUN_0046b230()==3`). This recovered
  inventory is encoded as metadata in `cmr-core::audio_weather_assets`. Thirteen
  further load-wrapper callers (engine, surface, UI, …) remain to enumerate. See
  evidence `rea.official-eu.audio-weather-wave-inventory` and
  `impl.audio-weather-assets`.
- The ambience trigger tick `0x41cfe0`/`FUN_0041cfe0` drives those waves on one
  shared channel: a proximity-summed crowd volume (clamped to 32000) plays the
  crowd slot looping (`QSWaveMixPlayEx` flag `0xffffffff`) while crowd objects are
  in range, and a storm selector plays a one-shot thunder variant (flag `0`) at
  `base+2+selector`. The volume setter `0x47eb70`/`FUN_0047eb70` applies a 0..100
  master-volume percentage (`DAT_004f653c`) over the recovered `0x495b70`
  wrapper. The recovered play flags, master-volume scale, volume clamp, and
  thunder selector are encoded in `cmr-core::audio_mixer` and
  `audio_weather_assets`. See evidence `rea.official-eu.audio-ambience-trigger`.
  A third source, the abduct trigger `0x41d270`/`FUN_0041d270`, plays slot
  `base+1` one-shot at fixed volume `0x7fff` and, with the storm flag, suppresses
  the looped crowd voice while it holds the shared channel (crowd plays only when
  `DAT_0050d868==0 && DAT_0050d860==0`). Only the crowd slot loops. See evidence
  `rea.official-eu.audio-abduct-trigger`.
- Both ambience ticks are reconstructed as pure command-emitting functions in
  `cmr-core::audio_ambience::AmbienceController`, checked against the recovered
  branch behavior (evidence `impl.audio-ambience-controller`). The per-frame
  scheduler that calls them, and the thirteen other wave loaders and their play
  trigger sites, remain to recover.
- The full recovered sound namespace (string table) is catalogued in
  `cmr-core::audio_assets`: explicit waves (crowd, six weather incl.
  `wet_out`/`rain_in`), `frontend`/`replay` music, menu sounds, and the
  `car`/`accident`/`prangs`/`newsurfs`/`speech`/`rallschl` directory groups. See
  evidence `rea.official-eu.audio-sound-taxonomy`.
- The menu UI sound loader `0x468f50`/`FUN_00468f50` registers four 22050 Hz /
  16-bit mono waves - `FORWARD`/`BACKWARD`/`LEFTRIGH`/`UPDOWN.WAV` under
  `sounds\menu\` - on a dedicated full-volume menu channel (`DAT_0057c608`, base
  slot `DAT_0057d88c`), inferred frontend navigation sounds. The separate
  `BACK`/`SELECT`/… strings are not this loader's waves. See evidence
  `rea.official-eu.audio-menu-loader`.
- The complete bare wave-filename inventory is recovered (string table): the
  car engine model (twelve cars × six `{RT,FT,RM,FM,RS,FS}` loops + shared
  gear/ignition/pop/chatter/whine, loaded by `0x4872b0`), the co-driver pacenote
  call corpus, the surface/collision material set, extra menu waves, tutorial
  numbers, and per-language cheat/secret sounds. The structured car model is in
  `cmr-core::audio_car_assets`. See evidence `rea.official-eu.audio-wave-inventory`.
- The car loader body `0x4872b0`/`FUN_004872b0` is recovered: it selects the
  player car from the low nibble of a config byte (`DAT_00d3ca80+0xb6`) via a
  switch remap (row 0 = SUBARU), allocates slots `+0..+7` (and `+8..+0xf` when
  `DAT_00550c08==2`), and loads 14 waves at 11025 Hz/16-bit/mono in order
  `FS,RS,FM,RM,WHINE,FT,RT,CHATTER,POPLOOP1-4,IGNITION,GEAR`. The format, load
  order, and selector remap are in `cmr-core::audio_car_assets`. See evidence
  `rea.official-eu.audio-car-loader`.
- The engine tick `0x487e00`/`FUN_00487e00` (+ whine sub-tick `0x488ab0`) drives
  the looping engine layers: each layer on its own channel
  (`base + slot + player*8`), updated per frame in volume and pitch. Three rev
  bands are stereo pairs (Slow 0/1, Mid 2/3, Top 5/6; WHINE 4, chatter 7), front
  slot pans one way and rear the other. The band index is
  `((RPM-lo)*count)/(hi-lo)` with out-of-window muting (pitch `0x2b11`) and
  overlapping windows giving the crossfade; the Top band is throttle-gated
  (`DAT_00829548`). Crossfade primitives are in `cmr-core::audio_engine`. F/R is
  a stereo-pan split, not on/off power. See evidence
  `rea.official-eu.audio-engine-crossfade`.
- The menu play helpers (`0x467ef0`, `0x467dd0`, siblings) each set full volume,
  stop the menu channel, and play `base+offset` one-shot: offset 0 `FORWARD`
  (confirm, carved) and offset 3 `UPDOWN` (vertical nav, carved); offsets 1
  `BACKWARD` and 2 `LEFTRIGH` by slot order. Reconstructed in
  `cmr-core::audio_menu::play_menu_sound`. See evidence
  `rea.official-eu.audio-menu-play`.
- The surface and collision sounds are two further banks, each with its own
  slot/channel base. The collision/prang loader `0x469ac0` reserves 5 channels at
  full volume `0x7d00` and loads 48 named `prangs`/`accident` waves; impacts play
  one-shot via `0x469e80` (speed picks a sub-wave and scales volume, a named
  material wave layering on `channel+1` for hard objects `0 < objType < 0x11`).
  The surface/skid loader `0x4892b0` (`newsurfs`) reserves 4 looping channels and
  loads per-object-type variant waves named `<type>_o/i/sko/ski<n>.wav` (surface
  outer/inner, skid outer/inner); roll play `0x46a030` ramps volume
  `base/3 + (count-5)*base/0x2d` to full at `count>=0x24`. Both banks are
  11025 Hz/16-bit mono. The roll ramp, variant naming, and bank constants are in
  `cmr-core::audio_surface`; the impact sub-wave/volume bit-expression is left in
  the evidence ledger pending a confident transcription. See evidence
  `rea.official-eu.audio-surface-collision`.
- The co-driver pacenotes (speech) are a 40-wave bank loaded once by
  `0x444160` ("CoDriver_Create") at 11025 Hz/16-bit/mono, slot offset == load
  index (slot 0 `CAUTION` ... 0x27 `INTO`, with the number calls `1..5` at
  3..7). Each path is `<root>` + `<language dir>` + `codriver\` + filename; the
  language is `cfg[0x18] & 7` over `english/french/german/spanish/italian/helium`
  (`helium` is the forced pitched-up developer voice). Calls play one component
  at a time, one-shot, on a single shared speech channel gated by
  `IsChannelDone`; a packed "pacenote word" carries several fields the sequencer
  `0x444450` extracts (direction 1=RIGHT/2=LEFT, a number, and higher fields of
  unknown meaning), linking consecutive queued notes with `INTO`. An 8-entry
  event ring feeds a depth-2 per-track FIFO with eviction-cancel. Reconstructed
  in `cmr-core::audio_pacenote` (inventory, format/channel constants, language
  gate, `codriver\` path, and the observed field-extraction arithmetic); the
  higher-field semantics and FIFO accumulation are left in the ledger. See
  evidence `rea.official-eu.audio-pacenote`.
- Music (`frontend.wav`, `replay.wav`) is loaded whole - not streamed - through
  a second loader variant `0x4959f0` (managed handle table, fixed format args,
  vs. the base `0x4958e0` rate/bits/stereo loader) and played looping; the
  frontend start `0x443b20` plays once and the keep-alive `0x4c9d70` re-starts
  the loop when the channel reports done and fades it in. All music is gated by
  the sound-disabled flag `DAT_00d3ca88`. The start-of-stage countdown
  (`5-4-3-2-1-GO`) is six `countdwn\` waves preloaded by `0x46a740` ("PreSnd")
  into consecutive slots at 11025 Hz/16-bit/mono under `<speech base>` +
  `<language dir>` + `countdwn\`, played one-shot as slot `base+index` by
  `0x46af10` on a shared SFX channel (cadence driver `0x439b10`); rally-school
  mode adds the `intro2\` set. The channel-setup helper `0x4958a0` (open +
  enable) is shared by 15 call sites across every audio bank, and `0x47eba0` is
  a second master-volume bus beside the known `0x47eb70`. Reconstructed in
  `cmr-core::audio_music_cues` (music tracks/flags, countdown set/format/path);
  the volume scaling, countdown slot ordering/cadence, and replay/SETUP control
  flow are left in the ledger. See evidence
  `rea.official-eu.audio-music-countdown`.
- The first concrete audio backend `cmr-core::audio_render::SoftwareMixer`
  implements the recovered `AudioMixer` contract as a portable, I/O-free PCM
  mixer: it renders per-channel voices (one-shot/loop, linear volume gain,
  `frequency/output_rate` playback step, nearest-neighbour resample, mono/stereo)
  into an interleaved stereo buffer at 22050 Hz, drawing decoded waves from a
  `WaveSource`, and passes the shared `check_audio_mixer_contract`. Volume/
  frequency/position realisation is a documented design decision (the QMixer
  scales are unknown); 3D position is recorded but not yet spatialised. A later
  platform/SDL device backend wraps it by decoding the user's WAVs into the
  `WaveSource` and pumping `render` into the device. See evidence
  `impl.audio-render`.
- `cmr-formats::wav::parse_wav_pcm` decodes the game's PCM `WAVE` files (RIFF,
  PCM mono/stereo at 8/16-bit) to interleaved `i16`, bounds-checked and
  rejecting malformed/truncated input with field-specific errors while retaining
  unknown chunks and trailing bytes; `cmr-core::audio_render::PcmWave::from_wav`
  bridges a decoded wave into the software mixer. This is the decode half of the
  WAV -> `WaveSource` -> `render` playback chain; the remaining half is the
  file/directory wave source and the SDL audio-device wrapper. See evidence
  `impl.audio-wav`.
- `cmr-core::audio_render::FileWaveSource` is the directory-backed `WaveSource`:
  it maps a game wave name to a safe relative path under the user's source root
  (backslash-normalised, traversal/absolute/drive/NUL refused, regular-file
  only), reads the WAV in place and decodes it with the PCM parser. It completes
  the WAV -> `WaveSource` chain; only the SDL audio-device wrapper (opening a
  22050 Hz stereo device and pumping `render`) remains for live playback. See
  evidence `impl.audio-file-wave-source`.
- `cmr-platform-sdl3::audio::AudioHost` is the SDL3 audio-device wrapper: it
  opens a 22050 Hz/stereo/s16 playback stream and, in the SDL callback, locks a
  shared `SoftwareMixer<FileWaveSource>` and pumps `render` into the device
  buffer. The game drives the recovered subsystems against the shared mixer
  handle (`AudioHost::mixer()`). This closes the audio chain end to end:
  recovered trigger logic -> `AudioMixer` commands -> software mix -> WAV decode
  -> SDL device. The device glue is runtime-only (not unit-tested); everything
  it relies on is contract/unit-tested in `cmr-core`/`cmr-formats`. See evidence
  `impl.audio-sdl-device`.
- `cmr-core::audio_mixer::apply_command`/`apply_commands` bridge the recovered
  command-emitting controllers to any `AudioMixer` backend by dispatching each
  `MixerCommand` to the matching method (`OpenWave` rejected - loads go through
  `load_wave`). An end-to-end test drives `audio_menu::play_menu_sound` through
  the bridge into the `SoftwareMixer`, confirming the recovered stop-then-play
  shared-channel behaviour produces the expected samples. This is the seam that
  lets the game's recovered audio logic play through the software/SDL backend.
  See evidence `impl.audio-command-bridge`.
- The driving simulation runs on a fixed 25 Hz / 40 ms step. WinMain
  `0x49db00` pumps Windows messages and calls the frame driver `0x4c1e00` ->
  HighLevelTimer tick driver `0x443e90` -> FSM tick `0x443db0`, a
  return-next-state machine on enum `DAT_005590a8` over a handler table at
  `0x4ee830`/`0x4ee840` (ids 2..0xc; in-race states `5..=10`). The step is
  `timeGetTime` integer-millisecond bucketing (`t/0x28`, 0x28 = 40 ms) with
  catch-up and a decoupled present pump - no float dt accumulator. The in-race
  update `0x43afd0` ("Mechanics") loops the active cars (`DAT_00550e90`).
  Reconstructed in `cmr-core::sim_clock` (step constants, bucket/boundary
  predicates, in-race gate). See evidence `rea.official-eu.sim-fixed-step`.
- The simulation's numbers are signed 32-bit Q16.16 fixed-point (`0x10000`=1.0;
  canonical `(i64)a*(i64)b>>16` multiply; dt coefficient `0xa3d`~0.04) with
  16-bit BAM angles into a 4096-entry trig table `DAT_0082d380`
  (`sin=table[a&0xfff]`, `cos=table[(a+0x400)&0xfff]`); floats appear only at
  `__ftol` input boundaries. Reconstructed in `cmr-core::fixed_point` (`Fixed16`
  with the recovered multiply/units and BAM trig over a reconstructed sine LUT,
  the game's table values not copied). Primary car state base `DAT_00700300`
  (stride 0x768); control/telemetry base `DAT_005fffc8` (stride 0x464). See
  evidence `rea.official-eu.sim-fixed-step`.
- The per-car vehicle dynamics run from the dispatcher `0x407690` (current-car
  globals `DAT_0050c604`/`DAT_0050c5b4`): input smoother `0x4088c0`, gearbox
  `0x41b5b0` (auto `0x41b810` / sequential `0x41bb30` / mode-2 `0x41bf00`), RPM +
  rev limiter `0x40d0c0`, torque `0x40ce40`, and the force/velocity integrators.
  Gear is a signed byte (`0..=6` forward, `7` reverse; zero ratio = neutral);
  auto-shift compares RPM against per-gear `ratio*redline` thresholds; the rev
  limiter clamps `[0,redline]` with a `min(margin,1.0)*0x51eb` cut; engine torque
  is `throttle*maxTorque - engBrake*rpm^2`, delivered as `torque*ratio*finalDrive`
  split across the wheels; the tachometer/engine-audio RPM is `norm*6500+2000`
  clamped `[2000,8500]`. Reconstructed in `cmr-core::drivetrain` (all Q16.16).
  Per-car-type params live at `~0x50e170` (stride 0xcd0), selected by the
  car-type byte `car+0x738`. The actuator element order, the torque->velocity
  integrators, and the ratio-table loader remain. See evidence
  `rea.official-eu.sim-drivetrain`.
- The grip/surface response and velocity integration run as later passes of the
  same `0x407690` solver chain. The lateral-grip constraint `0x41b270` strips the
  sideways velocity component each step (`v -= (v - fwd*dot(fwd,v))*K`, `K`~0.02
  normal / ~0.01 handbrake); the slope-grip loss `0x408f50` ramps grip loss from
  tilt ~0.70 up to a ~0.90 cap; the integrator `0x40a4a0` computes
  `accel = force*invMass + gravity`, quadratic drag `v*speed`, and
  `v += accel*0.2` (the 40 ms step folded into the gain). Per-wheel surface id
  `ctrl[0x42a+k*2]` indexes 8-byte grip records with ~0.24 (high) / ~0.15 (low)
  tiers. Reconstructed in `cmr-core::vehicle_dynamics` over a new Q16.16
  `cmr-core::fixed_point::Vec3`. The slip-angle tyre saturation `0x40d810`, the
  position update, the damper source, and the runtime-loaded per-surface/per-car
  table values remain. See evidence `rea.official-eu.sim-grip`.
- Collision/damage runs from `0x410a60` (Mechanics callee), which delegates the
  crash path to `0x4115d0` once the out-of-control flag `car+0x680` is set. The
  contact impulse accumulates into `car+0x3cc..`; its magnitude feeds the impact
  computer `0x41f630` (threshold `0x2624`, slope `0xf536`, clamp 1.0), and the
  same 0..1 impact drives the pitched impact sound (`0x469e80`), force feedback,
  and damage. Damage is tracked twice: a coarse crash counter `car+0x678` (+1 per
  contacting tick, clamp 100, totalled latch `car+0x67c`) and a fine per-region
  mesh accumulator `DAT_0050e0d8` (`w = (1-dist^2)*impact`, clamp 1.0), with body
  panels shed at region-damage `0x3333` / `0x6666` (`0x44a770`). Crash reactions
  (`0x4756f0`) pick a sound bank by severity (tier-1 max `0x4ccc`) and material
  (5=METAL/8=HAY_BALE/0xd=PLASTIC/0x10=RUBBER). Out-of-bounds is handled by
  node-keyed stage trigger volumes (`0x403a00` on `car+0x750`): type 6 kills
  (`car+0x67c`), 0x15 arms the `car+0x75c` countdown, 7..=0xB switch surfaces.
  Reconstructed the impact, crash counter, deform/region accumulate, damage
  level, materials, and trigger types in `cmr-core::collision` (with
  `Fixed16::sqrt`/`Vec3::length` added to `fixed_point`). The geometry
  intersection, the re-righting blend, the reset trigger (`ctrl+0x3ec` only read,
  no position-snap/penalty confirmed), the damage->performance read site, and the
  `car+0x75c` countdown remain. See evidence `rea.official-eu.sim-collision`.
- Stage timing uses the global clock `DAT_005ffc00`, a Q16.16 value in seconds
  advanced once per physics tick (step `2^32/(rate*0x10000)` = `0xa3d`~0.04 s at
  25 Hz, the same dt as `sim_clock`; `0x51e` at 50 Hz), saturating at
  `0xe100000` = 3600.0 s. It is a deterministic tick accumulator, not a
  `timeGetTime` wall clock, so a reset or off-course costs real time implicitly.
  A per-car start timestamp is latched once (`0x47fc50`); elapsed time is read as
  whole seconds (`(now-start)>>16`, `0x47fcf0`) or centiseconds
  (`sec*100 + frac*100>>16`, `0x47fd20` - the stored/displayed unit). Checkpoints
  are type-0 trigger volumes de-duplicated per car by a passed-bitmask
  (`volume+0x2e |= 1<<(car&0x1f)`, `0x403ee0`) that also enqueues the pacenote
  callout; finish (type 1) freezes the player time `DAT_00573a80` and per-car
  time `DAT_005ffbf0[car]` and moves the FSM from in-race state 5 to the
  stage-results state 8. There is **no additive time-penalty system** (no
  false-start/jump/missed-checkpoint/off-course surcharge; the clock never
  pauses) - a recorded negative result. Reconstructed the clock step/advance/cap,
  the seconds/centisecond readouts, the start latch, the checkpoint passed-bits,
  and the finish constants in `cmr-core::stage_timing`. The reference-split
  comparison, the best-time persistence, and the per-driver results record
  remain. See evidence `rea.official-eu.sim-stage-timing`.
- Co-driver call scheduling has two queues: an 8-entry audio ring
  (`DAT_005590c8`, one packed note-id per entry, head/tail mod 8) and a depth-2
  interleaved visual FIFO (`DAT_004f2120`) that drives the on-screen pacenote
  strip (`DAT_0057c178`, stride 0x14, 0x12/track). Submit (`0x444380`, called
  only from the trigger walker `0x403a00` on a type-0 volume, de-duplicated per
  player) optionally swaps the direction modifier (bits 7-8, mirror stages), then
  enqueues the whole id into the ring (blind, no full check) or routes it to the
  visual FIFO. The sequencer (`0x444450`, once per frame before Mechanics, gated
  by `QSWaveMixIsChannelDone` on the single shared speech channel) plays one
  bitfield group per tick - lowest field first via the `audio_pacenote`
  field table - clearing it and writing the remainder back, advancing only when
  the entry hits 0; a following word field (`& 0x78`) inserts the INTO link
  (slot 0x27) for the "corner-A into corner-B" cadence. Scheduling is
  event-driven off checkpoint crossings and channel-free status, with no
  clock/distance sampling (look-ahead is authored into trigger placement); the
  resolved `9*(mod-1)` is a display-row bump, not an audio stride. Reconstructed
  the ring, the incremental sequencer, the modifier swap, the into-link test, the
  routing, and the display arithmetic in `cmr-core::pacenote_schedule`. The
  display-record layout and the mode-gate semantics remain. See evidence
  `rea.official-eu.sim-pacenote-schedule`.
- There is no on-track AI opponent and no single-player ghost: the mechanics
  loop simulates exactly `DAT_00550e90` cars, which the race setup (`0x481350`)
  never sets above 2, and the only second car is a second *human* (split-screen
  `0x485bb0` or network `0x401060`; replay forces 1). No code steers a second car
  from a racing line - the node array `DAT_0057ae6c` is a timing spline read to
  record times (G05), not a driving line. Rival WRC drivers are a name roster
  (`0x507300`, 8 records) feeding the standings UI; their times are authored data
  outside the executable. Car-vs-car collision (`0x442740` -> `0x441f30`) runs
  only when two cars are present, not in replay, and split or network.
  Reconstructed the participant-count model, the dispatcher count, and the
  collision gate in `cmr-core::race_participants` (record strides as format
  metadata; name strings not vendored). See evidence
  `rea.official-eu.sim-race-participants`.
- Replay has two subsystems. The authoritative trace (the "Replay" class,
  `0x46d7b0`) is input-based re-simulation saved to `.SSR` files: a `0x40`-byte
  header, a per-frame `0x151ec`-byte initial-state seed, then `nTicks` (7500 at
  25 Hz = 300 s) 4-byte packed control records (actuators quantized `>>10` into
  6-bit fields, XOR-delta coded, plus gear/flags). Playback decodes the inputs
  back into the control struct and re-runs the normal physics (`0x43afd0`), with
  collision suppressed and the `0x462ac0` tracker used under the playback flag;
  the live frame driver `0x437690` bails and `0x437ef0` drives instead. Files go
  through the shared loader `0x495ed0`, whose optional archive container carries
  the `"CMPR"` magic + size. The same per-car input stream serves the local
  action replay, the `.SSR` shadow, and the network replay car. A separate
  render snapshot buffer (`0x46c7d0`, gated by the record flag) captures 24-byte
  pos+compressed-orientation keyframes at 6.25 Hz (cap 0x8ca x 6 cars) and
  interpolates them into the render transforms for smooth display only - it
  never feeds physics. Reconstructed the object sizing/region layout, the
  actuator quantize/XOR-delta codec, the CMPR header, and the snapshot
  indexing/compression/interpolation in `cmr-core::replay`. The per-byte actuator
  bit assignment, the Region-A layout, the save path, and the wire framing
  remain. See evidence `rea.official-eu.sim-replay`.
- The canonical code reference is the official EU updater payload. The
  installed EU-derived executable, official US payload, and unofficial xdelta
  results are tracked comparison variants.

The fixed-base/no-relocation assumption is part of historical compatibility,
not an implementation requirement for a modern portable binary.

