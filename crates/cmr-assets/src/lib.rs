//! Verification and import of user-owned game data.

mod catalog;
mod import;
mod path;
mod profile;
mod resolver;
mod schema;
mod store;
mod surface_import;
mod surface_profile;
mod surface_store;
mod verify;

pub use catalog::catalog_standard_source;
pub use import::{ImportError, ImportSummary, import_source};
pub use path::normalize_relative_path;
pub use profile::{
    AssetFile, AssetProfile, ProfileError, ReviewedProfile, ReviewedProfileError, StorageEncoding,
};
pub use resolver::{AssetIndex, ResolveError};
pub use schema::{
    IMPORT_STATE_SCHEMA_VERSION, PROFILE_SCHEMA_VERSION, SchemaStatus, import_state_schema_status,
    profile_schema_status,
};
pub use store::{AssetStore, AssetStoreError};
pub use surface_import::{
    SURFACE_DATA_FILE, SURFACE_IMPORT_SCHEMA_VERSION, SURFACE_STATE_FILE, SurfaceImportError,
    SurfaceImportFile, SurfaceImportSummary, extract_surface_tables,
};
pub use surface_profile::{
    ReviewedSurfaceProfile, ReviewedSurfaceProfileError, SurfaceExecutableProfile,
    SurfaceProfileError, SurfaceTableEncoding, SurfaceTableProfile, SurfaceTableSpec,
};
pub use surface_store::{
    SurfaceDataError, SurfaceTableStore, SurfaceTableStoreError, decode_surface_table_data,
};
pub use verify::{VerificationError, VerificationSummary, verify_source};
