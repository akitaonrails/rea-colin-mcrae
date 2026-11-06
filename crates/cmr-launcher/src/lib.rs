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

pub mod config;
pub mod dgvoodoo;
pub mod edition;
pub mod fixes;
pub mod form;
pub mod hdpack;
pub mod instance;
#[cfg(any(windows, test))]
pub mod look;
pub mod plan;
pub mod registry;
pub mod runtime;
#[cfg(any(windows, test))]
pub mod window;

/// This launcher's commands on top of the family's (`--help`, `--check`):
/// a run with one of them prints its result (`report::Run::from_args`).
pub const COMMAND_FLAGS: &[&str] = &["--controllers"];

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
/// The hook library the game's runtime copy imports (F13): the DLL's file
/// name, where its address table goes in the cave, and the exports in table
/// order.
pub struct Hooks<'a> {
    pub dll: &'a str,
    pub table: u32,
    pub exports: &'a [&'a str],
}

/// Removes the static `DPLAYX.dll` import descriptor from `Rally.exe` and
/// points its two IAT slots (`0x4e602c` `DirectPlayEnumerateA`, `0x4e6030`
/// `DirectPlayCreate`) at fallback stubs in the 11-byte NOP alignment block
/// right before the thunks (`0x4b8755..0x4b8760`).
///
/// On Windows 10 and 11 where the optional legacy `DirectPlay` feature is not
/// enabled, `C:\Windows\SysWOW64\dplayx.dll` is absent and a static import of
/// `DPLAYX.dll` stops process creation with `STATUS_DLL_NOT_FOUND` (or opens
/// `fondue.exe /enable-feature:DirectPlay`) even for single-player. When
/// `cmrhooks.dll` loads, it replaces those two IAT slots with dynamic wrappers
/// that load `<SystemDir>\dplayx.dll` only when that file exists on disk.
pub fn detach_dplayx(file: &mut [u8]) -> bool {
    const IMP_RVA: u32 = 0x000e_8fa4;
    const NAME_RVA: u32 = 0x000e_9348;
    const IAT_RVA: u32 = 0x000e_602c;
    const STUB_OFF: usize = 0x000b_8755;
    const ENUM_STUB_VA: u32 = 0x004b_8755;
    const CREATE_STUB_VA: u32 = 0x004b_875a;

    if file.len() < 0x000e_9354 {
        return false;
    }
    let Some(pe_off) = file
        .get(0x3c..0x40)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()) as usize)
    else {
        return false;
    };
    let Some(imp_dir) = file.get(pe_off + 0x80..pe_off + 0x88) else {
        return false;
    };
    let imp_rva = u32::from_le_bytes(imp_dir[0..4].try_into().unwrap());
    let imp_size = u32::from_le_bytes(imp_dir[4..8].try_into().unwrap());
    if imp_rva != IMP_RVA || imp_size < 40 {
        return false;
    }
    let desc_off = IMP_RVA as usize;
    let name_rva = u32::from_le_bytes(file[desc_off + 12..desc_off + 16].try_into().unwrap());
    let iat_rva = u32::from_le_bytes(file[desc_off + 16..desc_off + 20].try_into().unwrap());
    if name_rva != NAME_RVA || iat_rva != IAT_RVA {
        return false;
    }
    if &file[NAME_RVA as usize..NAME_RVA as usize + 11] != b"DPLAYX.dll\0" {
        return false;
    }
    if file[STUB_OFF..STUB_OFF + 11] != [0x90; 11] {
        return false;
    }
    // 0x4b8755: xor eax, eax; ret 8  (DirectPlayEnumerateA -> DP_OK, 0 providers)
    // 0x4b875a: or eax, -1; ret 12   (DirectPlayCreate -> E_FAIL)
    file[STUB_OFF..STUB_OFF + 11].copy_from_slice(&[
        0x31, 0xc0, 0xc2, 0x08, 0x00, 0x83, 0xc8, 0xff, 0xc2, 0x0c, 0x00,
    ]);
    let iat = IAT_RVA as usize;
    file[iat..iat + 4].copy_from_slice(&ENUM_STUB_VA.to_le_bytes());
    file[iat + 4..iat + 8].copy_from_slice(&CREATE_STUB_VA.to_le_bytes());
    file[pe_off + 0x80..pe_off + 0x84].copy_from_slice(&(IMP_RVA + 20).to_le_bytes());
    file[pe_off + 0x84..pe_off + 0x88].copy_from_slice(&(imp_size - 20).to_le_bytes());
    file[desc_off..desc_off + 20].fill(0);
    file[NAME_RVA as usize..NAME_RVA as usize + 11].fill(0);
    true
}

