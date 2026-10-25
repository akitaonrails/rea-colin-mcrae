//! Shared bounded reader for i386 PE32 images.
//!
//! This module deliberately implements only the image structure needed by the
//! reconstruction parsers: validated headers and unambiguous translation of a
//! complete virtual-address range into file-backed bytes.

const DOS_LFANEW_OFFSET: usize = 0x3c;
const COFF_HEADER_SIZE: usize = 20;
const SECTION_HEADER_SIZE: usize = 40;
const PE32_MAGIC: u16 = 0x10b;
const I386_MACHINE: u16 = 0x14c;
const IMAGE_SCN_CNT_CODE: u32 = 0x0000_0020;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Pe32Error {
    EmptyRange {
        virtual_address: u32,
    },
    Truncated {
        offset: usize,
        needed: usize,
        input_len: usize,
    },
    InvalidDosMagic {
        offset: usize,
    },
    InvalidPeSignature {
        offset: usize,
    },
    UnsupportedMachine {
        offset: usize,
        machine: u16,
    },
    NotPe32 {
        offset: usize,
        magic: u16,
    },
    OptionalHeaderTooShort {
        offset: usize,
        size: usize,
    },
    AddressBeforeImage {
        virtual_address: u32,
        image_base: u32,
    },
    UnmappedRange {
        virtual_address: u32,
        byte_len: usize,
    },
    AmbiguousRange {
        virtual_address: u32,
        first_section_offset: usize,
        second_section_offset: usize,
    },
    SectionNotExecutable {
        virtual_address: u32,
        section_offset: usize,
        characteristics: u32,
    },
    ArithmeticOverflow {
        offset: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Pe32MappedRange {
    pub(crate) file_offset: usize,
    pub(crate) section_offset: usize,
    pub(crate) characteristics: u32,
}

/// A validated i386 PE32 image with a borrowed, immutable byte source.
pub(crate) struct Pe32Image<'a> {
    input: &'a [u8],
    image_base: u32,
    section_table: usize,
    section_count: usize,
}

impl<'a> Pe32Image<'a> {
    pub(crate) fn parse(input: &'a [u8]) -> Result<Self, Pe32Error> {
        require(input, 0, 2)?;
        if &input[..2] != b"MZ" {
            return Err(Pe32Error::InvalidDosMagic { offset: 0 });
        }

        let pe_offset = usize::try_from(read_u32(input, DOS_LFANEW_OFFSET)?).map_err(|_| {
            Pe32Error::ArithmeticOverflow {
                offset: DOS_LFANEW_OFFSET,
            }
        })?;
        let pe_header_len = 4usize
            .checked_add(COFF_HEADER_SIZE)
            .ok_or(Pe32Error::ArithmeticOverflow { offset: pe_offset })?;
        require(input, pe_offset, pe_header_len)?;
        if &input[pe_offset..pe_offset + 4] != b"PE\0\0" {
            return Err(Pe32Error::InvalidPeSignature { offset: pe_offset });
        }

        let coff_offset = pe_offset
            .checked_add(4)
            .ok_or(Pe32Error::ArithmeticOverflow { offset: pe_offset })?;
        let machine = read_u16(input, coff_offset)?;
        if machine != I386_MACHINE {
            return Err(Pe32Error::UnsupportedMachine {
                offset: coff_offset,
                machine,
            });
        }
        let section_count = read_u16(input, checked_add(coff_offset, 2)?)? as usize;
        let optional_size = read_u16(input, checked_add(coff_offset, 16)?)? as usize;
        let optional_offset =
            coff_offset
                .checked_add(COFF_HEADER_SIZE)
                .ok_or(Pe32Error::ArithmeticOverflow {
                    offset: coff_offset,
                })?;
        require(input, optional_offset, optional_size)?;
        if optional_size < 32 {
            return Err(Pe32Error::OptionalHeaderTooShort {
                offset: optional_offset,
                size: optional_size,
            });
        }
        let optional_magic = read_u16(input, optional_offset)?;
        if optional_magic != PE32_MAGIC {
            return Err(Pe32Error::NotPe32 {
                offset: optional_offset,
                magic: optional_magic,
            });
        }
        let image_base = read_u32(input, checked_add(optional_offset, 28)?)?;
        let section_table =
            optional_offset
                .checked_add(optional_size)
                .ok_or(Pe32Error::ArithmeticOverflow {
                    offset: optional_offset,
                })?;
        let section_bytes = section_count.checked_mul(SECTION_HEADER_SIZE).ok_or(
            Pe32Error::ArithmeticOverflow {
                offset: section_table,
            },
        )?;
        require(input, section_table, section_bytes)?;

        Ok(Self {
            input,
            image_base,
            section_table,
            section_count,
        })
    }

    pub(crate) const fn image_base(&self) -> u32 {
        self.image_base
    }

    /// Resolve a complete virtual-address range and retain its section
    /// provenance for callers that need to validate section characteristics.
    pub(crate) fn map_va_range(
        &self,
        virtual_address: u32,
        byte_len: usize,
    ) -> Result<Pe32MappedRange, Pe32Error> {
        if byte_len == 0 {
            return Err(Pe32Error::EmptyRange { virtual_address });
        }
        let target_rva =
            virtual_address
                .checked_sub(self.image_base)
                .ok_or(Pe32Error::AddressBeforeImage {
                    virtual_address,
                    image_base: self.image_base,
                })?;
        let byte_len_u32 = u32::try_from(byte_len).map_err(|_| Pe32Error::ArithmeticOverflow {
            offset: self.section_table,
        })?;
        let _range_end =
            target_rva
                .checked_add(byte_len_u32)
                .ok_or(Pe32Error::ArithmeticOverflow {
                    offset: virtual_address as usize,
                })?;

        let mut matched: Option<Pe32MappedRange> = None;
        for index in 0..self.section_count {
            let header_delta =
                index
                    .checked_mul(SECTION_HEADER_SIZE)
                    .ok_or(Pe32Error::ArithmeticOverflow {
                        offset: self.section_table,
                    })?;
            let header = self.section_table.checked_add(header_delta).ok_or(
                Pe32Error::ArithmeticOverflow {
                    offset: self.section_table,
                },
            )?;
            let virtual_size = read_u32(self.input, checked_add(header, 8)?)?;
            let section_rva = read_u32(self.input, checked_add(header, 12)?)?;
            let raw_size = read_u32(self.input, checked_add(header, 16)?)?;
            let raw_offset_field = checked_add(header, 20)?;
            let raw_offset = read_u32(self.input, raw_offset_field)?;
            let characteristics = read_u32(self.input, checked_add(header, 36)?)?;

            let Some(delta) = target_rva.checked_sub(section_rva) else {
                continue;
            };
            let Some(section_range_end) = delta.checked_add(byte_len_u32) else {
                continue;
            };
            // A zero VirtualSize uses SizeOfRawData. Otherwise a requested
            // range must fit both the mapped virtual extent and raw backing.
            let mapped_size = if virtual_size == 0 {
                raw_size
            } else {
                virtual_size
            };
            if section_range_end > mapped_size || section_range_end > raw_size {
                continue;
            }

            let raw_offset =
                usize::try_from(raw_offset).map_err(|_| Pe32Error::ArithmeticOverflow {
                    offset: raw_offset_field,
                })?;
            let delta = usize::try_from(delta).map_err(|_| Pe32Error::ArithmeticOverflow {
                offset: raw_offset_field,
            })?;
            let file_offset =
                raw_offset
                    .checked_add(delta)
                    .ok_or(Pe32Error::ArithmeticOverflow {
                        offset: raw_offset_field,
                    })?;
            if let Some(first) = matched {
                return Err(Pe32Error::AmbiguousRange {
                    virtual_address,
                    first_section_offset: first.section_offset,
                    second_section_offset: header,
                });
            }
            matched = Some(Pe32MappedRange {
                file_offset,
                section_offset: header,
                characteristics,
            });
        }

        let mapped = matched.ok_or(Pe32Error::UnmappedRange {
            virtual_address,
            byte_len,
        })?;
        require(self.input, mapped.file_offset, byte_len)?;
        Ok(mapped)
    }

    /// Borrow a complete uniquely mapped virtual-address range.
    pub(crate) fn read_va_range(
        &self,
        virtual_address: u32,
        byte_len: usize,
    ) -> Result<&'a [u8], Pe32Error> {
        self.read_va_range_with_permissions(virtual_address, byte_len, false)
    }

    /// Borrow a complete range only when its unique backing section is both
    /// marked as code and executable.
    #[allow(dead_code)] // Reserved for the compiled-initializer parser.
    pub(crate) fn read_code_va_range(
        &self,
        virtual_address: u32,
        byte_len: usize,
    ) -> Result<&'a [u8], Pe32Error> {
        self.read_va_range_with_permissions(virtual_address, byte_len, true)
    }

    fn read_va_range_with_permissions(
        &self,
        virtual_address: u32,
        byte_len: usize,
        require_executable_code: bool,
    ) -> Result<&'a [u8], Pe32Error> {
        let mapped = self.map_va_range(virtual_address, byte_len)?;
        let required = IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE;
        if require_executable_code && mapped.characteristics & required != required {
            return Err(Pe32Error::SectionNotExecutable {
                virtual_address,
                section_offset: mapped.section_offset,
                characteristics: mapped.characteristics,
            });
        }
        let end =
            mapped
                .file_offset
                .checked_add(byte_len)
                .ok_or(Pe32Error::ArithmeticOverflow {
                    offset: mapped.file_offset,
                })?;
        Ok(&self.input[mapped.file_offset..end])
    }
}

