//! Verification and import of user-owned game data.

mod aux_control_import;
mod aux_control_profile;
mod aux_control_store;
mod catalog;
mod compiled_initializer_import;
mod compiled_initializer_profile;
mod compiled_initializer_store;
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

pub use aux_control_import::{
    AUX_CONTROL_DATA_FILE, AUX_CONTROL_IMPORT_SCHEMA_VERSION, AUX_CONTROL_STATE_FILE,
    AuxControlImportError, AuxControlImportFile, AuxControlImportShape, AuxControlImportSummary,
    extract_aux_control_constants,
};
pub use aux_control_profile::{
    AUX_CONTROL_PROFILE_SCHEMA_VERSION, AuxControlEncoding, AuxControlExecutable,
    AuxControlProfile, AuxControlProfileError, AuxControlSpec, ReviewedAuxControlProfile,
    ReviewedAuxControlProfileError,
};
pub use aux_control_store::{AuxControlConstantsStore, AuxControlStoreError};
pub use catalog::catalog_standard_source;
pub use compiled_initializer_import::{
    COMPILED_INITIALIZER_DATA_FILE, COMPILED_INITIALIZER_IMPORT_SCHEMA_VERSION,
    COMPILED_INITIALIZER_STATE_FILE, CompiledInitializerImportError, CompiledInitializerImportFile,
    CompiledInitializerImportShape, CompiledInitializerImportSummary,
    extract_compiled_car_initializer,
};
pub use compiled_initializer_profile::{
    COMPILED_INITIALIZER_PROFILE_SCHEMA_VERSION, CompiledInitializerEncoding,
    CompiledInitializerExecutable, CompiledInitializerProfile, CompiledInitializerProfileError,
    CompiledInitializerSpec, ReviewedCompiledInitializerProfile,
    ReviewedCompiledInitializerProfileError,
};
pub use compiled_initializer_store::{CompiledInitializerStore, CompiledInitializerStoreError};
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
    SURFACE_CAPABILITY_DATA_FILE, SURFACE_CAPABILITY_IMPORT_SCHEMA_VERSION,
    SURFACE_CAPABILITY_STATE_FILE, SURFACE_DATA_FILE, SURFACE_IMPORT_SCHEMA_VERSION,
    SURFACE_STATE_FILE, SurfaceCapabilityImportSummary, SurfaceImportError, SurfaceImportFile,
    SurfaceImportSummary, extract_surface_capabilities, extract_surface_tables,
};
pub use surface_profile::{
    ReviewedSurfaceCapabilityProfile, ReviewedSurfaceProfile, ReviewedSurfaceProfileError,
    SURFACE_CAPABILITY_PROFILE_SCHEMA_VERSION, SurfaceCapabilityEncoding, SurfaceCapabilityProfile,
    SurfaceCapabilitySpec, SurfaceExecutableProfile, SurfaceProfileError, SurfaceTableEncoding,
    SurfaceTableProfile, SurfaceTableSpec,
};
pub use surface_store::{
    SurfaceCapabilityStore, SurfaceDataError, SurfaceTableStore, SurfaceTableStoreError,
    decode_surface_capability_data, decode_surface_table_data,
};
pub use verify::{VerificationError, VerificationSummary, verify_source};
