# Colin McRae Rally reconstruction

This project aims to reconstruct a buildable, portable implementation of the
original Windows game from static analysis. The first target is behavioral and
data compatibility with a precisely identified shipped build. Platform
modernization comes afterward, behind compatibility interfaces.

The repository intentionally contains no original game assets or binaries. A
user will point the tooling at their own installation; the importer will verify
the source version and prepare a local, ignored `runtime-data/` tree.

## Current status

Milestone 0 (provenance and reconstruction plan) is complete. Milestone 1 has
started with bounded parsers for the observed asset storage and indexed startup
art, strict asset import/runtime loading, and recovered startup-art selection
and surface contracts. A backend-neutral indexed display contract now preserves
the recovered device, surface, shared-palette, and front/back startup-copy
ordering. A pure window-loop reducer also captures activation pause/restore,
message dispatch, and steady-frame presentation decisions for future Win32 and
SDL backends. No gameplay subsystem is yet claimed compatible.

`cmr-video` currently supplies a deterministic in-memory indexed framebuffer
used as the first executable backend oracle. It supports startup, persistent
font/root resources, recovered aligned BMP surfaces, bounded root C3D drawing,
palette commit, and complete headless root-frame capture. Native window, input,
clock, and audio hosts remain separate work.

The first frontend foundation also includes bounded legacy configuration and
localization models plus deterministic keyboard/joystick input rules. These
now extend through the recovered root-screen state machine, exact menu visual
and animation transcripts, verified root-bitmap loading, timing plans, and
bounded FON/text layout plus persistent keyed-glyph resources. A production
software session now assembles the verified user import and renders repeated
complete root frames; an opt-in reviewed-corpus smoke test covers two frames
and teardown. The first post-root championship setup state now also preserves
car/gear filtering, navigation, preview identities, and the original commit-on-
Escape behavior as a tested pure model. Its production asset boundary now
admits all 14 car objects and palettes plus 11 deduplicated manufacturer logos
from the caller-selected verified import; an opt-in reviewed-corpus test covers
the complete set. Logo surfaces now have a transactional shared-store upload
path, and an exact backend-neutral setup-frame plan covers title/rows,
selection geometry, active preview ordering, and paired palette-cache behavior.
An identity-neutral checked bitmap executor now supplies the shared safe blit
boundary needed by selection, info-box, logo, and later frontend resources.
The exact R_INFO bitmap, 15 Chequered preview frames, and FCHE palette now have
a verified, transactional persistent asset boundary, while a generic prepared
text path validates complete font work before issuing frame effects. Available
setup text and 2D bitmap commands now execute in their recovered order after
whole-frame preflight; info formatting, car C3D, palette integration, and the
animation timer remain explicit downstream work.
These are platform-neutral compatibility contracts; registry, host clock/input,
native window, audio, setup-screen pixel execution, later frontend screens, and
gameplay integrations are not yet complete.

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
```

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
[docs/MODERN_BACKEND_RESEARCH.md](docs/MODERN_BACKEND_RESEARCH.md).

From the repository root, watch the compact progress footer with:

```sh
watch -n 2 'tail -n 10 docs/DECOMP_MAP.md'
```

## Legal note

“Abandonware” is not a legal license. This repository will contain independently
written source, documentation, and synthetic tests only. Users remain
responsible for obtaining and using the original game data lawfully.