fn checked_add(offset: usize, addend: usize) -> Result<usize, Pe32Error> {
    offset
        .checked_add(addend)
        .ok_or(Pe32Error::ArithmeticOverflow { offset })
}

fn require(input: &[u8], offset: usize, needed: usize) -> Result<(), Pe32Error> {
    let end = offset
        .checked_add(needed)
        .ok_or(Pe32Error::ArithmeticOverflow { offset })?;
    if end > input.len() {
        return Err(Pe32Error::Truncated {
            offset,
            needed,
            input_len: input.len(),
        });
    }
    Ok(())
}

fn read_u16(input: &[u8], offset: usize) -> Result<u16, Pe32Error> {
    require(input, offset, 2)?;
    Ok(u16::from_le_bytes([input[offset], input[offset + 1]]))
}

fn read_u32(input: &[u8], offset: usize) -> Result<u32, Pe32Error> {
    require(input, offset, 4)?;
    Ok(u32::from_le_bytes([
        input[offset],
        input[offset + 1],
        input[offset + 2],
        input[offset + 3],
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PE_OFFSET: usize = 0x80;
    const OPTIONAL_SIZE: usize = 0xe0;
    const SECTION_TABLE: usize = PE_OFFSET + 4 + COFF_HEADER_SIZE + OPTIONAL_SIZE;
    const RAW_OFFSET: usize = 0x200;
    const IMAGE_BASE: u32 = 0x0040_0000;
    const SECTION_RVA: u32 = 0x1000;

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn synthetic_pe() -> Vec<u8> {
        let mut bytes = vec![0; RAW_OFFSET + 0x100];
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, DOS_LFANEW_OFFSET, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        let coff = PE_OFFSET + 4;
        put_u16(&mut bytes, coff, I386_MACHINE);
        put_u16(&mut bytes, coff + 2, 1);
        put_u16(&mut bytes, coff + 16, OPTIONAL_SIZE as u16);
        let optional = coff + COFF_HEADER_SIZE;
        put_u16(&mut bytes, optional, PE32_MAGIC);
        put_u32(&mut bytes, optional + 28, IMAGE_BASE);
        put_u32(&mut bytes, SECTION_TABLE + 8, 0x100);
        put_u32(&mut bytes, SECTION_TABLE + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTION_TABLE + 16, 0x100);
        put_u32(&mut bytes, SECTION_TABLE + 20, RAW_OFFSET as u32);
        put_u32(
            &mut bytes,
            SECTION_TABLE + 36,
            IMAGE_SCN_CNT_CODE | IMAGE_SCN_MEM_EXECUTE,
        );
        bytes[RAW_OFFSET + 0x30..RAW_OFFSET + 0x34].copy_from_slice(&[1, 2, 3, 4]);
        bytes
    }

    fn set_extents(bytes: &mut [u8], virtual_size: u32, raw_size: u32) {
        put_u32(bytes, SECTION_TABLE + 8, virtual_size);
        put_u32(bytes, SECTION_TABLE + 16, raw_size);
    }

    fn set_characteristics(bytes: &mut [u8], characteristics: u32) {
        put_u32(bytes, SECTION_TABLE + 36, characteristics);
    }

    #[test]
    fn maps_and_borrows_an_arbitrary_complete_va_range() {
        let bytes = synthetic_pe();
        let image = Pe32Image::parse(&bytes).unwrap();
        let address = IMAGE_BASE + SECTION_RVA + 0x30;
        assert_eq!(
            image.map_va_range(address, 4).unwrap().file_offset,
            RAW_OFFSET + 0x30
        );
        assert_eq!(image.read_va_range(address, 4).unwrap(), &[1, 2, 3, 4]);
    }

    #[test]
    fn accepts_ranges_that_exactly_fit_both_section_extents() {
        let bytes = synthetic_pe();
        let image = Pe32Image::parse(&bytes).unwrap();
        let address = IMAGE_BASE + SECTION_RVA;
        assert_eq!(
            image.map_va_range(address, 0x100).unwrap().file_offset,
            RAW_OFFSET
        );
        assert_eq!(image.read_va_range(address, 0x100).unwrap().len(), 0x100);
    }

    #[test]
    fn rejects_virtual_and_raw_section_tails_independently() {
        let address = IMAGE_BASE + SECTION_RVA + 0x70;

        let mut virtual_tail = synthetic_pe();
        set_extents(&mut virtual_tail, 0x80, 0x100);
        let image = Pe32Image::parse(&virtual_tail).unwrap();
        assert_eq!(
            image.read_va_range(address, 0x11),
            Err(Pe32Error::UnmappedRange {
                virtual_address: address,
                byte_len: 0x11,
            })
        );

        let mut raw_tail = synthetic_pe();
        set_extents(&mut raw_tail, 0x100, 0x80);
        let image = Pe32Image::parse(&raw_tail).unwrap();
        assert_eq!(
            image.read_va_range(address, 0x11),
            Err(Pe32Error::UnmappedRange {
                virtual_address: address,
                byte_len: 0x11,
            })
        );
    }

    #[test]
    fn zero_virtual_size_uses_the_raw_extent() {
        let mut bytes = synthetic_pe();
        set_extents(&mut bytes, 0, 0x100);
        let image = Pe32Image::parse(&bytes).unwrap();
        let address = IMAGE_BASE + SECTION_RVA;
        assert_eq!(image.read_va_range(address, 0x100).unwrap().len(), 0x100);
    }

    #[test]
    fn ambiguity_precedes_code_permission_checks() {
        let mut bytes = synthetic_pe();
        put_u16(&mut bytes, PE_OFFSET + 4 + 2, 2);
        let second = SECTION_TABLE + SECTION_HEADER_SIZE;
        bytes.copy_within(SECTION_TABLE..SECTION_TABLE + SECTION_HEADER_SIZE, second);
        put_u32(&mut bytes, second + 36, 0);
        let image = Pe32Image::parse(&bytes).unwrap();
        let address = IMAGE_BASE + SECTION_RVA + 0x30;
        assert_eq!(
            image.read_code_va_range(address, 4),
            Err(Pe32Error::AmbiguousRange {
                virtual_address: address,
                first_section_offset: SECTION_TABLE,
                second_section_offset: second,
            })
        );
    }

    #[test]
    fn reports_truncated_backing_after_a_unique_mapping() {
        let mut bytes = synthetic_pe();
        bytes.truncate(RAW_OFFSET + 0xff);
        let image = Pe32Image::parse(&bytes).unwrap();
        let address = IMAGE_BASE + SECTION_RVA;
        assert_eq!(
            image.read_va_range(address, 0x100),
            Err(Pe32Error::Truncated {
                offset: RAW_OFFSET,
                needed: 0x100,
                input_len: RAW_OFFSET + 0xff,
            })
        );
    }

    #[test]
    fn rejects_before_image_empty_and_overflowing_ranges() {
        let bytes = synthetic_pe();
        let image = Pe32Image::parse(&bytes).unwrap();
        assert_eq!(
            image.read_va_range(IMAGE_BASE - 1, 1),
            Err(Pe32Error::AddressBeforeImage {
                virtual_address: IMAGE_BASE - 1,
                image_base: IMAGE_BASE,
            })
        );
        let address = IMAGE_BASE + SECTION_RVA;
        assert_eq!(
            image.read_va_range(address, 0),
            Err(Pe32Error::EmptyRange {
                virtual_address: address,
            })
        );
        assert_eq!(
            image.read_va_range(u32::MAX, IMAGE_BASE as usize + 2),
            Err(Pe32Error::ArithmeticOverflow {
                offset: u32::MAX as usize,
            })
        );

        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            image.read_va_range(address, u32::MAX as usize + 1),
            Err(Pe32Error::ArithmeticOverflow {
                offset: SECTION_TABLE,
            })
        );
    }

    #[test]
    fn code_reads_require_both_code_and_execute_characteristics() {
        let address = IMAGE_BASE + SECTION_RVA + 0x30;
        let bytes = synthetic_pe();
        let image = Pe32Image::parse(&bytes).unwrap();
        assert_eq!(image.read_code_va_range(address, 4).unwrap(), &[1, 2, 3, 4]);

        for characteristics in [0, IMAGE_SCN_CNT_CODE, IMAGE_SCN_MEM_EXECUTE] {
            let mut bytes = synthetic_pe();
            set_characteristics(&mut bytes, characteristics);
            let image = Pe32Image::parse(&bytes).unwrap();
            assert_eq!(
                image.read_code_va_range(address, 4),
                Err(Pe32Error::SectionNotExecutable {
                    virtual_address: address,
                    section_offset: SECTION_TABLE,
                    characteristics,
                })
            );
        }
    }
}
