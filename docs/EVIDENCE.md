# Evidence ledger

Every compatibility claim should be reviewable without trusting an analyst's
memory. Store compact, redistributable summaries in Git; keep full REA output,
snapshots, and target-derived material under ignored `.work/`.

## Record shape

Each investigation record should contain:

- stable record ID and date;
- question being answered;
- artifact path role, byte size, and SHA-256;
- tool and provider versions plus exact command or REA operation;
- observations, with Evidence IDs when REA returns them;
- inferences and their confidence;
- coverage limits and unknowns;
- next smallest query that could resolve each important unknown;
- affected reconstruction obligations.

## Vocabulary

- **Observed:** directly parsed from the identified artifact or captured in a
  controlled run.
- **Derived:** deterministically computed from observations, such as a hash or
  decoded header field.
- **Inferred:** an interpretation supported by observations but not directly
  established by them.
- **Unknown:** not established within the stated search or runtime boundary.

Decompiler pseudocode is an observation produced by a particular analysis
profile. Names, source structure, types, intent, and runtime reachability can
still be inferred or unknown.

## Initial ledger

| ID | Status | Finding | Boundary / next action |
| --- | --- | --- | --- |
| `prov.rally.sha256` | derived | `Rally.exe` is 1,114,190 bytes with SHA-256 `46500c…c0bb` | Local installation only; identify edition and patch ancestry. |
| `prov.game.sha256` | derived | `Game.exe` is 502,396 bytes with SHA-256 `b445fd…e78` | Role not yet established. |
| `prov.qmixer.sha256` | derived | `qmixer.dll` is 136,192 bytes with SHA-256 `262c32…0b26` | Version/export ABI still to inventory. |
| `pe.rally.container` | observed | `Rally.exe` is a PE32 i386 Windows GUI image with four sections and entry RVA `0xD1800`. | Parsed statically; no runtime behavior established. |
| `pe.rally.legacy-boundaries` | observed | Imports include DirectDraw, DirectInput, DirectPlay, WinMM, QMixer, Win32/GDI, registry, and COM APIs. | Exact call sites and subsystem contracts remain unknown. |
| `rea.rally.overview` | derived | REA/Ghidra reports 2,169 procedures, 2,310 defined strings, and six loaded memory blocks for the exact `Rally.exe` hash. Evidence `ev_8c5658c5f650f52a3a597c47f66db6fccea0a0fd95892d43f10665c1c3618fd6`. | Defined-string coverage is provider-bounded; analysis does not establish runtime reachability. |
| `rea.rally.inventory` | observed | The complete recovered inventories contain 2,003 internal / 166 external procedures. PE imports include Kernel32 88, User32 35, QMixer 16, WinMM 9, GDI32 7, Advapi32 5, OLE32 3, DirectDraw 2, DirectPlay 2, and DirectInput 1. Procedure Evidence `ev_5ff7f4300bb0c7a48f7a30b6b9815021fc351d8d58d538db85999c1e404cbfac`; string Evidence `ev_36ebddd47f0bb05628c5361b203705338fc41b38ee0214e60d45c361909e07ac`. | Import presence does not prove runtime use or complete subsystem behavior. REA's recovered external-procedure count is not an import-table count. |
| `rea.rally.pdb-reference` | observed | The PE references `Game.pdb` through an NB10 CodeView record (signature `393fa60a`, age `0x3`); REA did not load a matching PDB. Same Evidence as `rea.rally.overview`. | Original symbols/types remain unavailable; the filename alone does not establish executable role. |
| `rea.rally.source-paths` | observed | Defined strings include original-looking paths naming `Loading.cpp`, `welcome.cpp`, `cm_image.cpp`, `Cm_surf.cpp`, and `New_fe/pc/Startup.cpp`, alongside asset/config keys and paths. Same string-inventory Evidence as `rea.rally.inventory`. | Filenames help cluster domains but do not recover source or prove ownership of nearby functions. |
| `rea.game.overview` | derived | REA/Ghidra reports 384 procedures, 485 defined strings, and six loaded memory blocks for the exact `Game.exe` hash. Evidence `ev_6fc13dabc016f52fe3888874fc722b599ebdccf016b72a88dd651f46dbb61166`. | Its role and relationship to `Rally.exe` remain under investigation. |
| `rea.game.inventory` | observed | The complete recovered inventories contain 292 internal / 92 external procedures; imports include registry reads, `CreateProcessA`, Win32 window/message-loop APIs, and COM initialization. Procedure Evidence `ev_7e4c847e832a829c1d6eaf0ac773b29cbb4229a8a7d491009edacb384b967a8f`; string Evidence `ev_fbfd61a764edb306f996e1024db4c45c433fb04ae9ee60e83b9b3cd1af094e1f`. | Static inventory only. |
| `rea.game.rally-string` | observed | `Game.exe` contains a defined string `\\rally.exe` at `0x41503c`, plus product-name strings. Evidence `ev_3d469b8703cd842a1e9dfda0fe5215d7fb1d66f41d2d16e55862e4ab7832ba32`. | The string has one recovered static reference at `0x401299` (Evidence `ev_0a18418ecb8666fccd929433c286fe146e0f420268ee65edfe599ed5b9e5ba13`). |
| `rea.game.launcher-role` | observed | The containing function at `0x401179` is named `KillVideoAndStartGame` by Ghidra. Its recovered body releases DirectShow-style video interfaces, builds the configured `HDPath` plus `\\rally.exe`, calls imported `CreateProcessA`, reports failure, and posts quit. It is called from recovered `WndMainProc`. Evidence `ev_d849ac1daaefc5bf766053d12465ae66abc4f5f0041ac8a78d9ed2ff3f273b05`. | Strong static evidence that `Game.exe` is a video/launcher frontend for `Rally.exe`; successful runtime launch and registry semantics remain unobserved. Callee coverage is partial due to seven unresolved call targets. |
| `assets.initial-count` | derived | Initial install has 5,174 files and 349,094,537 stored bytes overall. Runtime candidate roots contain 4,600 files / 267,320,474 bytes in `Game` and 543 files / 72,783,230 bytes in `New_Fe`. | The tree includes patches, installer residue, and mutable save state; whole-tree identity does not establish a retail edition. |
| `assets.gzip-cmpr` | observed | 3,754 files are gzip streams; every successful decode begins `CMPR` + LE `u32` length, and each declared length matches the bytes after the 8-byte wrapper. | `cmr-formats` has bounded synthetic positive, CRC, length, header, trailing-member, stored-size, and decoded-size cases. Its validator directly checked all 5,143 files under `Game` and `New_Fe`: 1,389 raw plus 3,754 valid gzip-`CMPR`, 340,103,704 stored bytes and 263,662,772 decoded payload bytes. This validates storage envelopes, not inner formats or runtime loading. |
| `assets.mixed-storage` | observed | Extensions do not determine storage: BMP, PAL, LOA, LOL, FON, REP, SSR, CJP, RSP, and JBF examples include both raw and gzip-wrapped data. | Dispatch must use magic/invariants rather than suffix. |
| `assets.path-semantics` | observed | No symlinks or case-fold collisions were found, but stored paths use mixed case while executable strings use backslashes and often uppercase names. | Portable lookup needs separator normalization plus fail-closed ASCII case-fold indexing. Runtime lookup behavior remains to verify. |
| `assets.duplicates` | derived | 385 exact-duplicate groups include 1,015 redundant files and 7,746,838 redundant bytes. | Preserve path identity; do not deduplicate imports. |
| `assets.save-state` | observed | `save/cmRally.cfg` is a 7,712-byte mutable/opaque file containing profile-related text. | Exclude from immutable import; recover and test persistence separately. |
| `assets.profile.v1` | derived | Metadata-only profile `installed-eu-derived-assets-v1` identifies all 5,143 files in `Game` and `New_Fe`; profile SHA-256 is `2b06a119e61d9ab32cc47ecd147f0c29ba961f6888de59138ad625ccde4011c8`. | Applies only to the observed installed corpus, not every EU release. |
| `assets.import.smoke` | observed | Production CLI verification passed 5,143 files / 340,103,704 bytes. A full staged import copied the same counts and bytes, rehashed destination files, wrote a matching `import-state.json`, and atomically promoted output. | Local filesystem observation; smoke output was removed afterward. Unit tests cover error/cleanup boundaries with synthetic trees. |
| `patch.official-eu` | observed | The EU updater ZIP contains a self-extracting member whose decoded resource is `CMPR` + length + PE. Removing the 8-byte envelope yields a 1,114,190-byte `Rally.exe`, SHA-256 `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, identified by resources as European. | Official-package provenance is stronger than the installed modified executable, but relationship to a pre-update retail disc binary remains unknown. |
| `patch.official-us` | observed | Equivalent extraction from the US updater yields a 1,114,190-byte PE, SHA-256 `048921d20151e56aea7bac37c0a4b218012b55e81be0d561b89a967107246b60`. EU and US differ in 39,476 bytes across 26,346 runs. | Track as a separate regional build, not a resource-only variation. |
| `patch.installed-eu-derived` | derived | Installed `Rally.exe` differs from the official EU payload in 87 bytes across eight runs, but differs from the official US payload in 39,563 bytes; its resources identify it as European. | Strong EU-derivation inference. The `.chemicalflood` name is not cryptographic provenance. |
| `patch.unofficial-modern` | observed | The supplied xdelta transforms each official regional payload by 51 bytes in three file ranges; neither output matches the installed executable. | It is a small crack-style binary transform, not a modern API/library compatibility layer. Exact changed-code intent remains unknown. |
| `pe.qmixer` | observed | `qmixer.dll` identifies QSound Labs product/version 3.00 and exports 62 contiguous named `QSWaveMix*` APIs. It uses WinMM waveOut/MMIO and dynamically loads DirectSound. Resource Evidence `ev_384f2eb512822c548c9704880682a238ead5ee1fd82d9779459a5fd7795e0607`. | `Rally.exe` imports 16 of the exports. Runtime mixer semantics remain unknown. |
| `pe.rally.resources` | observed | Six icon images, two icon groups, and one version resource use language ID 2057 (`en-GB`). Evidence `ev_8718520f7aef2ac0338b0cdff2c92a391d8f6cf5f149be054909fcc0336a2206`. | Complete PE resource-tree coverage, not gameplay behavior. |
| `pe.game.resources` | observed | Three icon images and one icon group use language ID 2057; no version resource was present. Evidence `ev_0ce718bff05eea068ad4f6edd16faae037dbe03abf2af3027ff9724b2060b889`. | Complete PE resource-tree coverage. |
| `rea.provider` | observed | Installed REA 6.5.0 reports Ghidra 12.1.2 available on Linux/x64 with Java 27. | Source checkout 6.7.0 is unbuilt; evidence must retain the 6.5.0 profile identity. |

Hash abbreviations in prose are display-only. Machine-readable manifests must
always retain complete hashes.