/// The game's executable with every fix written into the file, for the
/// runtime copy (never the player's): each image write in place, the cave
/// as a new section `.rea` (its address is passed to `writes_for`), and the
/// hook library in the import table with its address table at the cave's
/// F13 table, so Windows loads the DLL and fills the table before the game
/// runs. Until v0.9.1 the launcher started the game suspended and wrote all
/// of this into its memory (`VirtualAllocEx`, `WriteProcessMemory`,
/// `CreateRemoteThread`), which antivirus heuristics flag as a trojan.
/// Returns the file and the writes it holds (for the log).
pub fn patched_exe(
    image: &Image,
    cave_size: u32,
    hooks: Option<Hooks>,
    writes_for: impl FnOnce(u32) -> Result<Vec<CheckedWrite>, String>,
) -> Result<(Vec<u8>, Vec<CheckedWrite>), String> {
    use rea_launcher::pe::edit;
    let err = |e: edit::EditError| e.to_string();
    let mut file = image.file.to_vec();
    edit::fix_base(&mut file).map_err(err)?;
    detach_dplayx(&mut file);
    let cave_rva = edit::next_section_rva(&file).map_err(err)?;
    let cave_base = if cave_size == 0 {
        0
    } else {
        (image.pe.image_base + u64::from(cave_rva)) as u32
    };
    let writes = writes_for(cave_base)?;
    let mut cave = vec![0u8; cave_size as usize];
    for w in &writes {
        let bytes = &w.write.bytes;
        if w.write.in_image {
            let at = image
                .pe
                .va_to_offset(u64::from(w.write.address))
                .filter(|&at| file.get(at..at + bytes.len()) == Some(&w.original[..]))
                .ok_or_else(|| {
                    format!(
                        "{}: 0x{:08x} doesn't match the executable",
                        w.write.fix, w.write.address
                    )
                })?;
            file[at..at + bytes.len()].copy_from_slice(bytes);
        } else {
            let at = w.write.address.wrapping_sub(cave_base) as usize;
            cave.get_mut(at..at + bytes.len())
                .ok_or_else(|| format!("{}: outside the fix section", w.write.fix))?
                .copy_from_slice(bytes);
        }
    }
    if cave_size == 0 {
        return Ok((file, writes));
    }
    let (file, section) =
        edit::add_section(&file, ".rea", &cave, edit::SCN_CODE_RWX).map_err(err)?;
    debug_assert_eq!(section.rva, cave_rva);
    let file = match hooks {
        Some(h) => edit::add_import(&file, ".reaimp", h.dll, h.exports, Some(cave_rva + h.table))
            .map_err(err)?,
        None => file,
    };
    Ok((file, writes))
}

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
    fn every_fix_is_written_into_the_file_and_the_hooks_into_its_imports() {
        use rea_launcher::pe::edit;
        let file = synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let hooks = Hooks {
            dll: "cmrhooks.dll",
            table: 0x10,
            exports: &["cmr_a", "cmr_b"],
        };
        let (patched, writes) = patched_exe(&img, 0x40, Some(hooks), |cave| {
            prepare(&img, &TEST_FIXES, &[], cave)
        })
        .unwrap();
        let pe = rea_launcher::pe::Pe::parse(&patched).unwrap();
        let rea = pe.section(".rea").unwrap();
        // The cave lands right after .text, at a fixed address.
        assert_eq!(rea.rva, 0x2000);
        assert_eq!(writes[1].write.address, 0x40_2000);
        // The image write, in the file.
        let at = pe.va_to_offset(0x40_1002).unwrap();
        assert_eq!(patched[at], 0xc3);
        // The cave's jmp back to 0x401000: e9 rel32 from 0x402005.
        let cave = pe.va_to_offset(0x40_2000).unwrap();
        assert_eq!(patched[cave], 0xe9);
        let rel = i32::from_le_bytes(patched[cave + 1..cave + 5].try_into().unwrap());
        assert_eq!(0x40_2005i64 + i64::from(rel), 0x40_1000);
        // The hook library, its address table on the cave's table.
        let imports = edit::imports(&patched).unwrap();
        let hook = imports.iter().find(|i| i.dll == "cmrhooks.dll").unwrap();
        assert_eq!(hook.functions, ["cmr_a", "cmr_b"]);
        assert_eq!(hook.iat_rva, 0x2010);
        // The player's bytes are never touched: only the copy changes.
        assert_eq!(file[pe.va_to_offset(0x40_1002).unwrap()], 0xec);
    }

    #[test]
    fn no_cave_means_no_section_and_no_hooks() {
        let file = synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let only_image = FixSet {
            shared: &[],
            own: &[Fix {
                id: "T2",
                what: "test",
                optional: false,
                code: &[Piece {
                    at: At::Image(0x401002),
                    ops: &[Op::Raw(&[0xc3])],
                }],
            }],
        };
        let (patched, _) = patched_exe(&img, 0, None, |cave| {
            assert_eq!(cave, 0);
            prepare(&img, &only_image, &[], cave)
        })
        .unwrap();
        let pe = rea_launcher::pe::Pe::parse(&patched).unwrap();
        assert!(pe.section(".rea").is_none());
        assert_eq!(patched.len(), file.len());
        assert_eq!(patched[pe.va_to_offset(0x40_1002).unwrap()], 0xc3);
    }

    #[test]
    fn a_write_that_no_longer_matches_the_file_is_refused() {
        let file = synthetic(&[0x55, 0x8b, 0xec, 0x90]);
        let img = Image::parse(&file).unwrap();
        let result = patched_exe(&img, 0x40, None, |cave| {
            let mut writes = prepare(&img, &TEST_FIXES, &[], cave)?;
            writes[0].original = vec![0x00];
            Ok(writes)
        });
        assert!(result.unwrap_err().contains("doesn't match"));
    }

    #[test]
    fn prepare_refuses_sites_outside_code() {
        let file = synthetic(&[0x90]);
        let img = Image::parse(&file).unwrap();
        assert!(prepare(&img, &TEST_FIXES, &[], 0x500000).is_err());
    }

    #[test]
    fn detaches_static_dplayx_import_and_sets_fallback_stubs() {
        let mut file = vec![0u8; 0x000e_9360];
        file[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        file[0x80 + 0x80..0x80 + 0x84].copy_from_slice(&0x000e_8fa4u32.to_le_bytes());
        file[0x80 + 0x84..0x80 + 0x88].copy_from_slice(&0xdcu32.to_le_bytes());
        file[0x000e_8fa4 + 12..0x000e_8fa4 + 16].copy_from_slice(&0x000e_9348u32.to_le_bytes());
        file[0x000e_8fa4 + 16..0x000e_8fa4 + 20].copy_from_slice(&0x000e_602cu32.to_le_bytes());
        file[0x000e_9348..0x000e_9348 + 11].copy_from_slice(b"DPLAYX.dll\0");
        file[0x000b_8755..0x000b_8760].fill(0x90);

        assert!(detach_dplayx(&mut file));
        assert_eq!(
            u32::from_le_bytes(file[0x80 + 0x80..0x80 + 0x84].try_into().unwrap()),
            0x000e_8fb8
        );
        assert_eq!(
            u32::from_le_bytes(file[0x80 + 0x84..0x80 + 0x88].try_into().unwrap()),
            0xc8
        );
        assert_eq!(&file[0x000e_9348..0x000e_9348 + 11], &[0u8; 11]);
        assert_eq!(
            u32::from_le_bytes(file[0x000e_602c..0x000e_6030].try_into().unwrap()),
            0x004b_8755
        );
        assert_eq!(
            u32::from_le_bytes(file[0x000e_6030..0x000e_6034].try_into().unwrap()),
            0x004b_875a
        );
        assert_eq!(
            &file[0x000b_8755..0x000b_8760],
            &[
                0x31, 0xc0, 0xc2, 0x08, 0x00, 0x83, 0xc8, 0xff, 0xc2, 0x0c, 0x00
            ]
        );
        // Second call is a no-op.
        assert!(!detach_dplayx(&mut file));
    }
}
