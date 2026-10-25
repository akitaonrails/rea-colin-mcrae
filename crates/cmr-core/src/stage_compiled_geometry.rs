//! Authenticated runtime ownership for one stage's compiled-lane geometry.
//!
//! The native `.hpc` load is separate from the 14-section stage bundle. This
//! boundary therefore resolves an explicit profile-relative stage base through
//! the reviewed [`AssetStore`], then parses and owns the decoded payload. It
//! does not search by basename or fall back to unverified original-media IO.

use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};
use cmr_formats::hpc::{HpcError, HpcFile, parse_hpc};
use cmr_formats::stage::stage_section_path;

use crate::compiled_car_initializer::{
    DecodedCompiledLaneGeometryProvider, decoded_compiled_lane_provider_from_hpc,
};

/// Extension derived by `FUN_00480a10` for the compiled stage geometry.
///
/// This is deliberately not part of `stage::STAGE_EXTENSIONS`: the native code
/// loads it through a distinct path after the master stage-section walk.
pub const HPC_EXTENSION: &str = ".hpc";

/// An authenticated, decoded, validated, and owned stage geometry snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageCompiledGeometry {
    hpc: HpcFile,
}

impl StageCompiledGeometry {
    /// Loads `<stage_base>.hpc` from a reviewed runtime-data store.
    ///
    /// `stage_base` is a profile-relative identity such as
    /// `Game/Au/1/AU1`, not a filesystem root or ambiguous basename.
    pub fn load(
        store: &AssetStore,
        stage_base: &str,
    ) -> Result<Self, StageCompiledGeometryLoadError> {
        let logical_path = stage_section_path(stage_base, HPC_EXTENSION);
        let decoded = store
            .load(&logical_path)
            .map_err(StageCompiledGeometryLoadError::Store)?;
        let hpc = parse_hpc(&decoded).map_err(StageCompiledGeometryLoadError::Parse)?;
        Ok(Self { hpc })
    }

    /// The retained decoded payload, including all preserved unknown fields.
    #[must_use]
    pub const fn hpc(&self) -> &HpcFile {
        &self.hpc
    }

    /// A stable-table provider borrowing this owned snapshot.
    #[must_use]
    pub fn provider(&self) -> DecodedCompiledLaneGeometryProvider<'_> {
        decoded_compiled_lane_provider_from_hpc(&self.hpc)
    }

    /// Eight providers backed by the same immutable snapshot.
    ///
    /// This is the ordinary reconstruction policy for one loaded stage. The
    /// lower-level executor still accepts distinct providers when later
    /// evidence requires per-lane rebinding or a controlled differential
    /// fixture.
    #[must_use]
    pub fn eight_lane_providers(&self) -> [DecodedCompiledLaneGeometryProvider<'_>; 8] {
        [self.provider(); 8]
    }
}

/// Failure to cross the authenticated stage-geometry load boundary.
#[derive(Debug)]
pub enum StageCompiledGeometryLoadError {
    /// Profile resolution, stored-byte verification, or storage decode failed.
    Store(AssetStoreError),
    /// Authenticated decoded bytes were not a valid `.hpc` payload.
    Parse(HpcError),
}

impl fmt::Display for StageCompiledGeometryLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "cannot load stage .hpc asset: {error}"),
            Self::Parse(error) => write!(formatter, "cannot parse stage .hpc payload: {error}"),
        }
    }
}

