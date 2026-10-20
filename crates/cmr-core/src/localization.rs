use std::fmt;

/// Number of text entries indexed for each language by the recovered frontend.
pub const FRONTEND_TEXT_COUNT: usize = 0xfe;

/// Number of text entries indexed by the recovered in-game text loader.
pub const GAME_TEXT_COUNT: usize = 0x142;

/// Diagnostic returned by the original lookup routine for an index at or
/// beyond the configured table width.
pub const INVALID_TEXT: &[u8] = b"INVALID TEXT STRING";

/// Language numbers stored in the low three bits of the recovered settings
/// byte.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Language {
    English = 0,
    French = 1,
    German = 2,
    Spanish = 3,
    Italian = 4,
}

impl Language {
    /// Reconstructs the exact case-sensitive registry value mapping. Missing,
    /// malformed, and unknown values deliberately fall back to English rather
    /// than reproducing the original routine's unchecked registry buffer.
    pub fn from_registry_value(value: Option<&[u8]>) -> Self {
        let value = value.map(c_string_prefix);
        match value {
            Some(b"FRENCH") => Self::French,
            Some(b"GERMAN") => Self::German,
            Some(b"SPANISH") => Self::Spanish,
            Some(b"ITALIAN") => Self::Italian,
            Some(b"ENGLISH") | None => Self::English,
            Some(_) => Self::English,
        }
    }

    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::English),
            1 => Some(Self::French),
            2 => Some(Self::German),
            3 => Some(Self::Spanish),
            4 => Some(Self::Italian),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    /// Source-root-relative path used while constructing the five frontend
    /// text rows. The shipped executable intentionally substitutes English
    /// for Italian.
    pub const fn frontend_path(self) -> &'static str {
        match self {
            Self::English | Self::Italian => "New_Fe/Language/english.txt",
            Self::French => "New_Fe/Language/french.txt",
            Self::German => "New_Fe/Language/german.txt",
            Self::Spanish => "New_Fe/Language/spanish.txt",
        }
    }

    /// Source-root-relative path used by the later in-game, single-language
    /// text loader.
    pub const fn game_path(self) -> &'static str {
        match self {
            Self::English => "Game/Language/english.txt",
            Self::French => "Game/Language/french.txt",
            Self::German => "Game/Language/german.txt",
            Self::Spanish => "Game/Language/spanish.txt",
            Self::Italian => "Game/Language/italian.txt",
        }
    }
}

fn c_string_prefix(value: &[u8]) -> &[u8] {
    match value.iter().position(|byte| *byte == 0) {
        Some(end) => &value[..end],
        None => value,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedTextTable {
    records: Vec<Vec<u8>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextTableError {
    MissingRecord {
        requested: usize,
        parsed: usize,
        offset: usize,
    },
}

impl fmt::Display for TextTableError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRecord {
                requested,
                parsed,
                offset,
            } => write!(
                formatter,
                "text table requested {requested} records but found {parsed} by byte offset {offset}"
            ),
        }
    }
}

impl std::error::Error for TextTableError {}

