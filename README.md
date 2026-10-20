# Colin McRae Rally reconstruction

This project aims to reconstruct a buildable, portable implementation of the
original Windows game from static analysis. The first target is behavioral and
data compatibility with a precisely identified shipped build. Platform
modernization comes afterward, behind compatibility interfaces.

The repository intentionally contains no original game assets or binaries. A
user will point the tooling at their own installation; the importer will verify
the source version and prepare a local, ignored `runtime-data/` tree.

## Current status

Milestone 0 — provenance and reconstruction plan. No game code has yet been
claimed compatible.

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

The execution plan and evidence policy live in [docs/ROADMAP.md](docs/ROADMAP.md)
and [docs/EVIDENCE.md](docs/EVIDENCE.md). The reproducible REA workflow is in
[docs/ANALYSIS.md](docs/ANALYSIS.md), and the asset boundary is described in
[docs/ASSET_PIPELINE.md](docs/ASSET_PIPELINE.md). The current recovery map is in
[docs/SUBSYSTEMS.md](docs/SUBSYSTEMS.md).

## Legal note

“Abandonware” is not a legal license. This repository will contain independently
written source, documentation, and synthetic tests only. Users remain
responsible for obtaining and using the original game data lawfully.

