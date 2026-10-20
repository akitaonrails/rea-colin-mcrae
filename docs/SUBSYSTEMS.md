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
- `cmr-core::window`: pure compatibility reducer for recovered Win32 message
  handling, activation-driven pause/restore state, queue dispatch decisions,
  and fullscreen-flip versus windowed-vblank presentation planning.
- `cmr-video::SoftwareIndexedBackend`: deterministic source-owned front/back
  framebuffers for exact-size indexed copies and recovered index-zero keyed
  glyph copies; unrecovered stretch and other color-key ranges are explicit
  errors rather than guessed behavior.
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
  transactional lifetime ownership for the persistent 16-font upload pool.
- `cmr-core::root_menu_footer`: phase-gated localized footer layout and indexed
  execution into the caller-selected active frame target.
- `cmr-core::frontend`: checked first/root-screen navigation, transitions,
  idle/exit effects, and opaque target IDs.
- `cmr-core::root_menu_assets`: typed, verified-profile paths for localized
  root chrome, center icons, and normal/`g_` menu objects without asset bytes.
- `cmr-core::root_menu_visual`: exact phase-gated root chrome geometry, Q16
  backdrop crop and center placement, and an ordered asset/object transcript.
- `cmr-core::root_menu_animation`: pure activation, wrap, tween-schedule, and
  transition-suppression state for the root screen.
- `cmr-core::root_menu_bitmap_assets`: verified, bounded loading of required
  and optional indexed root bitmaps with retained optional diagnostics.

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

