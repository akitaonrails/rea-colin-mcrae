//! Versioning policy for the two on-disk imported-data schemas.
//!
//! Imported data crosses two versioned boundaries:
//!
//! - the **reviewed source profile** (`profiles/*.json`, `schema_version`),
//!   checked into source control because it contains metadata only;
//! - the **import state** (`<runtime-data>/import-state.json`,
//!   `schema_version`) written by the importer and read when a store opens.
//!
//! Both readers are deliberately strict: they accept exactly the version this
//! build was written against and reject anything else. Imported trees are
//! *derived* data, so an incompatible version is never migrated in place - the
//! caller regenerates the artifact from the user's own media with the matching
//! importer. That keeps the recovery baseline and the on-disk contract in
//! lockstep instead of accumulating translation layers for data nobody needs
//! to preserve.
//!
//! When a change needs a bump, it must be a semantic or required-field change:
//! additive optional fields that readers can ignore leave the version alone.
//! A bump must update these constants, every writer, the worked example in
//! `docs/ASSET_PIPELINE.md`, and add a profile fixture at the new version plus
//! the rejection tests below.

/// Version of the reviewed source-profile JSON this build accepts.
pub const PROFILE_SCHEMA_VERSION: u32 = 1;

/// Version of `import-state.json` this build accepts.
pub const IMPORT_STATE_SCHEMA_VERSION: u32 = 1;

/// What a reader must do with an on-disk artifact at a given schema version.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SchemaStatus {
    /// The artifact matches the version this build understands.
    Supported,
    /// The artifact was written by a different schema generation. It is
    /// derived data: fail closed and regenerate it from the user's media.
    Regenerate,
}

/// Compatibility of a reviewed-profile `schema_version`.
#[must_use]
pub const fn profile_schema_status(found: u32) -> SchemaStatus {
    if found == PROFILE_SCHEMA_VERSION {
        SchemaStatus::Supported
    } else {
        SchemaStatus::Regenerate
    }
}

/// Compatibility of an `import-state.json` `schema_version`.
#[must_use]
pub const fn import_state_schema_status(found: u32) -> SchemaStatus {
    if found == IMPORT_STATE_SCHEMA_VERSION {
        SchemaStatus::Supported
    } else {
        SchemaStatus::Regenerate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_current_generation_is_supported() {
        assert_eq!(
            profile_schema_status(PROFILE_SCHEMA_VERSION),
            SchemaStatus::Supported
        );
        assert_eq!(
            import_state_schema_status(IMPORT_STATE_SCHEMA_VERSION),
            SchemaStatus::Supported
        );
        // Older and newer generations are both rejected: there is no implicit
        // downgrade path for derived data.
        for version in [0, PROFILE_SCHEMA_VERSION + 1, 99] {
            assert_eq!(profile_schema_status(version), SchemaStatus::Regenerate);
            assert_eq!(
                import_state_schema_status(version),
                SchemaStatus::Regenerate
            );
        }
    }
}
