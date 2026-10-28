# Public reconstruction survey

Last checked: 2026-10-27

This survey prevents the project from independently rebuilding work that is
already public. It is a bounded search result, not proof that no private,
unindexed, renamed, or later-published project exists.

## Search scope

The check covered GitHub repository and code search plus general web search for
the game title and identifying strings from the Windows and PlayStation
releases. Queries included:

- `"Colin McRae Rally" decompilation`
- `"Colin McRae Rally" reimplementation`
- `CMRally Rally.exe`
- `SCUS_94474`
- `SLES-00477`
- `"Colin McRae Rally" 1998 source`

The results for the 1998 game were release databases, emulator metadata,
cheats, disc catalogs, and general game repositories. No public GitHub source
reconstruction, matching decompilation, or portable reimplementation of the
original 1998 Colin McRae Rally was found.

## Related public projects

| Project | Target | Relevance | Boundary |
| --- | --- | --- | --- |
| [CMR2Decomp](https://github.com/pablocpas/CMR2Decomp) | Colin McRae Rally 2.0 Windows | Matching-decompilation workflow, function accounting, `reccmp`, binary-layout checks, and differential tests | A sequel with different binaries and behavior; GPL-3.0 |
| [OpenCMR2](https://github.com/pablocpas/OpenCMR2) | Portable Colin McRae Rally 2.0 | Narrow platform interfaces, SDL 3/SDL_GPU backends, external user-owned data, recorded-input parity tests, and numeric-precision audits | A sequel, not evidence for CMR1 behavior; GPL-3.0 |
| [SilentPatchCMR2](https://github.com/CookiePLMonster/SilentPatchCMR2) | Compatibility patch for Colin McRae Rally 2.0 | Useful catalog of later-game compatibility problems | Patch code for the sequel, not a CMR1 reconstruction |

The reviewed snapshots are stored only in ignored `.work/github-survey/`:

- CMR2Decomp: `2122986f2f6baffccea9edebf6815c2a5c82d85a`
- OpenCMR2: `c4f3452922656e0063dabbbddcd59e8c160fc7d1`

Pinning the observed revisions keeps future comparisons reproducible without
vendoring either project.

## What is safe to reuse

The useful overlap is methodological:

- track recovered functions and contracts against explicit evidence;
- use byte/layout comparison tooling for the Windows compatibility lane;
- replay recorded inputs and compare state fields at every fixed update;
- keep graphics, audio, input, movie, and network implementations behind
  narrow platform contracts;
- audit x87 precision, conversion rounding, and compiler assumptions before
  claiming numeric parity;
- require portable backends to pass the same contract tests as compatibility
  implementations.

These practices reinforce this repository's existing evidence ledger,
decompilation map, pure parsers, contract tests, and backend separation. They
do not justify importing sequel code or changing recovered CMR1 behavior.

## Why similarly named files are not interchangeable

CMR2 uses several familiar extensions, including C3D and CAT, but shared names
do not establish a shared format. One concrete comparison already disproves a
drop-in CAT parser: CMR2Decomp describes a 0x2c-byte route-node record, while
the independently recovered CMR1 CAT contract uses a 0x1c-byte record. Fields
and semantics must therefore continue to be established from CMR1 artifacts.

Treat every CMR2 constant, structure, algorithm, and state transition as a
hypothesis generator only. A claim enters this reconstruction only when it has
independent CMR1 evidence and focused tests.

## Licensing boundary

CMR2Decomp and OpenCMR2 are GPL-3.0 projects. This repository currently uses
them only as research references. Do not copy or translate their code unless
the project first makes an explicit licensing decision and records provenance
and attribution. The default is clean, evidence-backed implementation from the
target CMR1 artifacts and source-owned synthetic fixtures.

## Recheck procedure

Run this survey again before a major new subsystem or release, and record newly
found projects here:

```sh
gh search repos '"Colin McRae Rally"' --limit 100
gh search repos '"Colin McRae Rally" decompilation' --limit 100
gh search repos '"Colin McRae Rally" reimplementation' --limit 100
gh search code 'SCUS_94474' --limit 100
gh search code 'SLES-00477' --limit 100
gh search code '"Rally.exe" "Colin McRae"' --limit 100
```

Search results are time-dependent. Update the check date and pin the exact
revision of any project used as a research reference.
