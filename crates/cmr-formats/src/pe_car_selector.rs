//! Authenticated extraction and payload-free encoding of the car-code selector table.

use std::collections::BTreeSet;
use std::fmt;

use crate::pe32::{Pe32Error, Pe32Image};

pub const CAR_SELECTOR_COUNT: usize = 14;
pub const CAR_SELECTOR_CODE_BYTES: usize = 32;
pub const CAR_SELECTOR_FORMAT_VERSION: u32 = 1;
const MAGIC: &[u8; 8] = b"CMRCSEL\0";
const HEADER_LEN: usize = 20;
pub const CAR_SELECTOR_ENCODED_LEN: usize =
    HEADER_LEN + CAR_SELECTOR_COUNT * CAR_SELECTOR_CODE_BYTES;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CarSelectorLayout {
    pub image_base: u32,
    pub consumer_va: u32,
    pub pointer_table_va: u32,
    pub selector_count: usize,
    pub max_code_bytes: usize,
}

pub const OFFICIAL_EU_CAR_SELECTOR_LAYOUT: CarSelectorLayout = CarSelectorLayout {
    image_base: 0x0040_0000,
    consumer_va: 0x0044_ddee,
    pointer_table_va: 0x004f_0d68,
    selector_count: CAR_SELECTOR_COUNT,
    max_code_bytes: CAR_SELECTOR_CODE_BYTES - 1,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CarSelectorMap {
    codes: [String; CAR_SELECTOR_COUNT],
}

impl CarSelectorMap {
    pub fn new(codes: [String; CAR_SELECTOR_COUNT]) -> Result<Self, CarSelectorError> {
        validate_codes(&codes)?;
        Ok(Self { codes })
    }

    pub fn codes(&self) -> &[String; CAR_SELECTOR_COUNT] {
        &self.codes
    }
    pub fn selector_for_code(&self, code: &str) -> Option<u8> {
        self.codes
            .iter()
            .position(|item| item.eq_ignore_ascii_case(code))
            .map(|i| i as u8)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CarSelectorError {
    Pe(String),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    UnsupportedLayout,
    ConsumerInstruction,
    Unterminated {
        selector: usize,
        address: u32,
    },
    Empty {
        selector: usize,
    },
    InvalidByte {
        selector: usize,
        offset: usize,
        value: u8,
    },
    Duplicate {
        selector: usize,
    },
    WrongLength {
        expected: usize,
        actual: usize,
    },
    InvalidHeader,
    InvalidRecord {
        selector: usize,
    },
}

impl fmt::Display for CarSelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(e) => write!(f, "invalid PE32 image: {e}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "image base mismatch: expected {expected:#x}, got {actual:#x}"
            ),
            Self::UnsupportedLayout => write!(f, "unsupported selector layout"),
            Self::ConsumerInstruction => write!(
                f,
                "selector consumer does not reference the profiled pointer table"
            ),
            Self::Unterminated { selector, address } => write!(
                f,
                "selector {selector} string at {address:#x} is not NUL terminated"
            ),
            Self::Empty { selector } => write!(f, "selector {selector} has an empty code"),
            Self::InvalidByte {
                selector,
                offset,
                value,
            } => write!(
                f,
                "selector {selector} has invalid byte {value:#x} at code offset {offset}"
            ),
            Self::Duplicate { selector } => write!(
                f,
                "selector {selector} duplicates an earlier code ignoring ASCII case"
            ),
            Self::WrongLength { expected, actual } => {
                write!(f, "wrong encoded length: expected {expected}, got {actual}")
            }
            Self::InvalidHeader => write!(f, "invalid car-selector header"),
            Self::InvalidRecord { selector } => {
                write!(f, "invalid fixed-width record for selector {selector}")
            }
        }
    }
}
impl std::error::Error for CarSelectorError {}

fn pe_error(error: Pe32Error) -> CarSelectorError {
    CarSelectorError::Pe(format!("{error:?}"))
}

pub fn extract_car_selectors(
    input: &[u8],
    layout: &CarSelectorLayout,
) -> Result<CarSelectorMap, CarSelectorError> {
    if layout.selector_count != CAR_SELECTOR_COUNT
        || layout.max_code_bytes != CAR_SELECTOR_CODE_BYTES - 1
    {
        return Err(CarSelectorError::UnsupportedLayout);
    }
    let image = Pe32Image::parse(input).map_err(pe_error)?;
    if image.image_base() != layout.image_base {
        return Err(CarSelectorError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let consumer = image
        .read_code_va_range(layout.consumer_va, 7)
        .map_err(pe_error)?;
    if consumer[..3] != [0x8b, 0x04, 0xb5]
        || u32::from_le_bytes(consumer[3..7].try_into().unwrap()) != layout.pointer_table_va
    {
        return Err(CarSelectorError::ConsumerInstruction);
    }
    let pointers = image
        .read_va_range(layout.pointer_table_va, CAR_SELECTOR_COUNT * 4)
        .map_err(pe_error)?;
    let mut codes = std::array::from_fn(|_| String::new());
    for (selector, code) in codes.iter_mut().enumerate() {
        let start = selector * 4;
        let address = u32::from_le_bytes(pointers[start..start + 4].try_into().unwrap());
        let bytes = image
            .read_va_range(address, layout.max_code_bytes + 1)
            .map_err(pe_error)?;
        let end = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or(CarSelectorError::Unterminated { selector, address })?;
        *code = String::from_utf8(validate_code(selector, &bytes[..end])?.to_vec())
            .expect("validated ASCII");
    }
    CarSelectorMap::new(codes)
}

pub fn encode_car_selectors(map: &CarSelectorMap) -> [u8; CAR_SELECTOR_ENCODED_LEN] {
    let mut output = [0u8; CAR_SELECTOR_ENCODED_LEN];
    output[..8].copy_from_slice(MAGIC);
    output[8..12].copy_from_slice(&CAR_SELECTOR_FORMAT_VERSION.to_le_bytes());
    output[12..16].copy_from_slice(&(CAR_SELECTOR_COUNT as u32).to_le_bytes());
    output[16..20].copy_from_slice(&(CAR_SELECTOR_CODE_BYTES as u32).to_le_bytes());
    for (selector, code) in map.codes.iter().enumerate() {
        let start = HEADER_LEN + selector * CAR_SELECTOR_CODE_BYTES;
        output[start..start + code.len()].copy_from_slice(code.as_bytes());
    }
    output
}

pub fn decode_car_selectors(input: &[u8]) -> Result<CarSelectorMap, CarSelectorError> {
    if input.len() != CAR_SELECTOR_ENCODED_LEN {
        return Err(CarSelectorError::WrongLength {
            expected: CAR_SELECTOR_ENCODED_LEN,
            actual: input.len(),
        });
    }
    if &input[..8] != MAGIC
        || u32::from_le_bytes(input[8..12].try_into().unwrap()) != CAR_SELECTOR_FORMAT_VERSION
        || u32::from_le_bytes(input[12..16].try_into().unwrap()) != CAR_SELECTOR_COUNT as u32
        || u32::from_le_bytes(input[16..20].try_into().unwrap()) != CAR_SELECTOR_CODE_BYTES as u32
    {
        return Err(CarSelectorError::InvalidHeader);
    }
    let mut codes = std::array::from_fn(|_| String::new());
    for (selector, code) in codes.iter_mut().enumerate() {
        let start = HEADER_LEN + selector * CAR_SELECTOR_CODE_BYTES;
        let record = &input[start..start + CAR_SELECTOR_CODE_BYTES];
        let end = record
            .iter()
            .position(|b| *b == 0)
            .ok_or(CarSelectorError::InvalidRecord { selector })?;
        if record[end..].iter().any(|b| *b != 0) {
            return Err(CarSelectorError::InvalidRecord { selector });
        }
        *code = String::from_utf8(validate_code(selector, &record[..end])?.to_vec())
            .expect("validated ASCII");
    }
    CarSelectorMap::new(codes)
}

fn validate_codes(codes: &[String; CAR_SELECTOR_COUNT]) -> Result<(), CarSelectorError> {
    let mut seen = BTreeSet::new();
    for (selector, code) in codes.iter().enumerate() {
        validate_code(selector, code.as_bytes())?;
        if !seen.insert(code.to_ascii_lowercase()) {
            return Err(CarSelectorError::Duplicate { selector });
        }
    }
    Ok(())
}

fn validate_code(selector: usize, bytes: &[u8]) -> Result<&[u8], CarSelectorError> {
    if bytes.is_empty() {
        return Err(CarSelectorError::Empty { selector });
    }
    if bytes.len() >= CAR_SELECTOR_CODE_BYTES {
        return Err(CarSelectorError::InvalidRecord { selector });
    }
    for (offset, value) in bytes.iter().copied().enumerate() {
        if !(value.is_ascii_alphanumeric() || matches!(value, b'_' | b'-')) {
            return Err(CarSelectorError::InvalidByte {
                selector,
                offset,
                value,
            });
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_pe() -> (Vec<u8>, CarSelectorLayout) {
        const PE: usize = 0x80;
        const OPTIONAL: usize = PE + 24;
        const SECTIONS: usize = OPTIONAL + 0xe0;
        const RAW: usize = 0x200;
        let layout = CarSelectorLayout {
            image_base: 0x400000,
            consumer_va: 0x401000,
            pointer_table_va: 0x401100,
            selector_count: CAR_SELECTOR_COUNT,
            max_code_bytes: 31,
        };
        let mut bytes = vec![0_u8; RAW + 0x1000];
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE as u32);
        bytes[PE..PE + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE + 4, 0x14c);
        put_u16(&mut bytes, PE + 6, 1);
        put_u16(&mut bytes, PE + 20, 0xe0);
        put_u16(&mut bytes, OPTIONAL, 0x10b);
        put_u32(&mut bytes, OPTIONAL + 28, layout.image_base);
        put_u32(&mut bytes, SECTIONS + 8, 0x1000);
        put_u32(&mut bytes, SECTIONS + 12, 0x1000);
        put_u32(&mut bytes, SECTIONS + 16, 0x1000);
        put_u32(&mut bytes, SECTIONS + 20, RAW as u32);
        put_u32(&mut bytes, SECTIONS + 36, 0x6000_0020);
        let offset = |va: u32| RAW + (va - 0x401000) as usize;
        bytes[offset(layout.consumer_va)..offset(layout.consumer_va) + 7]
            .copy_from_slice(&[0x8b, 0x04, 0xb5, 0x00, 0x11, 0x40, 0x00]);
        for selector in 0..CAR_SELECTOR_COUNT {
            let string_va = 0x401200 + selector as u32 * 0x20;
            put_u32(
                &mut bytes,
                offset(layout.pointer_table_va) + selector * 4,
                string_va,
            );
            let code = format!("CAR{selector:02}");
            let start = offset(string_va);
            bytes[start..start + code.len()].copy_from_slice(code.as_bytes());
        }
        (bytes, layout)
    }

    fn map() -> CarSelectorMap {
        CarSelectorMap::new(std::array::from_fn(|i| format!("CAR{i:02}"))).unwrap()
    }

    #[test]
    fn fixed_codec_round_trips_and_rejects_dirty_padding() {
        let expected = map();
        let mut encoded = encode_car_selectors(&expected);
        assert_eq!(decode_car_selectors(&encoded).unwrap(), expected);
        encoded[HEADER_LEN + 31] = 1;
        assert!(matches!(
            decode_car_selectors(&encoded),
            Err(CarSelectorError::InvalidRecord { selector: 0 })
        ));
    }

    #[test]
    fn codec_accepts_the_exact_maximum_code_width() {
        let map = CarSelectorMap::new(std::array::from_fn(|i| {
            format!("{i:02}{}", "A".repeat(CAR_SELECTOR_CODE_BYTES - 3))
        }))
        .unwrap();
        assert_eq!(
            decode_car_selectors(&encode_car_selectors(&map)).unwrap(),
            map
        );
    }

    #[test]
    fn decoder_rejects_every_header_and_length_boundary() {
        let encoded = encode_car_selectors(&map());
        assert!(matches!(
            decode_car_selectors(&encoded[..encoded.len() - 1]),
            Err(CarSelectorError::WrongLength { .. })
        ));
        let mut trailing = encoded.to_vec();
        trailing.push(0);
        assert!(matches!(
            decode_car_selectors(&trailing),
            Err(CarSelectorError::WrongLength { .. })
        ));
        for offset in [0, 8, 12, 16] {
            let mut candidate = encoded;
            candidate[offset] ^= 1;
            assert_eq!(
                decode_car_selectors(&candidate),
                Err(CarSelectorError::InvalidHeader)
            );
        }
    }

    #[test]
    fn decoder_rejects_empty_unterminated_unsafe_and_duplicate_records() {
        let encoded = encode_car_selectors(&map());
        let mut empty = encoded;
        empty[HEADER_LEN..HEADER_LEN + CAR_SELECTOR_CODE_BYTES].fill(0);
        assert_eq!(
            decode_car_selectors(&empty),
            Err(CarSelectorError::Empty { selector: 0 })
        );
        let mut unterminated = encoded;
        unterminated[HEADER_LEN..HEADER_LEN + CAR_SELECTOR_CODE_BYTES].fill(b'A');
        assert_eq!(
            decode_car_selectors(&unterminated),
            Err(CarSelectorError::InvalidRecord { selector: 0 })
        );
        let mut unsafe_code = encoded;
        unsafe_code[HEADER_LEN] = b'/';
        assert!(matches!(
            decode_car_selectors(&unsafe_code),
            Err(CarSelectorError::InvalidByte { selector: 0, .. })
        ));
        let mut duplicate = encoded;
        let second = HEADER_LEN + CAR_SELECTOR_CODE_BYTES;
        duplicate[second..second + 5].copy_from_slice(b"car00");
        assert_eq!(
            decode_car_selectors(&duplicate),
            Err(CarSelectorError::Duplicate { selector: 1 })
        );
    }

    #[test]
    fn rejects_case_folded_collisions_and_unsafe_codes() {
        let mut codes = std::array::from_fn(|i| format!("CAR{i:02}"));
        codes[1] = "car00".into();
        assert!(matches!(
            CarSelectorMap::new(codes),
            Err(CarSelectorError::Duplicate { selector: 1 })
        ));
        let mut codes = std::array::from_fn(|i| format!("CAR{i:02}"));
        codes[0] = "../bad".into();
        assert!(matches!(
            CarSelectorMap::new(codes),
            Err(CarSelectorError::InvalidByte { .. })
        ));
    }

    #[test]
    fn extracts_only_after_authenticating_consumer_and_table() {
        let (mut bytes, layout) = synthetic_pe();
        let extracted = extract_car_selectors(&bytes, &layout).unwrap();
        assert_eq!(extracted.selector_for_code("car13"), Some(13));
        bytes[0x200] = 0x90;
        assert_eq!(
            extract_car_selectors(&bytes, &layout),
            Err(CarSelectorError::ConsumerInstruction)
        );
    }

    #[test]
    fn extractor_rejects_layout_image_mapping_and_pointee_failures() {
        let (bytes, layout) = synthetic_pe();
        let mut wrong = layout;
        wrong.selector_count -= 1;
        assert_eq!(
            extract_car_selectors(&bytes, &wrong),
            Err(CarSelectorError::UnsupportedLayout)
        );
        let mut wrong = layout;
        wrong.max_code_bytes -= 1;
        assert_eq!(
            extract_car_selectors(&bytes, &wrong),
            Err(CarSelectorError::UnsupportedLayout)
        );
        let mut wrong = layout;
        wrong.image_base += 1;
        assert!(matches!(
            extract_car_selectors(&bytes, &wrong),
            Err(CarSelectorError::ImageBase { .. })
        ));
        let mut wrong = layout;
        wrong.consumer_va = 0x403000;
        assert!(matches!(
            extract_car_selectors(&bytes, &wrong),
            Err(CarSelectorError::Pe(_))
        ));

        let mut unmapped_table = bytes.clone();
        unmapped_table[0x203..0x207].copy_from_slice(&0x0040_3000_u32.to_le_bytes());
        let mut wrong = layout;
        wrong.pointer_table_va = 0x0040_3000;
        assert!(matches!(
            extract_car_selectors(&unmapped_table, &wrong),
            Err(CarSelectorError::Pe(_))
        ));

        let mut non_executable = bytes.clone();
        put_u32(&mut non_executable, 0x178 + 36, 0x4000_0040);
        assert!(matches!(
            extract_car_selectors(&non_executable, &layout),
            Err(CarSelectorError::Pe(_))
        ));

        let mut ambiguous = bytes.clone();
        put_u16(&mut ambiguous, 0x80 + 6, 2);
        ambiguous.copy_within(0x178..0x178 + 40, 0x178 + 40);
        assert!(matches!(
            extract_car_selectors(&ambiguous, &layout),
            Err(CarSelectorError::Pe(_))
        ));

        let mut bad_table = bytes.clone();
        bad_table[0x203..0x207].copy_from_slice(&0x0040_0200_u32.to_le_bytes());
        assert_eq!(
            extract_car_selectors(&bad_table, &layout),
            Err(CarSelectorError::ConsumerInstruction)
        );
        let mut bad_pointer = bytes;
        bad_pointer[0x300..0x304].copy_from_slice(&0x0040_0200_u32.to_le_bytes());
        assert!(matches!(
            extract_car_selectors(&bad_pointer, &layout),
            Err(CarSelectorError::Pe(_))
        ));
    }

    #[test]
    fn extractor_rejects_unterminated_unsafe_and_duplicate_pointees() {
        let (mut bytes, layout) = synthetic_pe();
        bytes[0x400..0x420].fill(b'A');
        assert_eq!(
            extract_car_selectors(&bytes, &layout),
            Err(CarSelectorError::Unterminated {
                selector: 0,
                address: 0x401200
            })
        );
        let (mut bytes, layout) = synthetic_pe();
        bytes[0x400] = b'/';
        assert!(matches!(
            extract_car_selectors(&bytes, &layout),
            Err(CarSelectorError::InvalidByte { selector: 0, .. })
        ));
        let (mut bytes, layout) = synthetic_pe();
        bytes[0x420..0x425].copy_from_slice(b"car00");
        assert_eq!(
            extract_car_selectors(&bytes, &layout),
            Err(CarSelectorError::Duplicate { selector: 1 })
        );
    }
}
