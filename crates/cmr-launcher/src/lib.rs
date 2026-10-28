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

/// Assembles the fixes for an image and pairs each image write with the
/// bytes the file has there. Fails when a site is not file-backed code.
pub fn prepare(image: &Image, fixes: &FixSet, cave_base: u32) -> Result<Vec<CheckedWrite>, String> {
    fixes
        .assemble(cave_base)
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

#[cfg(test)]
mod tests {
    use super::*;
    use fixes::{At, Fix, Op, Piece};

    static TEST_FIXES: FixSet = FixSet {
        fixes: &[Fix {
            id: "T1",
            what: "test",
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
        let w = prepare(&img, &TEST_FIXES, 0x500000).unwrap();
        assert_eq!(w[0].original, [0xec]);
        assert!(w[1].original.is_empty());
    }

    #[test]
    fn prepare_refuses_sites_outside_code() {
        let file = pe::tests::synthetic(&[0x90]);
        let img = Image::parse(&file).unwrap();
        assert!(prepare(&img, &TEST_FIXES, 0x500000).is_err());
    }
}
