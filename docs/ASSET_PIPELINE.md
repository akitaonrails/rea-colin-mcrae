# User-owned asset pipeline

The project must build and test without proprietary game data. Assets are an
external runtime input prepared from a user's lawful installation.

## Planned flow

1. The user supplies `--source /path/to/CMRally` (or `CMR_ORIGINAL_DIR`).
2. The importer scans relative paths without following links outside the root.
3. It hashes recognized executables and key data files to select a source
   profile. Unknown or mixed profiles fail closed with actionable diagnostics.
4. It validates required files, sizes, magic values, and format-specific
   invariants before writing output.
5. It copies or converts data into a new `runtime-data/<profile>/` staging
   directory.
6. It writes a manifest containing source-profile ID, importer version,
   relative-path mappings, sizes, hashes, conversions, and warnings.
7. It atomically promotes a complete staging directory. Partial output is
   removable and never treated as ready.

The default import mode will copy data so the original installation remains
untouched. A later opt-in link mode may be added only with explicit symlink and
case-sensitivity rules.

The application consumes only the completed import, never the original-media
path. The first executable slice accepts `--runtime-data PATH` or the
`CMR_RUNTIME_DATA` fallback and opens that directory against the embedded
reviewed-profile metadata. It performs no current-directory probing or
automatic import. Its headless root-frame command is therefore portable to any
user-selected import location while keeping proprietary payloads outside the
binary and repository.

For the currently observed installation, the default immutable roots are
`Game/**`, `New_Fe/**`, and `ini/**`. The importer must exclude executables, DLLs, patch
archives, registry files, uninstall state, instructions, and `save/**`.

Canonical-EU static evidence establishes that startup loads palettes and bitmaps
from `ini/**`; treating it as installer-only would produce an incomplete runtime
import.

The importer must not recursively unpack the `patches-applied/` directory.

## Source profiles

A profile is more than an executable hash. It should declare:

- region/language and known patch lineage;
- accepted hashes for `Rally.exe`, `Game.exe`, and required libraries;
- required, optional, and forbidden/mixed-version paths;
- case-canonical relative paths;
- format probes for containers that lack stable per-file hashes;
- transformations needed by the reconstructed engine;
- provenance notes and known limitations.

Profiles belong in source control because they contain metadata, not payloads.
The initial local hashes are candidates only until patch and regional
relationships are established.

The observed source is a patched installed distribution, not a proven pristine
retail image. Whole-tree and subtree digests identify that corpus only; they do
not establish edition provenance.

## Observed data conventions

The initial read-only scan found:

- 4,600 files (267,320,474 bytes) under `Game/`;
- 543 files (72,783,230 bytes) under `New_Fe/`;
- 3,754 gzip streams with the deterministic prefix
  `1f 8b 08 00 00 00 00 00 00 0b`;
- every decoded gzip stream begins with `CMPR`, followed by a little-endian
  `u32` payload length and then that many payload bytes;
- raw and wrapped files share extensions, so suffixes cannot select decoders;
- no symlinks and no ASCII case-fold collisions in this corpus, but pervasive
  mixed case and Windows backslashes in executable strings;
- 385 exact-duplicate groups. Every pathname remains semantically distinct and
  must be preserved instead of deduplicated.

Compatibility mode preserves the exact stored bytes. The runtime asset reader
sniffs gzip, performs a bounded inflate with CRC validation, requires the
`CMPR` envelope, validates the declared length, and exposes the inner payload.
Non-gzip input passes through unchanged. A decoded cache may be added later but
cannot become the canonical imported representation.

The initial implementation is `cmr-formats::stored_asset`. Its source-owned
tests cover successful raw/wrapped reads and malformed boundaries. A local
corpus check can be reproduced without writing output:

```sh
cargo run -q -p cmr-formats --example validate_tree -- \
  "$CMR_ORIGINAL_DIR/Game" "$CMR_ORIGINAL_DIR/New_Fe"
```

Runtime lookup will normalize backslashes to slashes, reject traversal, and
resolve each component through a precomputed ASCII case-folded index. It will
fail on ambiguity rather than pick a platform-dependent path.

`cmr-assets::AssetIndex` implements the first profile-backed lookup contract.
Its unit tests cover Windows separators/casing, missing entries, drive/absolute
paths, traversal, repeated separators, NULs, and ambiguous components. This is
a portability design backed by observed path mismatches; exact legacy Win32
path failure behavior remains a separate recovery question.

`cmr-assets::AssetStore` connects that lookup contract to an imported runtime
tree. Import and store APIs accept a `ReviewedProfile` parsed and hashed from
the exact distributed JSON bytes, so a digest cannot be paired with substituted
metadata. Opening a store requires matching `import-state.json` identity and
digest. Every load rechecks the root and path components for links, verifies
the stored size and SHA-256 against reviewed metadata, enforces caller-supplied
stored and decoded byte limits, confirms the declared storage encoding, and
returns raw or decoded `CMPR` payload bytes. Synthetic tests cover state
mismatches, Windows path lookup, raw/wrapped loads, post-import tampering,
encoding disagreement, size bounds, substituted profiles, and replaced
root/component symlinks.

## Manifest sketch

```json
{
  "schema_version": 1,
  "profile": "unknown-local-candidate",
  "tree_digest": {
    "algorithm": "cmr-tree-sha256-v1",
    "value": "<full lowercase digest>"
  },
  "source_identity": {
    "rally_exe_sha256": "46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb"
  },
  "files": [
    {
      "source_path": "Game/example.ext",
      "runtime_path": "Game/example.ext",
      "size": 0,
      "sha256": "<full lowercase digest>",
      "operation": "copy"
    }
  ],
  "warnings": []
}
```

