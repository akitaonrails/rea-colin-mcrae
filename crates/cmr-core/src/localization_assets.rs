use std::fmt;

use cmr_assets::{AssetStore, AssetStoreError};

use crate::localization::{
    FRONTEND_TEXT_COUNT, GAME_TEXT_COUNT, IndexedTextTable, Language, TextTableError,
};

#[derive(Debug)]
pub enum LocalizationLoadError {
    Asset {
        path: String,
        source: AssetStoreError,
    },
    TextTable {
        path: String,
        source: TextTableError,
    },
}

impl fmt::Display for LocalizationLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asset { path, source } => write!(formatter, "cannot load {path}: {source}"),
            Self::TextTable { path, source } => {
                write!(formatter, "cannot parse text table {path}: {source}")
            }
        }
    }
}

impl std::error::Error for LocalizationLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Asset { source, .. } => Some(source),
            Self::TextTable { source, .. } => Some(source),
        }
    }
}

/// Loads the recovered fixed-width frontend table for one language row.
///
/// Path selection retains the shipped Italian-to-English frontend quirk.
pub fn load_frontend_text_table(
    store: &AssetStore,
    language: Language,
) -> Result<IndexedTextTable, LocalizationLoadError> {
    load_text_table(store, language.frontend_path(), FRONTEND_TEXT_COUNT)
}

/// Loads the recovered fixed-width in-game table for the selected language.
pub fn load_game_text_table(
    store: &AssetStore,
    language: Language,
) -> Result<IndexedTextTable, LocalizationLoadError> {
    load_text_table(store, language.game_path(), GAME_TEXT_COUNT)
}

fn load_text_table(
    store: &AssetStore,
    path: &str,
    requested: usize,
) -> Result<IndexedTextTable, LocalizationLoadError> {
    let bytes = store
        .load(path)
        .map_err(|source| LocalizationLoadError::Asset {
            path: path.to_owned(),
            source,
        })?;
    IndexedTextTable::parse(&bytes, requested).map_err(|source| LocalizationLoadError::TextTable {
        path: path.to_owned(),
        source,
    })
}
