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

For the currently observed installation, the default immutable roots are
`Game/**` and `New_Fe/**`. The importer must exclude executables, DLLs, patch
archives, registry files, uninstall state, instructions, and `save/**`.
Installer-only `ini/**` data may be supported later as an explicit preservation
option, but it is not part of the runtime corpus until code references establish
otherwise.

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
will sniff gzip, perform a bounded inflate with CRC validation, require the
`CMPR` envelope, validate the declared length, and expose the inner payload.
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

The first reviewed manifest is
`profiles/installed-eu-derived-assets-v1.json` (SHA-256
`2b06a119e61d9ab32cc47ecd147f0c29ba961f6888de59138ad625ccde4011c8`).
It contains metadata for 5,143 files and no payload bytes or absolute paths.

## Safety and verification

- Reject absolute, parent-traversal, NUL-containing, and ambiguous case-folded
  manifest paths.
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