impl IndexedTextTable {
    /// Splits the requested number of records with the recovered byte-level
    /// grammar. Bytes `0x00..=0x1f` terminate a record and consecutive control
    /// bytes are skipped. Bytes `0x20..=0xff` are retained verbatim.
    ///
    /// The original routine had no bounds checks. This safe reconstruction
    /// accepts EOF as the final record terminator, matching the observed
    /// English frontend file, but reports an error if more records are needed.
    /// Bytes after the requested record count are intentionally ignored.
    pub fn parse(bytes: &[u8], requested: usize) -> Result<Self, TextTableError> {
        // A malformed public request must not turn a tiny input into a huge
        // allocation. At most one record can start at each input byte.
        let mut records = Vec::with_capacity(requested.min(bytes.len()));
        let mut cursor = 0;

        for _ in 0..requested {
            if cursor == bytes.len() {
                return Err(TextTableError::MissingRecord {
                    requested,
                    parsed: records.len(),
                    offset: cursor,
                });
            }

            let start = cursor;
            while cursor < bytes.len() && bytes[cursor] >= 0x20 {
                cursor += 1;
            }
            records.push(bytes[start..cursor].to_vec());

            if cursor == bytes.len() {
                if records.len() == requested {
                    break;
                }
                return Err(TextTableError::MissingRecord {
                    requested,
                    parsed: records.len(),
                    offset: cursor,
                });
            }

            while cursor < bytes.len() && bytes[cursor] < 0x20 {
                cursor += 1;
            }
        }

        Ok(Self { records })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Returns the recovered diagnostic text for an out-of-range index.
    pub fn lookup(&self, index: usize) -> &[u8] {
        self.records.get(index).map_or(INVALID_TEXT, Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_values_are_exact_c_strings_with_english_fallback() {
        let cases = [
            (Some(b"ENGLISH".as_slice()), Language::English),
            (Some(b"FRENCH\0ignored".as_slice()), Language::French),
            (Some(b"GERMAN".as_slice()), Language::German),
            (Some(b"SPANISH".as_slice()), Language::Spanish),
            (Some(b"ITALIAN".as_slice()), Language::Italian),
            (Some(b"english".as_slice()), Language::English),
            (Some(b"UNKNOWN".as_slice()), Language::English),
            (None, Language::English),
        ];

        for (value, expected) in cases {
            assert_eq!(Language::from_registry_value(value), expected);
        }
    }

    #[test]
    fn language_indices_are_bounded() {
        for (index, language) in [
            Language::English,
            Language::French,
            Language::German,
            Language::Spanish,
            Language::Italian,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(Language::from_index(index), Some(language));
            assert_eq!(language.index(), index);
        }
        assert_eq!(Language::from_index(5), None);
        assert_eq!(Language::from_index(usize::MAX), None);
    }

    #[test]
    fn frontend_substitutes_english_for_italian_but_game_does_not() {
        let cases = [
            (
                Language::English,
                "New_Fe/Language/english.txt",
                "Game/Language/english.txt",
            ),
            (
                Language::French,
                "New_Fe/Language/french.txt",
                "Game/Language/french.txt",
            ),
            (
                Language::German,
                "New_Fe/Language/german.txt",
                "Game/Language/german.txt",
            ),
            (
                Language::Spanish,
                "New_Fe/Language/spanish.txt",
                "Game/Language/spanish.txt",
            ),
            (
                Language::Italian,
                "New_Fe/Language/english.txt",
                "Game/Language/italian.txt",
            ),
        ];

        for (language, frontend, game) in cases {
            assert_eq!(language.frontend_path(), frontend);
            assert_eq!(language.game_path(), game);
        }
    }

    #[test]
    fn splitter_collapses_control_runs_and_preserves_high_bytes() {
        let table = IndexedTextTable::parse(b"A\r\nB\t\x1fC \xff\0", 3).unwrap();

        assert_eq!(table.lookup(0), b"A");
        assert_eq!(table.lookup(1), b"B");
        assert_eq!(table.lookup(2), b"C \xff");
    }

    #[test]
    fn splitter_preserves_an_initial_empty_record() {
        let table = IndexedTextTable::parse(b"\r\nnext", 2).unwrap();
        assert_eq!(table.lookup(0), b"");
        assert_eq!(table.lookup(1), b"next");
    }

    #[test]
    fn splitter_accepts_eof_only_for_the_final_requested_record() {
        let table = IndexedTextTable::parse(b"one\r\ntwo", 2).unwrap();
        assert_eq!(table.lookup(1), b"two");

        assert_eq!(
            IndexedTextTable::parse(b"one\r\ntwo", 3),
            Err(TextTableError::MissingRecord {
                requested: 3,
                parsed: 2,
                offset: 8,
            })
        );
    }

    #[test]
    fn splitter_ignores_trailing_controls_after_exact_record_count() {
        let table = IndexedTextTable::parse(b"one\r\n\r\n", 1).unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup(0), b"one");

        let table = IndexedTextTable::parse(b"one\ntwo", 1).unwrap();
        assert_eq!(table.len(), 1);
        assert_eq!(table.lookup(0), b"one");
    }

    #[test]
    fn zero_or_impossibly_large_requests_are_bounded() {
        assert!(IndexedTextTable::parse(b"ignored", 0).unwrap().is_empty());
        assert_eq!(
            IndexedTextTable::parse(b"one", usize::MAX),
            Err(TextTableError::MissingRecord {
                requested: usize::MAX,
                parsed: 1,
                offset: 3,
            })
        );
    }

    #[test]
    fn lookup_returns_original_fallback_at_and_above_bound() {
        let table = IndexedTextTable::parse(b"zero\none\n", 2).unwrap();
        assert_eq!(table.lookup(0), b"zero");
        assert_eq!(table.lookup(1), b"one");
        assert_eq!(table.lookup(2), INVALID_TEXT);
        assert_eq!(table.lookup(usize::MAX), INVALID_TEXT);
    }

    #[test]
    fn recovered_table_widths_parse_synthetic_inputs() {
        let frontend =
            IndexedTextTable::parse(&synthetic_records(FRONTEND_TEXT_COUNT), FRONTEND_TEXT_COUNT)
                .unwrap();
        let game =
            IndexedTextTable::parse(&synthetic_records(GAME_TEXT_COUNT), GAME_TEXT_COUNT).unwrap();

        assert_eq!(frontend.len(), FRONTEND_TEXT_COUNT);
        assert_eq!(game.len(), GAME_TEXT_COUNT);
    }

    fn synthetic_records(count: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        for index in 0..count {
            if index != 0 {
                bytes.extend_from_slice(b"\r\n");
            }
            bytes.extend_from_slice(b"text");
        }
        bytes
    }
}
