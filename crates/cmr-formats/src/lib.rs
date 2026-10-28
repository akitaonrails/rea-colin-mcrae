//! Parsers for data formats used by the original game.
//!
//! The crate owns format validation only. Filesystem lookup, source-profile
//! verification, and caching belong to higher layers.

pub mod bitmap;
pub mod c3d;
pub mod font;
pub mod hpc;
mod pe32;
pub mod pe_aux_control_initializer;
pub mod pe_car_selector;
pub mod pe_compiled_initializer;
pub mod pe_engine_audio_tables;
pub mod pe_rival_records;
pub mod pe_stage_weather_tables;
pub mod pe_static_tables;
pub mod pe_surface;
pub mod pe_surface_audio_tables;
pub mod pe_tyre_tables;
pub mod riff_palette;
pub mod stage;
pub mod stored_asset;
pub mod wav;
