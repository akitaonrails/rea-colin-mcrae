//! Launcher logic that runs anywhere; the Windows-only parts live in `win32`.

pub mod config;
pub mod edition;
pub mod fixes;
pub mod pe;
pub mod registry;
pub mod runtime;

use fixes::{FixSet, Write};
use pe::Image;

/// A write plus the file bytes it replaces, checked against memory first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckedWrite {
    pub write: Write,
    /// Empty for cave writes.
    pub original: Vec<u8>,
}

/// Assembles the active fixes (see `FixSet::active`) for an image and pairs
/// each image write with the bytes the file has there. Fails when a site is
/// not file-backed code.
pub fn prepare(
    image: &Image,
    fixes: &FixSet,
    skip: &[&str],
    cave_base: u32,
) -> Result<Vec<CheckedWrite>, String> {
    fixes
        .assemble(cave_base, skip)
        .into_iter()
        .map(|write| {
            let original = if write.in_image {
                image
                    .code_at(write.address, write.bytes.len())
                    .ok_or_else(|| {
                        format!(
                            "{}: 0x{:08x} is not code in this executable",
                            write.fix, write.address
                        )
                    })?
                    .to_vec()
            } else {
                Vec::new()
            };
            Ok(CheckedWrite { write, original })
        })
        .collect()
}

/// Replaces the bytes of the write at `address` with launch-time values: a
/// cave data area such as `fixes::F7_TABLE`, or an immediate in the game's
/// code whose piece carries the original value as a placeholder (the check
/// against the file still compares the original bytes). Returns false when
/// no such write is active or the size differs.
pub fn fill_data(writes: &mut [CheckedWrite], address: u32, bytes: &[u8]) -> bool {
    match writes.iter_mut().find(|w| w.write.address == address) {
        Some(w) if w.write.bytes.len() == bytes.len() => {
            w.write.bytes = bytes.to_vec();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fixes::{At, Fix, Op, Piece};

    static TEST_FIXES: FixSet = FixSet {
        fixes: &[Fix {
            id: "T1",
            what: "test",
            optional: false,
            code: &[
                Piece {
                    at: At::Image(0x401002),
                    ops: &[Op::Raw(&[0xc3])],
                },
                Piece {
                    at: At::Cave(0),
                    ops: &[Op::Jmp(At::Image(0x401000))],
                },
            ],
        }],
    };

    #[test]
    fn prepare_reads_the_replaced_bytes() {
        let file = pe::tests::synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let w = prepare(&img, &TEST_FIXES, &[], 0x500000).unwrap();
        assert_eq!(w[0].original, [0xec]);
        assert!(w[1].original.is_empty());
    }

    #[test]
    fn data_is_filled_only_when_sizes_match() {
        let file = pe::tests::synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let mut w = prepare(&img, &TEST_FIXES, &[], 0x500000).unwrap();
        assert!(!fill_data(&mut w, 0x500000, &[1, 2]));
        assert!(fill_data(&mut w, 0x500000, &[1, 2, 3, 4, 5]));
        assert_eq!(w[1].write.bytes, [1, 2, 3, 4, 5]);
        // An image placeholder is replaced; its original bytes stay as read.
        assert!(fill_data(&mut w, 0x401002, &[0x90]));
        assert_eq!(
            (w[0].write.bytes.as_slice(), w[0].original.as_slice()),
            (&[0x90][..], &[0xec][..])
        );
    }

    #[test]
    fn prepare_refuses_sites_outside_code() {
        let file = pe::tests::synthetic(&[0x90]);
        let img = Image::parse(&file).unwrap();
        assert!(prepare(&img, &TEST_FIXES, &[], 0x500000).is_err());
    }
}