impl std::error::Error for StageCompiledGeometryLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::Parse(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use cmr_assets::{
        AssetFile, AssetProfile, AssetStore, AssetStoreError, ReviewedProfile, StorageEncoding,
        import_source,
    };
    use cmr_formats::hpc::HpcError;
    use cmr_formats::stored_asset::DecodeLimits;
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use sha2::{Digest, Sha256};

    use crate::compiled_car_initializer::{
        CompiledEightLaneExecutionInputs, CompiledEightLaneExecutionState,
        execute_compiled_eight_lanes,
    };

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "cmr-stage-compiled-geometry-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn sha256(bytes: &[u8]) -> String {
        use std::fmt::Write as _;
        let mut output = String::with_capacity(64);
        for byte in Sha256::digest(bytes) {
            write!(output, "{byte:02x}").unwrap();
        }
        output
    }

    fn cmpr(payload: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(b"CMPR").unwrap();
        encoder
            .write_all(&(payload.len() as u32).to_le_bytes())
            .unwrap();
        encoder.write_all(payload).unwrap();
        encoder.finish().unwrap()
    }

    fn triangle_hpc() -> Vec<u8> {
        let vertices: [[i32; 3]; 3] = [
            [0, 0x2_0000, 0],
            [0, 0x2_0000, 0x1_0000],
            [0x1_0000, 0x2_0000, 0],
        ];
        let mut payload = Vec::new();
        for word in [0_i32, 0] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        for count in [1_i16, 0, 0, 0, 0] {
            payload.extend_from_slice(&count.to_le_bytes());
        }
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&1_i16.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        payload.extend_from_slice(&0_i32.to_le_bytes());
        payload.extend_from_slice(&3_u16.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        for vertex in vertices {
            for component in vertex {
                payload.extend_from_slice(&component.to_le_bytes());
            }
        }
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&0_u16.to_le_bytes());
        for word in [0_u16, 1, 2, 0x1234] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.extend_from_slice(&1_u32.to_le_bytes());
        payload.extend_from_slice(&0_i16.to_le_bytes());
        payload.extend_from_slice(&1_i16.to_le_bytes());
        payload.extend_from_slice(&1_i16.to_le_bytes());
        payload
    }

    fn open_store(
        temp: &TestDirectory,
        stored: &[u8],
        encoding: StorageEncoding,
    ) -> (PathBuf, AssetStore) {
        let source = temp.0.join("source");
        let stage_dir = source.join("Game/Test");
        fs::create_dir_all(&stage_dir).unwrap();
        fs::write(stage_dir.join("TEST.hpc"), stored).unwrap();
        let profile = AssetProfile {
            schema_version: 1,
            id: "synthetic-stage-hpc".to_owned(),
            roots: vec!["Game".to_owned()],
            empty_directories: Vec::new(),
            sentinels: Vec::new(),
            files: vec![AssetFile {
                path: "Game/Test/TEST.hpc".to_owned(),
                size: stored.len() as u64,
                sha256: sha256(stored),
                encoding,
            }],
        };
        let reviewed = ReviewedProfile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
        let output = temp.0.join("runtime-data");
        import_source(&source, &output, &reviewed).unwrap();
        let store = AssetStore::open(
            &output,
            &reviewed,
            DecodeLimits {
                max_stored_bytes: 1024 * 1024,
                max_payload_bytes: 1024 * 1024,
            },
        )
        .unwrap();
        (output, store)
    }

    fn assert_loaded_shape(geometry: &StageCompiledGeometry) {
        assert_eq!(geometry.hpc().origin_xz(), [0, 0]);
        assert_eq!(geometry.hpc().levels()[0].len(), 1);
        assert_eq!(geometry.hpc().vertices().len(), 3);
        assert_eq!(geometry.hpc().records().len(), 1);
        assert_eq!(geometry.hpc().candidates(), &[0]);
        assert_eq!((geometry.hpc().width(), geometry.hpc().height()), (1, 1));
    }

    #[test]
    fn loads_authenticated_raw_and_cmpr_snapshots() {
        let payload = triangle_hpc();
        for (stored, encoding) in [
            (payload.clone(), StorageEncoding::Raw),
            (cmpr(&payload), StorageEncoding::GzipCmpr),
        ] {
            let temp = TestDirectory::new();
            let (_, store) = open_store(&temp, &stored, encoding);
            let geometry = StageCompiledGeometry::load(&store, "game/test/test").unwrap();
            assert_loaded_shape(&geometry);
        }
    }

    #[test]
    fn owned_snapshot_survives_backing_store_removal() {
        let temp = TestDirectory::new();
        let (output, store) = open_store(&temp, &triangle_hpc(), StorageEncoding::Raw);
        let geometry = StageCompiledGeometry::load(&store, "Game/Test/TEST").unwrap();
        drop(store);
        fs::remove_dir_all(output).unwrap();
        assert_loaded_shape(&geometry);
        assert_eq!(geometry.provider().tables.records.len(), 1);
    }

    #[test]
    fn stored_byte_tamper_fails_before_hpc_parsing() {
        let temp = TestDirectory::new();
        let (output, store) = open_store(&temp, &triangle_hpc(), StorageEncoding::Raw);
        fs::write(output.join("Game/Test/TEST.hpc"), b"bad").unwrap();
        assert!(matches!(
            StageCompiledGeometry::load(&store, "Game/Test/TEST"),
            Err(StageCompiledGeometryLoadError::Store(
                AssetStoreError::Verification(_)
            ))
        ));
    }

    #[test]
    fn authenticated_malformed_payload_reports_parser_error() {
        let temp = TestDirectory::new();
        let (_, store) = open_store(&temp, b"bad", StorageEncoding::Raw);
        let error = StageCompiledGeometry::load(&store, "Game/Test/TEST").unwrap_err();
        assert!(
            matches!(
                &error,
                StageCompiledGeometryLoadError::Parse(HpcError::Truncated {
                    section: "header",
                    offset: 0,
                    needed: 20,
                    available: 3,
                })
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn missing_and_unsafe_stage_identities_remain_store_errors() {
        let temp = TestDirectory::new();
        let (_, store) = open_store(&temp, &triangle_hpc(), StorageEncoding::Raw);
        for stage in ["Game/Test/MISSING", "../TEST", "/Game/Test/TEST", "C:TEST"] {
            assert!(matches!(
                StageCompiledGeometry::load(&store, stage),
                Err(StageCompiledGeometryLoadError::Store(
                    AssetStoreError::Resolve(_)
                ))
            ));
        }
    }

    #[test]
    fn authenticated_snapshot_supplies_all_eight_executor_lanes() {
        let temp = TestDirectory::new();
        let (_, store) = open_store(&temp, &cmpr(&triangle_hpc()), StorageEncoding::GzipCmpr);
        let geometry = StageCompiledGeometry::load(&store, "Game/Test/TEST").unwrap();
        let mut providers = geometry.eight_lane_providers();
        let source = [0x4000, i32::MIN, 0x4000];
        let input = CompiledEightLaneExecutionInputs {
            source_vectors: [source; 8],
            gate_words: [0; 8],
            drive_words: [0; 8],
            first_four_subtractions: [0; 4],
        };
        let state = CompiledEightLaneExecutionState {
            handles: [0; 8],
            type_words: [u16::MAX; 8],
            output_vectors: [[i32::MIN; 3]; 8],
            lane_words: [-99; 8],
            accumulator_words: [0; 8],
            latch_67c: 0,
        };
        let mut config = |_lane| -> Result<u8, std::convert::Infallible> { Ok(0) };
        let success = execute_compiled_eight_lanes(input, state, &mut providers, &mut config)
            .expect("authenticated stage geometry should execute every lane");
        assert_eq!(success.state.handles, [0; 8]);
        assert_eq!(success.state.type_words, [0x1234; 8]);
        assert_eq!(success.state.output_vectors, [[0, 0x1_0000, 0]; 8]);
        assert!(
            success
                .lanes
                .iter()
                .all(|lane| lane.dispatch.type_word_write == Some(0x1234))
        );
    }

    #[test]
    fn authenticated_parser_error_does_not_reveal_the_store_root() {
        let temp = TestDirectory::new();
        let (_, store) = open_store(&temp, b"bad", StorageEncoding::Raw);
        let error = StageCompiledGeometry::load(&store, "Game/Test/TEST").unwrap_err();
        assert!(!error.to_string().contains(temp.0.to_str().unwrap()));
    }
}
