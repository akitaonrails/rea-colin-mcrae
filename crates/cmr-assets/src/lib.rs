//! Verification and import of user-owned game data.

mod catalog;
mod import;
mod profile;
mod resolver;
mod store;
mod verify;

pub use catalog::catalog_standard_source;
pub use import::{ImportError, ImportSummary, import_source};
pub use profile::{
    AssetFile, AssetProfile, ProfileError, ReviewedProfile, ReviewedProfileError, StorageEncoding,
};
pub use resolver::{AssetIndex, ResolveError};
pub use store::{AssetStore, AssetStoreError};
pub use verify::{VerificationError, VerificationSummary, verify_source};
