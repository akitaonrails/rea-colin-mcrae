//! Parsers for data formats used by the original game.
//!
//! The crate owns format validation only. Filesystem lookup, source-profile
//! verification, and caching belong to higher layers.

pub mod bitmap;
pub mod c3d;
pub mod font;
mod pe32;
pub mod pe_compiled_initializer;
pub mod pe_surface;
pub mod riff_palette;
pub mod stage;
pub mod stored_asset;
pub mod wav;
