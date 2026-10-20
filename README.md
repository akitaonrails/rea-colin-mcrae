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
used as the first executable backend oracle. It supports the recovered
exact-size startup and index-zero keyed copies while rejecting unrecovered
stretch and other color-key behavior.

The first frontend foundation also includes bounded legacy configuration and
localization models plus deterministic keyboard/joystick input rules. These
now extend through the recovered root-screen state machine, exact menu visual
and animation transcripts, verified root-bitmap loading, timing plans, and
bounded FON/text layout. These are platform-neutral contracts backed by
synthetic tests; registry, host clock/input, native window, pixel-composition,
audio, and gameplay integrations are not yet complete.

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

The profile accepts only the exact locally observed `Game/**`, `New_Fe/**`, and
`ini/**` corpus. Other legitimate releases need reviewed metadata profiles;
the importer intentionally has no `--ignore-hash` escape hatch.

The execution plan and evidence policy live in [docs/ROADMAP.md](docs/ROADMAP.md)
and [docs/EVIDENCE.md](docs/EVIDENCE.md). The reproducible REA workflow is in
[docs/ANALYSIS.md](docs/ANALYSIS.md), and the asset boundary is described in
[docs/ASSET_PIPELINE.md](docs/ASSET_PIPELINE.md). The current recovery map is in
[docs/SUBSYSTEMS.md](docs/SUBSYSTEMS.md).

## Legal note

“Abandonware” is not a legal license. This repository will contain independently
written source, documentation, and synthetic tests only. Users remain
responsible for obtaining and using the original game data lawfully.

