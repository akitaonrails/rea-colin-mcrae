# Colin McRae Rally reconstruction

This project aims to reconstruct a buildable, portable implementation of the
original Windows game from static analysis. The first target is behavioral and
data compatibility with a precisely identified shipped build. Platform
modernization comes afterward, behind compatibility interfaces.

The repository intentionally contains no original game assets or binaries. A
user will point the tooling at their own installation; the importer will verify
the source version and prepare a local, ignored `runtime-data/` tree.

## Current status

The reconstruction map currently contains 117 complete and 2 partial items.
The supported baseline is the reviewed official-EU executable and installed
EU-derived assets. Other editions and locales remain unsupported until
separately validated.

The workspace includes the fixed-step game/session orchestration, four-wheel
physics and surface recovery, collision/timing, championship progression,
menus and persistence, indexed C3D rendering with a wgpu backend and software
oracle, SDL window/input/audio hosts, PCM music and portable video contracts,
DirectPlay session/player records, and opt-in direct TCP transport. Native
DirectX/QMixer/DirectInput/DirectPlay DLLs are not required by the portable
build.

The native single-player stage supports live keyboard/gamepad driving, authored
`.csp` starts and `.cat` checkpoints, layered stage scenery, the real stage
palette, a caller-selected in-race car model, and a live follow camera. Its
normal single-player HUD is composed from authenticated user-owned bitmap/font
inputs. Planar dynamics, gear/rev telemetry, and control strengths remain
explicit playable harness approximations. Recovered menu-to-race selection,
the layered GPU path, and post-race sequencing remain open.

Unsupported boundaries are explicit: native driver-owned padding and exact
font/filter/alpha output, alternate type-1 lighting, a concrete video decoder
and FFB device, other section formats/editions, full multi-peer synchronization,
and native Windows or GitHub Actions execution not run locally. See
[DECOMP_MAP.md](docs/DECOMP_MAP.md), [COMPARISON.md](docs/COMPARISON.md), and
[DEVIATIONS.md](docs/DEVIATIONS.md) for the evidence and current limits.

The [public reconstruction survey](docs/UPSTREAM_SURVEY.md) records related
projects and the searches used to avoid duplicating existing work. The public
decompilation and portable port found so far target Colin McRae Rally 2.0, not
this 1998 game; they are methodology references, not compatibility evidence.

The optional PlayStation reference-edition workflow is documented in
[PSX_REFERENCE_PLAN.md](docs/PSX_REFERENCE_PLAN.md). It uses PS1-specific tools
to narrow Windows questions while keeping artifact provenance and compatibility
claims separate; it is not a second supported target.

Known artifacts from the initial local installation:

| Role | Size | SHA-256 |
| --- | ---: | --- |
| `Rally.exe` | 1,114,190 | `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb` |
| `Game.exe` | 502,396 | `b445fd0dc6243b652a894c299a336296f48e35f7034775d4d60edd30fcbdee78` |
| `qmixer.dll` | 136,192 | `262c3247b130c9de8410f11f39898230b1776bff5acbd820125d1e06ae260b26` |

These hashes identify observations; they do not yet establish edition, region,
or patch ancestry by themselves. Static patch analysis identifies the installed
game as EU-derived. The canonical clean code reference is the official EU updater
payload (`0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`);
the installed executable remains a comparison variant.

## Working locally

Keep the original installation outside this repository and expose its path only
for local commands:

```sh
export CMR_ORIGINAL_DIR=/path/to/your/CMRally
```

All generated reverse-engineering state belongs in `.work/`, and all imported
game data belongs in `runtime-data/`. Neither is versioned.

Run source-owned checks with:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
./scripts/check-portability.sh
./scripts/check-legacy-backends.sh
```

Run the exact offscreen indexed-palette GPU conformance slice with:

```sh
SDL_VIDEO_DRIVER=offscreen cargo run -q -p cmr-gpu-conformance -- \
  --backend both --warmup 2 --frames 8
```

The SDL_GPU contender currently targets Vulkan and compiles its source-owned
GLSL with `glslangValidator` (or `CMR_GLSLANG_VALIDATOR`). The run checks both
GPU readbacks against the established software presenter and against each
other. It does not choose a native 3D backend; that decision uses the
imported-stage benchmark below.

The source-owned raw-C3D cross-backend hardware check is opt-in:

```sh
SDL_VIDEO_DRIVER=offscreen cargo test -p cmr-gpu-conformance \
  both_backends_match_exactly_on_one_raw_recovered_c3d_packet \
  -- --ignored --nocapture
```

It proves that both APIs transform and rasterize the same raw C3D packet with
byte-exact readback.

The I08 decision lane is a correctness-gated steady-state benchmark over one
imported stage, read in place from the user's own media root:

```sh
SDL_VIDEO_DRIVER=offscreen cargo run --release -q -p cmr-gpu-conformance \
  --bin cmr-gpu-stage-benchmark -- \
  --source "$CMR_ORIGINAL_DIR/Game/Au/1" --stage AU1 \
  --warmup 32 --frames 240
