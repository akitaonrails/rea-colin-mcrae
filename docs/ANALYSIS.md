# Reproducing static analysis

The original installation remains outside this repository. Set its path and
create an ignored local evidence directory:

```sh
export CMR_ORIGINAL_DIR=/path/to/CMRally
mkdir -p .work/rea
```

The initial baseline used:

- REA CLI 6.5.0;
- local REA source checkout commit `799e8265b254e225dc1dabdd25e56f0ab7f86bd3`
  (version 6.7.0, not built during this baseline);
- Ghidra 12.1.2 in ephemeral, immutable-source import mode;
- OpenJDK 27 on Linux x86-64;
- REA upstream native analysis profile digest
  `5065c5dccf3ac786bcfd1c4ec698186fe19f7f79900f3b73a63052458af95250`;
- composed workflow profile digest
  `f15270094b327281d5c93435e5e5b324e248086614e048f1810b471465864cf5`.

Check the installed provider without changing configuration:

```sh
rea --version
rea doctor --provider ghidra --json
```

Create local reusable snapshots and compact overview Evidence:

```sh
rea inspect "$CMR_ORIGINAL_DIR/Rally.exe" \
  --provider ghidra \
  --snapshot "$PWD/.work/rea/rally.snapshot.json" \
  --json

rea inspect "$CMR_ORIGINAL_DIR/Game.exe" \
  --provider ghidra \
  --snapshot "$PWD/.work/rea/game.snapshot.json" \
  --json
```

Snapshots can contain target-derived analysis and must remain ignored. Commit
only concise conclusions, complete Evidence IDs, exact artifact identities, and
coverage limitations to [EVIDENCE.md](EVIDENCE.md).

## Investigation order

Avoid bulk decompilation. It produces an unauditable body of pseudocode and
wastes analysis context. Work from stable boundaries:

1. locate entry point and executable handoff;
2. group imported APIs by subsystem and follow their direct references;
3. search exact configuration keys, filenames, extensions, and error text;
4. select the containing procedures and record callers/callees;
5. analyze one function dossier only when it answers a named question;
6. translate the recovered contract into a clean module plus tests;
7. link the implementation and verifier back to the Evidence record.

Every query should make one important unknown smaller. Negative searches are
bounded to the provider's recovered inventory and must not be described as
proof that bytes or runtime behavior are absent.

## Checking startup artwork locally

The repository does not contain original artwork. After supplying an original
installation, parse the six startup bitmaps and six palettes without copying
them into the worktree:

```sh
cargo run --locked -p cmr-formats --example inspect_startup_art -- \
  "$CMR_ORIGINAL_DIR/ini"
```

This reports dimensions, row orientation, palette/chunk structure, and the
RGB-entry agreement between each startup bitmap and its separately selected
palette. It is an inspection aid, not a substitute for profile verification or
a claim that the files were exercised by the original executable.

After running the verified importer, exercise the full runtime-store and
startup decode path while comparing identity and nearest-color index modes:

```sh
cargo run --locked -p cmr-core --example inspect_startup_surface -- \
  runtime-data/installed-eu-derived-assets-v2 \
  profiles/installed-eu-derived-assets-v2.json legal english
```

The command reports counts only; it does not copy or export artwork.

## Checking C3D record framing locally

Inspect every user-owned C3D under one or more source roots without exporting
labels or blob bytes:

```sh
cargo run --locked -q -p cmr-assets -- verify \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/installed-eu-derived-assets-v2.json
cargo run --locked -p cmr-formats --example inspect_c3d_tree -- \
  "$CMR_ORIGINAL_DIR/Game" "$CMR_ORIGINAL_DIR/New_Fe"
```

The first command establishes exact reviewed-corpus identity. The scanner then
decodes supported raw or gzip/`CMPR` storage and validates the bounded C3D
header, all three recovered record iterators, group 0's counted `u16` values,
and group 1's opaque 32-byte and 24-byte entry arrays. It also exercises the
checked numeric joins from group 0 to group 1 and from group-1 24-byte entries
to group 2, counting the observed `-1` sentinel separately from invalid
references. By itself the scanner is an inspection aid, not provenance
verification. It reports aggregate counts, bytes, reference outcomes,
exact-layout counts, and section-boundary diagnostics only. Exact layout,
reference validity, and section exhaustion are corpus evidence, not universal
format requirements.

The unbuilt source checkout can be used later after an explicitly authorized
dependency install/build. Until then, commands in this document describe the
installed 6.5.0 CLI and its recorded analysis profile.

