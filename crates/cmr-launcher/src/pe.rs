//! Just enough of the PE format to map addresses to file bytes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub va: u32,
    pub virtual_size: u32,
    pub raw_offset: u32,
    pub raw_size: u32,
    pub executable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image<'a> {
    pub file: &'a [u8],
    pub image_base: u32,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeError {
    NotPe,
    Not32Bit,
    Truncated,
}

impl std::fmt::Display for PeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            PeError::NotPe => "not a Windows executable",
            PeError::Not32Bit => "not a 32-bit Windows executable",
            PeError::Truncated => "the executable is truncated",
        })
    }
}

fn u16_at(b: &[u8], off: usize) -> Result<u16, PeError> {
    b.get(off..off + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or(PeError::Truncated)
}

fn u32_at(b: &[u8], off: usize) -> Result<u32, PeError> {
    b.get(off..off + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or(PeError::Truncated)
}

const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;

impl<'a> Image<'a> {
    pub fn parse(file: &'a [u8]) -> Result<Self, PeError> {
        if file.get(0..2) != Some(b"MZ") {
            return Err(PeError::NotPe);
        }
        let pe = u32_at(file, 0x3c)? as usize;
        if file.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(PeError::NotPe);
        }
        let sections = u16_at(file, pe + 6)? as usize;
        let optional_size = u16_at(file, pe + 20)? as usize;
        let optional = pe + 24;
        if u16_at(file, optional)? != 0x10b {
            return Err(PeError::Not32Bit);
        }
        let image_base = u32_at(file, optional + 28)?;
        let table = optional + optional_size;
        let sections = (0..sections)
            .map(|i| {
                let s = table + i * 40;
                Ok(Section {
                    virtual_size: u32_at(file, s + 8)?,
                    va: image_base.wrapping_add(u32_at(file, s + 12)?),
                    raw_size: u32_at(file, s + 16)?,
                    raw_offset: u32_at(file, s + 20)?,
                    executable: u32_at(file, s + 36)? & IMAGE_SCN_MEM_EXECUTE != 0,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Image {
            file,
            image_base,
            sections,
        })
    }

    /// The file bytes that load at `[address, address + len)`, if that whole
    /// range is backed by the file inside one executable section.
    pub fn code_at(&self, address: u32, len: usize) -> Option<&'a [u8]> {
        let s = self.sections.iter().find(|s| {
            s.executable
                && address >= s.va
                && (address - s.va) as usize + len <= s.raw_size.min(s.virtual_size) as usize
        })?;
        let off = s.raw_offset as usize + (address - s.va) as usize;
        self.file.get(off..off + len)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny synthetic PE32 with one executable section at 0x401000 whose
    /// bytes are `code`.
    pub(crate) fn synthetic(code: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x200];
        f[0..2].copy_from_slice(b"MZ");
        f[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        f[0x40..0x44].copy_from_slice(b"PE\0\0");
        f[0x46..0x48].copy_from_slice(&1u16.to_le_bytes()); // sections
        f[0x54..0x56].copy_from_slice(&0xe0u16.to_le_bytes()); // optional size
        let opt = 0x58;
        f[opt..opt + 2].copy_from_slice(&0x10bu16.to_le_bytes());
        f[opt + 28..opt + 32].copy_from_slice(&0x40_0000u32.to_le_bytes());
        let s = opt + 0xe0;
        f[s..s + 5].copy_from_slice(b".text");
        f[s + 8..s + 12].copy_from_slice(&(code.len() as u32).to_le_bytes());
        f[s + 12..s + 16].copy_from_slice(&0x1000u32.to_le_bytes());
        f[s + 16..s + 20].copy_from_slice(&(code.len() as u32).to_le_bytes());
        f[s + 20..s + 24].copy_from_slice(&0x200u32.to_le_bytes());
        f[s + 36..s + 40].copy_from_slice(&0x6000_0020u32.to_le_bytes());
        f.extend_from_slice(code);
        f
    }

    #[test]
    fn maps_code_addresses() {
        let file = synthetic(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let img = Image::parse(&file).unwrap();
        assert_eq!(img.image_base, 0x400000);
        assert_eq!(img.code_at(0x401002, 3), Some(&[3, 4, 5][..]));
        assert_eq!(img.code_at(0x401006, 3), None, "runs past the section");
        assert_eq!(img.code_at(0x400fff, 1), None, "before the section");
    }

    #[test]
    fn rejects_non_pe() {
        assert_eq!(Image::parse(b"hello").unwrap_err(), PeError::NotPe);
        let mut file = synthetic(&[0]);
        file[0x58] = 0x0b;
        file[0x59] = 0x02; // PE32+
        assert_eq!(Image::parse(&file).unwrap_err(), PeError::Not32Bit);
    }
}