```

It composes the established software stage frame as the oracle, prepares the
persistent GPU scene once per candidate, gates correctness, warms up, and then
ranks only the steady-state encode/submit/fence-wait interval. The gate is
cross-backend byte equality plus a documented nearest-neighbour sampling-tie
budget (0.1% of the frame) against the software oracle, because a fixed-point
hardware rasterizer cannot match the float software reference at UV texel
boundaries. `--backend wgpu|sdl-gpu|both`, `--wgpu-adapter`, and `--low-power`
select the candidates. Stage output is path-free, reports timings and
`winner=not-selected`, and needs an otherwise idle GPU for meaningful numbers.

Verify the currently cataloged source corpus, then import exact asset bytes into
the ignored runtime directory:

```sh
mkdir -p runtime-data
cargo run -q -p cmr-assets -- verify \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/installed-eu-derived-assets-v2.json
cargo run -q -p cmr-assets -- import \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/installed-eu-derived-assets-v2.json \
  --output runtime-data/installed-eu-derived-assets-v2
```

Extract the official-EU executable-backed surface coefficients into their own
ignored, profile-bound runtime-data store (no game values are bundled):

```sh
cargo run -q -p cmr-assets -- extract-surface-tables \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/official-eu-surface-tables-v1.json \
  --output runtime-data/official-eu-surface-coefficients
```

This command requires the exact executable identified by the reviewed profile,
checks its hash before parsing, and records no absolute source path. It is
separate from the installed-media import above.

Extract the fourteen authenticated compiled initializer cases required by the
native setup path into a sibling ignored store:

```sh
cargo run -q -p cmr-assets -- extract-compiled-car-initializer \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/official-eu-compiled-car-initializer-v1.json \
  --output runtime-data/official-eu-compiled-car-initializer
```

Extract the authenticated car-code-to-selector mapping used to choose the
fourteen compiled car cases into another ignored store:

```sh
cargo run -q -p cmr-assets -- extract-car-selectors \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/official-eu-car-selector-map-v1.json \
  --output runtime-data/official-eu-car-selectors
```

The windowed session expects those two executable-derived stores as siblings of
`runtime-data/installed-eu-derived-assets-v2`. With that layout in place, run:

```sh
cargo run --release -q -p cmr-app -- \
  --windowed-game-session \
  --runtime-data runtime-data/installed-eu-derived-assets-v2 \
  --source "$CMR_ORIGINAL_DIR/Game/Au/1" \
  --stage AU1 \
  --car-code Sub \
  --renderer auto        # auto | software | gpu
```

The wgpu path prepares and uploads the layered stage, HZN, OBJ, SHT, and car
scenes once, updates dynamic world and camera matrices per frame, and renders
into the palette-index target. `auto` selects that path when setup succeeds and
falls back to the software compatibility oracle on a typed setup or frame
failure. `--car-code` is selected by the caller/menu; the reconstruction does
not embed the original selector table. `software` forces the much slower oracle
compositor for differential checks, while `gpu` fails closed. The session logs
the requested and actual renderer plus any fallback category without printing
local paths.

Render one complete deterministic root-menu frame through the native
application boundary (headless for now):

```sh
cargo run -q -p cmr-app -- \
  --headless-root-frame \
  --runtime-data runtime-data/installed-eu-derived-assets-v2
```

`--runtime-data` takes precedence over the optional `CMR_RUNTIME_DATA`
fallback. The command prints only the reviewed profile ID, render counters,
frame extent, and deterministic indexed-frame checksums; it never prints or
embeds the selected local path. The SDL3 host now provides window, input, audio,
GPU presentation, and fixed-step session pacing; gameplay compatibility remains
partial as tracked below.

Run the opt-in reviewed-import root-session smoke with:

```sh
CMR_RUNTIME_DATA="$PWD/runtime-data/installed-eu-derived-assets-v2" \
  cargo test --locked -p cmr-video --test reviewed_import_session_smoke \
  -- --ignored --nocapture
```

The profile accepts only the exact locally observed `Game/**`, `New_Fe/**`, and
`ini/**` corpus. Other legitimate releases need reviewed metadata profiles;
the importer intentionally has no `--ignore-hash` escape hatch.

The execution plan and evidence policy live in [docs/ROADMAP.md](docs/ROADMAP.md)
and [docs/EVIDENCE.md](docs/EVIDENCE.md). The reproducible REA workflow is in
[docs/ANALYSIS.md](docs/ANALYSIS.md), and the asset boundary is described in
[docs/ASSET_PIPELINE.md](docs/ASSET_PIPELINE.md). The live checklist and compact
progress footer are in [docs/DECOMP_MAP.md](docs/DECOMP_MAP.md), while recovered
subsystem boundaries are in [docs/SUBSYSTEMS.md](docs/SUBSYSTEMS.md). The
provisional Windows/Linux backend recommendation and its benchmark gates are in
[docs/MODERN_BACKEND_RESEARCH.md](docs/MODERN_BACKEND_RESEARCH.md). How the
reconstruction is checked against the original is in
[docs/COMPARISON.md](docs/COMPARISON.md), and the supported editions and
intentional differences are in [docs/DEVIATIONS.md](docs/DEVIATIONS.md).

From the repository root, watch the compact progress footer with:

```sh
watch -n 2 'tail -n 10 docs/DECOMP_MAP.md'
```

## Legal note

“Abandonware” is not a legal license. This repository will contain independently
written source, documentation, and synthetic tests only. Users remain
responsible for obtaining and using the original game data lawfully.

