//! Launcher logic that runs anywhere; the Windows-only parts live in the
//! binary's `win32`, `ui` and `app` modules.
//!
//! Unlike its siblings, which assemble open-source components around their
//! games, this launcher applies its fixes in memory: it starts `Rally.exe`
//! suspended, checks and writes the byte fixes (`fixes`), loads the hook
//! library (`cmr-hooks`) and resumes it. The user's folder stays read-only;
//! everything happens in the runtime copy (`runtime`).
//!
//! The generic machinery (runtime sync, whole-file swaps, save mirroring,
//! `launcher.cfg` editing, edition hashes, PE headers) is the shared
//! `rea_launcher` SDK; this crate holds the game's own knowledge.

pub mod audience;
pub mod config;
pub mod edition;
pub mod fixes;
pub mod form;
pub mod hdpack;
pub mod instance;
#[cfg(any(windows, test))]
pub mod look;
pub mod registry;
pub mod runtime;
#[cfg(any(windows, test))]
pub mod window;

use fixes::{FixSet, Write};
use rea_launcher::pe::{Format, Pe};

/// A 32-bit executable's file bytes and headers.
pub struct Image<'a> {
    pub file: &'a [u8],
    pub pe: Pe,
}

impl<'a> Image<'a> {
    /// Parses a PE32 executable (the game is 32-bit).
    pub fn parse(file: &'a [u8]) -> Result<Image<'a>, String> {
        let pe = Pe::parse(file).map_err(|e| e.to_string())?;
        if pe.format != Format::Pe32 {
            return Err("not a 32-bit Windows executable".into());
        }
        Ok(Image { file, pe })
    }

    /// The file bytes that load at `[address, address + len)`, if that
    /// whole range is backed by the file inside one executable section.
    pub fn code_at(&self, address: u32, len: usize) -> Option<&'a [u8]> {
        let rva = u64::from(address).checked_sub(self.pe.image_base)?;
        let s = self.pe.sections.iter().find(|s| {
            s.characteristics & rea_launcher::pe::SCN_EXECUTE != 0
                && rva >= u64::from(s.rva)
                && (rva - u64::from(s.rva)) as usize + len
                    <= s.raw_size.min(s.virtual_size) as usize
        })?;
        let off = s.raw_offset as usize + (rva - u64::from(s.rva)) as usize;
        self.file.get(off..off + len)
    }
}

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
        shared: &[],
        own: &[Fix {
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

    /// A tiny PE32 with one executable section at 0x401000 holding `code`.
    pub(crate) fn synthetic(code: &[u8]) -> Vec<u8> {
        use rea_launcher::pe::synthetic::{CODE, image};
        image(Format::Pe32, 0x40_0000, &[(".text", 0x1000, code, CODE)])
    }

    #[test]
    fn maps_code_addresses() {
        let file = synthetic(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let img = Image::parse(&file).unwrap();
        assert_eq!(img.pe.image_base, 0x400000);
        assert_eq!(img.code_at(0x401002, 3), Some(&[3, 4, 5][..]));
        assert_eq!(img.code_at(0x401006, 3), None, "runs past the section");
        assert_eq!(img.code_at(0x400fff, 1), None, "before the section");
    }

    #[test]
    fn rejects_non_pe_and_64_bit() {
        assert!(Image::parse(b"hello").is_err());
        let pe64 = rea_launcher::pe::synthetic::image(
            Format::Pe32Plus,
            0x1_4000_0000,
            &[(".text", 0x1000, &[0x90], rea_launcher::pe::synthetic::CODE)],
        );
        assert_eq!(
            Image::parse(&pe64).err().as_deref(),
            Some("not a 32-bit Windows executable")
        );
    }

    #[test]
    fn prepare_reads_the_replaced_bytes() {
        let file = synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let w = prepare(&img, &TEST_FIXES, &[], 0x500000).unwrap();
        assert_eq!(w[0].original, [0xec]);
        assert!(w[1].original.is_empty());
    }

    #[test]
    fn data_is_filled_only_when_sizes_match() {
        let file = synthetic(&[0x55, 0x8b, 0xec, 0x90]);
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
        let file = synthetic(&[0x90]);
        let img = Image::parse(&file).unwrap();
        assert!(prepare(&img, &TEST_FIXES, &[], 0x500000).is_err());
    }
}