The example path is synthetic and does not claim an observed file format.

`cmr-tree-sha256-v1` sorts by relative UTF-8 path bytes and hashes concatenated
binary records:

```text
u32le(path_byte_length) || path_bytes || u64le(file_size) || sha256(file_bytes)
```

The checked-in edition manifest should also record roots, excluded top-level
paths, sentinels, original stored hashes/sizes, storage encoding (`raw` or
`gzip-cmpr`), optional decoded payload hashes/sizes, and preserved empty
directories. It must never record the user's absolute source path.

The current reviewed manifest is
`profiles/installed-eu-derived-assets-v2.json` (SHA-256
`ae9db22479a0360b3e80a089afb02460b53b0b414c87cecba852b6dbcc3c25d7`).
It contains metadata for 5,162 files across all three required roots and no
payload bytes or absolute paths. V2 supersedes the incomplete V1 profile after
static evidence established startup reads from `ini/**`.

## Executable-derived surface coefficients

The surface coefficients consumed by the recovered vehicle solver are not an
external game file. In the official-EU executable they occupy two adjacent
120-word signed little-endian families. Extract them from a user-owned,
positively identified executable into a separate ignored runtime-data store:

```sh
cargo run -q -p cmr-assets -- extract-surface-tables \
  --source "$CMR_ORIGINAL_DIR" \
  --profile profiles/official-eu-surface-tables-v1.json \
  --output runtime-data/official-eu-surface-tables
```

All three paths are explicit. The reviewed profile contains metadata only: a
relative `Rally.exe` path, exact size and SHA-256, the reviewed virtual address,
and the two-by-120 shape. The extractor rejects linked path components, bounds
the read, verifies the hash before PE parsing, validates the complete PE32
virtual/raw mapping, and atomically promotes a fresh sibling stage. It writes:

```text
runtime-data/official-eu-surface-tables/
  surface-coefficients-q16le.bin
  surface-import-state.json
```

The binary is exactly 960 bytes of user-derived game data and must stay under
ignored `runtime-data/`. The JSON contains only relative metadata, sizes, and
hashes; it never records the selected absolute source or output path. This is a
project-owned extraction format, not an original game format, and remains
separate from the media `import-state.json`. Tests construct synthetic PE
images only; no coefficient values are committed.

`cmr_assets::SurfaceTableStore::open` is the corresponding runtime reader. On
a stable, caller-owned directory it fails closed unless the root is a real
directory; the bounded state document has the supported schema, reviewed
profile ID and exact profile digest, and matching source path/size/hash; and
the declared output path, size, and SHA-256 all match the opened regular
960-byte data file. Persistent symlinks in the root and the fixed, flat state
and data child paths are rejected; nested paths are not accepted or supported.
The file is re-hashed and decoded into
two owned `[i32; 120]` families during `open`, so the store retains no root or
data path and later ordinary filesystem changes cannot alter an opened value.
This is not a claim of race-free path resolution under concurrent adversarial
filesystem mutation; callers must keep the directory stable while `open`
runs.

## Schema versioning and migration

Four regenerable on-disk artifacts carry a `schema_version`. The completed
media-store readers accept only the exact generations selected by
`cmr-assets::PROFILE_SCHEMA_VERSION` and
`cmr-assets::IMPORT_STATE_SCHEMA_VERSION`. The executable-derived surface path
validates its reviewed profile with `SURFACE_PROFILE_SCHEMA_VERSION`, emits a
state document with `SURFACE_IMPORT_SCHEMA_VERSION`, and reads it back through
the fail-closed `SurfaceTableStore` described above:

- the reviewed source profile (`profiles/*.json`);
- the import state (`<runtime-data>/import-state.json`);
- the reviewed surface-extraction profile;
- the surface extraction state (`surface-import-state.json`).

Imported trees are derived data. For the media store, an incompatible version
is never migrated in place: the profile validator and store fail closed
(`ProfileError::UnsupportedSchema` /
`AssetStoreError::InvalidImportState { field: "schema_version" }`) and the
caller regenerates the tree from the user's own media with the matching
importer. A profile and its state from different generations cannot be paired.
The media store additionally checks profile identity, digest, and file/byte
totals. The surface profile validator rejects unsupported profile generations,
while the extractor records its state schema, exact profile digest, and output
hash in a path-free document. `SurfaceTableStore` rejects unsupported state
generations and verifies the profile ID/digest, all recorded source metadata,
and the declared output path/size/hash before consuming the local data.

A version bump is required for any semantic or required-field change. Purely
additive optional fields that readers can ignore leave the version alone. A
bump must update the constants, every writer, the worked example above, and add
a profile fixture plus the rejection tests at the new generation. There is no
implicit downgrade or forward-compatibility path.

## Safety and verification

- Reject absolute, parent-traversal, NUL-containing, and ambiguous case-folded
  manifest paths.
- Reject Windows-invalid and device-alias components, including drive-relative
  prefixes, trailing dots/spaces, and `CON`/`NUL`/`COM1`-style names.
- Bound input size, decoded size, entry count, nesting, and compression ratio.
- Never overwrite the source installation.
- Copy rather than hardlink so runtime writes cannot mutate the source.
- Refuse to publish or package `runtime-data/`.
- Test with synthetic directory trees and format fixtures only.
- Verify a completed local import by re-hashing outputs and checking that every
  required logical asset has exactly one owner.
- Write to a fresh sibling staging directory, verify it, then atomically rename
  it into place. A failed staging directory is never considered ready.
- Refuse an output whose parent is inside the source installation, and refuse
  pre-existing output/staging paths instead of overwriting them.

Mutable `save/cmRally.cfg` is runtime state, not an immutable asset. Persistence
recovery will model it separately and test it using synthetic fixtures.

