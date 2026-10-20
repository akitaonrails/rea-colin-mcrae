//! Parsers for data formats used by the original game.
//!
//! The crate owns format validation only. Filesystem lookup, source-profile
//! verification, and caching belong to higher layers.

pub mod bitmap;
pub mod font;
pub mod riff_palette;
pub mod stored_asset;
