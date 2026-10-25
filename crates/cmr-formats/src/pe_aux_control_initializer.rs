//! Pure extraction and canonical storage of auxiliary compiled-control words.
//!
//! The decoder accepts only the complete reviewed i386 instruction shape.
//! Artifact identity, filesystem access, and publication belong to higher
//! layers; no shipped tuning values are compiled into this module.

use std::{error::Error, fmt};

use crate::pe32::{Pe32Error, Pe32Image};

pub const AUX_CONTROL_COMMON_WORD_COUNT: usize = 9;
pub const AUX_CONTROL_MULTIPLIER_COUNT: usize = 2;
pub const AUX_CONTROL_POLICY_COUNT: usize = 3;
pub const AUX_CONTROL_POLICY_WORD_COUNT: usize = 7;
pub const AUX_CONTROL_POLICY_DIVISOR_COUNT: usize = 2;
pub const AUX_CONTROL_TOTAL_WORD_COUNT: usize = AUX_CONTROL_COMMON_WORD_COUNT
    + AUX_CONTROL_MULTIPLIER_COUNT
    + AUX_CONTROL_POLICY_COUNT * (AUX_CONTROL_POLICY_WORD_COUNT + AUX_CONTROL_POLICY_DIVISOR_COUNT);

pub const AUX_CONTROL_MAGIC: [u8; 8] = *b"CMRAUXC\0";
pub const AUX_CONTROL_FORMAT_VERSION: u32 = 1;
pub const AUX_CONTROL_ENCODED_LEN: usize = 32 + AUX_CONTROL_TOTAL_WORD_COUNT * 4;

pub const AUX_CONTROL_WORD_OFFSETS: [u16; 22] = [
    0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0ec, 0x0f0, 0x0f4, 0x0f8, 0x0fc, 0x100, 0x104, 0x108,
    0x10c, 0x110, 0x118, 0x11c, 0x120, 0x124, 0x128, 0x12c, 0x13c,
];
pub const AUX_CONTROL_COMMON_WORD_OFFSETS: [u16; AUX_CONTROL_COMMON_WORD_COUNT] = [
    0x0d8, 0x0dc, 0x0e0, 0x0e4, 0x0e8, 0x0ec, 0x0f0, 0x0fc, 0x104,
];
pub const AUX_CONTROL_POLICY_WORD_OFFSETS: [u16; AUX_CONTROL_POLICY_WORD_COUNT] =
    [0x0f8, 0x10c, 0x110, 0x120, 0x124, 0x128, 0x13c];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxControlPolicyWords {
    pub words: [i32; AUX_CONTROL_POLICY_WORD_COUNT],
    pub reciprocal_divisor: i32,
    pub scaled_reciprocal_divisor: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxControlConstantWords {
    pub common_words: [i32; AUX_CONTROL_COMMON_WORD_COUNT],
    pub q16_multipliers: [i32; AUX_CONTROL_MULTIPLIER_COUNT],
    pub policy_one: AuxControlPolicyWords,
    pub policy_two: AuxControlPolicyWords,
    pub default_policy: AuxControlPolicyWords,
}

/// Address-only description of one reviewed function body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxControlInitializerLayout {
    pub image_base: u32,
    pub function_start_va: u32,
    pub function_end_va: u32,
    pub lookup_va: u32,
    pub select_va: u32,
    pub default_policy_va: u32,
    pub policy_two_va: u32,
    pub policy_one_va: u32,
    pub common_va: u32,
    pub first_divide_va: u32,
    pub first_divide_join_va: u32,
    pub second_divide_va: u32,
    pub second_divide_join_va: u32,
    pub third_divide_va: u32,
    pub third_divide_join_va: u32,
    pub config_pointer_va: u32,
    pub state_pointer_va: u32,
    pub control_pointer_va: u32,
    pub global_word_va: u32,
    pub config_flag_offset: u16,
    pub state_index_offset: u16,
    pub config_option_offset: u16,
    pub config_record_stride: u16,
    pub state_word_offset: u16,
    pub output_offsets: [u16; 22],
}

pub const OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT: AuxControlInitializerLayout =
    AuxControlInitializerLayout {
        image_base: 0x0040_0000,
        function_start_va: 0x0040_7270,
        function_end_va: 0x0040_758f,
        lookup_va: 0x0040_7286,
        select_va: 0x0040_72a4,
        default_policy_va: 0x0040_72c4,
        policy_two_va: 0x0040_7331,
        policy_one_va: 0x0040_739c,
        common_va: 0x0040_740e,
        first_divide_va: 0x0040_74f2,
        first_divide_join_va: 0x0040_7508,
        second_divide_va: 0x0040_7520,
        second_divide_join_va: 0x0040_7536,
        third_divide_va: 0x0040_7557,
        third_divide_join_va: 0x0040_7570,
        config_pointer_va: 0x00d3_ca80,
        state_pointer_va: 0x0050_c5b4,
        control_pointer_va: 0x0050_c604,
        global_word_va: 0x004e_ac6c,
        config_flag_offset: 0x026,
        state_index_offset: 0x738,
        config_option_offset: 0x093,
        config_record_stride: 0x09c,
        state_word_offset: 0x048,
        output_offsets: AUX_CONTROL_WORD_OFFSETS,
    };

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuxControlInitializerError {
    Pe(AuxControlPeError),
    ImageBase { expected: u32, actual: u32 },
    Layout(&'static str),
    Shape { address: u32, detail: &'static str },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuxControlPeError {
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

impl From<Pe32Error> for AuxControlInitializerError {
    fn from(error: Pe32Error) -> Self {
        Self::Pe(match error {
            Pe32Error::EmptyRange { virtual_address } => {
                AuxControlPeError::EmptyRange { virtual_address }
            }
            Pe32Error::Truncated {
                offset,
                needed,
                input_len,
            } => AuxControlPeError::Truncated {
                offset,
                needed,
                input_len,
            },
            Pe32Error::InvalidDosMagic { offset } => AuxControlPeError::InvalidDosMagic { offset },
            Pe32Error::InvalidPeSignature { offset } => {
                AuxControlPeError::InvalidPeSignature { offset }
            }
            Pe32Error::UnsupportedMachine { offset, machine } => {
                AuxControlPeError::UnsupportedMachine { offset, machine }
            }
            Pe32Error::NotPe32 { offset, magic } => AuxControlPeError::NotPe32 { offset, magic },
            Pe32Error::OptionalHeaderTooShort { offset, size } => {
                AuxControlPeError::OptionalHeaderTooShort { offset, size }
            }
            Pe32Error::AddressBeforeImage {
                virtual_address,
                image_base,
            } => AuxControlPeError::AddressBeforeImage {
                virtual_address,
                image_base,
            },
            Pe32Error::UnmappedRange {
                virtual_address,
                byte_len,
            } => AuxControlPeError::UnmappedRange {
                virtual_address,
                byte_len,
            },
            Pe32Error::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            } => AuxControlPeError::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            },
            Pe32Error::SectionNotExecutable {
                virtual_address,
                section_offset,
                characteristics,
            } => AuxControlPeError::SectionNotExecutable {
                virtual_address,
                section_offset,
                characteristics,
            },
            Pe32Error::ArithmeticOverflow { offset } => {
                AuxControlPeError::ArithmeticOverflow { offset }
            }
        })
    }
}

impl fmt::Display for AuxControlInitializerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid auxiliary-control initializer: {self:?}")
    }
}

impl Error for AuxControlInitializerError {}

const LITERAL_COUNT: usize = 30;

#[repr(usize)]
#[derive(Clone, Copy)]
enum Literal {
    SeedEdx,
    SeedEsi,
    SeedEcx,
    DefaultDivisorB,
    DefaultDivisorA,
    Default10c,
    Default110,
    Default120,
    Default124,
    Default128,
    PolicyTwoDivisorB,
    PolicyTwo13c,
    PolicyTwo10c,
    PolicyTwo110,
    PolicyTwo120,
    PolicyTwo124,
    PolicyTwo128,
    PolicyOneDivisorB,
    PolicyOneF8,
    PolicyOne13c,
    PolicyOne10c,
    PolicyOne110,
    PolicyOne120,
    PolicyOne124,
    PolicyOne128,
    CommonD8,
    CommonEc,
    Common104,
    MultiplierF4,
    Multiplier100,
}

#[repr(u8)]
#[derive(Clone, Copy)]
enum Register {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
}

struct SemanticCursor<'a> {
    body: &'a [u8],
    start: u32,
    cursor: usize,
    branch_targets: Vec<u32>,
}

impl<'a> SemanticCursor<'a> {
    fn new(body: &'a [u8], start: u32) -> Self {
        Self {
            body,
            start,
            cursor: 0,
            branch_targets: Vec::new(),
        }
    }

    fn address(&self) -> u32 {
        self.start + self.cursor as u32
    }

    fn fail<T>(&self, detail: &'static str) -> Result<T, AuxControlInitializerError> {
        Err(AuxControlInitializerError::Shape {
            address: self.address(),
            detail,
        })
    }

    fn bytes(&mut self, count: usize) -> Result<&'a [u8], AuxControlInitializerError> {
        let end = self
            .cursor
            .checked_add(count)
            .ok_or(AuxControlInitializerError::Shape {
                address: self.address(),
                detail: "instruction length overflow",
            })?;
        let bytes = self
            .body
            .get(self.cursor..end)
            .ok_or(AuxControlInitializerError::Shape {
                address: self.address(),
                detail: "truncated instruction",
            })?;
        self.cursor = end;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, AuxControlInitializerError> {
        Ok(self.bytes(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, AuxControlInitializerError> {
        Ok(u32::from_le_bytes(
            self.bytes(4)?.try_into().expect("four bytes"),
        ))
    }
    fn i32(&mut self) -> Result<i32, AuxControlInitializerError> {
        Ok(self.u32()? as i32)
    }
    fn opcode(&mut self, expected: u8) -> Result<(), AuxControlInitializerError> {
        if self.byte()? != expected {
            return self.fail("wrong opcode");
        }
        Ok(())
    }
    fn modrm(
        &mut self,
        mode: u8,
        reg: Register,
        rm: Register,
    ) -> Result<(), AuxControlInitializerError> {
        let expected = (mode << 6) | ((reg as u8) << 3) | rm as u8;
        if self.byte()? != expected {
            return self.fail("wrong register or addressing form");
        }
        Ok(())
    }
    fn at(&self, expected: u32) -> Result<(), AuxControlInitializerError> {
        if self.address() != expected {
            return self.fail("wrong basic-block boundary");
        }
        Ok(())
    }
    fn push(&mut self, register: Register) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x50 + register as u8)
    }
    fn pop(&mut self, register: Register) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x58 + register as u8)
    }
    fn mov_reg(&mut self, dst: Register, src: Register) -> Result<(), AuxControlInitializerError> {
        match self.byte()? {
            0x8b => self.modrm(3, dst, src),
            0x89 => self.modrm(3, src, dst),
            _ => self.fail("wrong register move opcode"),
        }
    }
    fn mov_imm(&mut self, dst: Register) -> Result<i32, AuxControlInitializerError> {
        self.opcode(0xb8 + dst as u8)?;
        self.i32()
    }
    fn mov_abs(&mut self, dst: Register, address: u32) -> Result<(), AuxControlInitializerError> {
        if matches!(dst, Register::Eax) {
            self.opcode(0xa1)?;
        } else {
            self.opcode(0x8b)?;
            self.modrm(0, dst, Register::Ebp)?;
        }
        if self.u32()? != address {
            return self.fail("wrong absolute source");
        }
        Ok(())
    }
    fn mov_load(
        &mut self,
        dst: Register,
        base: Register,
        displacement: i32,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x8b)?;
        if (-128..=127).contains(&displacement) {
            self.modrm(1, dst, base)?;
            if self.byte()? as i8 as i32 != displacement {
                return self.fail("wrong load displacement");
            }
        } else {
            self.modrm(2, dst, base)?;
            if self.i32()? != displacement {
                return self.fail("wrong load displacement");
            }
        }
        Ok(())
    }
    fn mov_store(
        &mut self,
        base: Register,
        displacement: i32,
        src: Register,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x89)?;
        if (-128..=127).contains(&displacement) {
            self.modrm(1, src, base)?;
            if self.byte()? as i8 as i32 != displacement {
                return self.fail("wrong store destination");
            }
        } else {
            self.modrm(2, src, base)?;
            if self.i32()? != displacement {
                return self.fail("wrong store destination");
            }
        }
        Ok(())
    }
    fn mov_store_imm(
        &mut self,
        base: Register,
        displacement: i32,
    ) -> Result<i32, AuxControlInitializerError> {
        self.opcode(0xc7)?;
        self.modrm(
            if (-128..=127).contains(&displacement) {
                1
            } else {
                2
            },
            Register::Eax,
            base,
        )?;
        if (-128..=127).contains(&displacement) {
            if self.byte()? as i8 as i32 != displacement {
                return self.fail("wrong store destination");
            }
        } else if self.i32()? != displacement {
            return self.fail("wrong store destination");
        }
        self.i32()
    }
    fn alu_imm8(
        &mut self,
        extension: u8,
        dst: Register,
        value: i8,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x83)?;
        if self.byte()? != 0xc0 | (extension << 3) | dst as u8 || self.byte()? as i8 != value {
            return self.fail("wrong immediate ALU instruction");
        }
        Ok(())
    }
    fn dec(&mut self, register: Register) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x48 + register as u8)
    }
    fn xor_reg(&mut self, dst: Register, src: Register) -> Result<(), AuxControlInitializerError> {
        match self.byte()? {
            0x33 => self.modrm(3, dst, src),
            0x31 => self.modrm(3, src, dst),
            _ => self.fail("wrong XOR opcode"),
        }
    }
    fn sub_reg(&mut self, dst: Register, src: Register) -> Result<(), AuxControlInitializerError> {
        match self.byte()? {
            0x2b => self.modrm(3, dst, src),
            0x29 => self.modrm(3, src, dst),
            _ => self.fail("wrong subtraction opcode"),
        }
    }
    fn test_reg(
        &mut self,
        left: Register,
        right: Register,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x85)?;
        self.modrm(3, right, left)
    }
    fn shift(
        &mut self,
        extension: u8,
        register: Register,
        amount: u8,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0xc1)?;
        if self.byte()? != 0xc0 | (extension << 3) | register as u8 || self.byte()? != amount {
            return self.fail("wrong shift instruction");
        }
        Ok(())
    }
    fn test_mem8(
        &mut self,
        base: Register,
        displacement: i8,
        mask: u8,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0xf6)?;
        self.modrm(1, Register::Eax, base)?;
        if self.byte()? as i8 != displacement || self.byte()? != mask {
            return self.fail("wrong byte test");
        }
        Ok(())
    }
    fn movsx8_load32(
        &mut self,
        dst: Register,
        base: Register,
        displacement: i32,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x0f)?;
        self.opcode(0xbe)?;
        self.modrm(2, dst, base)?;
        if self.i32()? != displacement {
            return self.fail("wrong sign-extended load");
        }
        Ok(())
    }
    fn lea_eax_times_five_into_edx(&mut self) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x8d)?;
        self.modrm(0, Register::Edx, Register::Esp)?;
        if self.byte()? != 0x80 {
            return self.fail("wrong indexed LEA");
        }
        Ok(())
    }
    fn mov_option_byte(&mut self, displacement: i32) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x8a)?;
        self.modrm(2, Register::Eax, Register::Esp)?;
        if self.byte()? != 0x91 || self.i32()? != displacement {
            return self.fail("wrong option lookup");
        }
        Ok(())
    }
    fn cdq(&mut self) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x99)
    }
    fn imul_eax(&mut self, source: Register) -> Result<(), AuxControlInitializerError> {
        self.opcode(0xf7)?;
        self.modrm(3, Register::Ebp, source)
    }
    fn idiv(&mut self, divisor: Register) -> Result<(), AuxControlInitializerError> {
        self.opcode(0xf7)?;
        self.modrm(3, Register::Edi, divisor)
    }
    fn shrd(
        &mut self,
        dst: Register,
        src: Register,
        amount: u8,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x0f)?;
        self.opcode(0xac)?;
        self.modrm(3, src, dst)?;
        if self.byte()? != amount {
            return self.fail("wrong SHRD amount");
        }
        Ok(())
    }
    fn shld(
        &mut self,
        dst: Register,
        src: Register,
        amount: u8,
    ) -> Result<(), AuxControlInitializerError> {
        self.opcode(0x0f)?;
        self.opcode(0xa4)?;
        self.modrm(3, src, dst)?;
        if self.byte()? != amount {
            return self.fail("wrong SHLD amount");
        }
        Ok(())
    }
    fn branch8(&mut self, opcode: u8, target: u32) -> Result<(), AuxControlInitializerError> {
        let start = self.address();
        self.opcode(opcode)?;
        let displacement = self.byte()? as i8 as i64;
        let actual = (i64::from(start) + 2 + displacement) as u32;
        if actual != target {
            return self.fail("wrong branch target");
        }
        self.branch_targets.push(actual);
        Ok(())
    }
    fn branch32(
        &mut self,
        prefix: Option<u8>,
        opcode: u8,
        target: u32,
    ) -> Result<(), AuxControlInitializerError> {
        let start = self.address();
        if let Some(prefix) = prefix {
            self.opcode(prefix)?;
        }
        self.opcode(opcode)?;
        let displacement = i64::from(self.i32()?);
        let length = if prefix.is_some() { 6 } else { 5 };
        let actual = (i64::from(start) + length + displacement) as u32;
        if actual != target {
            return self.fail("wrong branch target");
        }
        self.branch_targets.push(actual);
        Ok(())
    }
    fn done(&self) -> Result<(), AuxControlInitializerError> {
        if self.cursor != self.body.len() {
            return self.fail("unconsumed or extra instruction bytes");
        }
        for target in &self.branch_targets {
            if *target < self.start || *target >= self.start + self.body.len() as u32 {
                return self.fail("branch leaves reviewed function");
            }
        }
        Ok(())
    }
}

fn decode_semantic_body(
    body: &[u8],
    layout: &AuxControlInitializerLayout,
) -> Result<[i32; LITERAL_COUNT], AuxControlInitializerError> {
    let mut d = SemanticCursor::new(body, layout.function_start_va);
    let mut v = [0; LITERAL_COUNT];
    let mut set = |id: Literal, value: i32| v[id as usize] = value;

    d.push(Register::Ebp)?;
    d.mov_reg(Register::Ebp, Register::Esp)?;
    d.alu_imm8(5, Register::Esp, 0x10)?;
    d.mov_abs(Register::Ecx, layout.config_pointer_va)?;
    d.test_mem8(Register::Ecx, layout.config_flag_offset as i8, 2)?;
    d.branch8(0x74, layout.lookup_va)?;
    d.xor_reg(Register::Eax, Register::Eax)?;
    d.branch8(0xeb, layout.select_va)?;
    d.at(layout.lookup_va)?;
    d.mov_abs(Register::Eax, layout.state_pointer_va)?;
    d.movsx8_load32(
        Register::Eax,
        Register::Eax,
        i32::from(layout.state_index_offset),
    )?;
    d.lea_eax_times_five_into_edx()?;
    d.shift(4, Register::Edx, 3)?;
    d.sub_reg(Register::Edx, Register::Eax)?;
    d.mov_option_byte(i32::from(layout.config_option_offset))?;
    d.alu_imm8(4, Register::Eax, 7)?;

    d.at(layout.select_va)?;
    d.push(Register::Esi)?;
    d.dec(Register::Eax)?;
    d.push(Register::Edi)?;
    set(Literal::SeedEdx, d.mov_imm(Register::Edx)?);
    set(Literal::SeedEsi, d.mov_imm(Register::Esi)?);
    set(Literal::SeedEcx, d.mov_imm(Register::Ecx)?);
    d.branch32(Some(0x0f), 0x84, layout.policy_one_va)?;
    d.dec(Register::Eax)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.branch8(0x74, layout.policy_two_va)?;

    d.at(layout.default_policy_va)?;
    d.mov_store(Register::Eax, 0x0f8, Register::Edx)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    set(
        Literal::DefaultDivisorB,
        d.mov_store_imm(Register::Ebp, -4)?,
    );
    set(Literal::DefaultDivisorA, d.mov_imm(Register::Edi)?);
    d.mov_store(Register::Eax, 0x13c, Register::Edx)?;
    for (offset, id) in [
        (0x10c, Literal::Default10c),
        (0x110, Literal::Default110),
        (0x120, Literal::Default120),
        (0x124, Literal::Default124),
        (0x128, Literal::Default128),
    ] {
        d.mov_abs(Register::Eax, layout.control_pointer_va)?;
        set(id, d.mov_store_imm(Register::Eax, offset)?);
    }
    d.branch32(None, 0xe9, layout.common_va)?;

    d.at(layout.policy_two_va)?;
    d.mov_store(Register::Eax, 0x0f8, Register::Esi)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    set(
        Literal::PolicyTwoDivisorB,
        d.mov_store_imm(Register::Ebp, -4)?,
    );
    d.mov_reg(Register::Edi, Register::Ecx)?;
    set(
        Literal::PolicyTwo13c,
        d.mov_store_imm(Register::Eax, 0x13c)?,
    );
    for (offset, id) in [
        (0x10c, Literal::PolicyTwo10c),
        (0x110, Literal::PolicyTwo110),
        (0x120, Literal::PolicyTwo120),
        (0x124, Literal::PolicyTwo124),
        (0x128, Literal::PolicyTwo128),
    ] {
        d.mov_abs(Register::Eax, layout.control_pointer_va)?;
        set(id, d.mov_store_imm(Register::Eax, offset)?);
    }
    d.branch8(0xeb, layout.common_va)?;

    d.at(layout.policy_one_va)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    set(
        Literal::PolicyOneDivisorB,
        d.mov_store_imm(Register::Ebp, -4)?,
    );
    d.mov_reg(Register::Edi, Register::Edx)?;
    set(Literal::PolicyOneF8, d.mov_store_imm(Register::Eax, 0x0f8)?);
    for (offset, id) in [
        (0x13c, Literal::PolicyOne13c),
        (0x10c, Literal::PolicyOne10c),
        (0x110, Literal::PolicyOne110),
        (0x120, Literal::PolicyOne120),
        (0x124, Literal::PolicyOne124),
        (0x128, Literal::PolicyOne128),
    ] {
        d.mov_abs(Register::Eax, layout.control_pointer_va)?;
        set(id, d.mov_store_imm(Register::Eax, offset)?);
    }

    d.at(layout.common_va)?;
    d.push(Register::Ebx)?;
    d.mov_abs(Register::Ebx, layout.control_pointer_va)?;
    set(Literal::CommonD8, d.mov_imm(Register::Eax)?);
    d.mov_store(Register::Ebp, -12, Register::Edi)?;
    d.mov_store(Register::Ebx, 0x0d8, Register::Eax)?;
    d.mov_abs(Register::Ebx, layout.control_pointer_va)?;
    d.mov_store(Register::Ebx, 0x0dc, Register::Esi)?;
    d.mov_abs(Register::Ebx, layout.control_pointer_va)?;
    d.mov_store(Register::Ebx, 0x0e0, Register::Eax)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.mov_store(Register::Eax, 0x0e4, Register::Ecx)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.mov_store(Register::Eax, 0x0e8, Register::Esi)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    set(Literal::CommonEc, d.mov_store_imm(Register::Eax, 0x0ec)?);
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.mov_store(Register::Eax, 0x0f0, Register::Edx)?;

    set(Literal::MultiplierF4, d.mov_imm(Register::Eax)?);
    d.mov_abs(Register::Edx, layout.global_word_va)?;
    d.imul_eax(Register::Edx)?;
    d.shrd(Register::Eax, Register::Edx, 16)?;
    d.mov_abs(Register::Edx, layout.control_pointer_va)?;
    d.mov_store(Register::Edx, 0x0f4, Register::Eax)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.mov_store(Register::Eax, 0x0fc, Register::Ecx)?;
    set(Literal::Multiplier100, d.mov_imm(Register::Eax)?);
    d.mov_abs(Register::Edx, layout.global_word_va)?;
    d.imul_eax(Register::Edx)?;
    d.shrd(Register::Eax, Register::Edx, 16)?;
    d.mov_abs(Register::Ecx, layout.control_pointer_va)?;
    d.mov_store(Register::Ebp, -8, Register::Esi)?;
    d.pop(Register::Ebx)?;
    d.mov_store(Register::Ecx, 0x100, Register::Eax)?;
    d.mov_abs(Register::Edx, layout.control_pointer_va)?;
    set(Literal::Common104, d.mov_store_imm(Register::Edx, 0x104)?);
    d.mov_abs(Register::Eax, layout.state_pointer_va)?;
    d.mov_abs(Register::Edx, layout.control_pointer_va)?;
    d.mov_load(
        Register::Ecx,
        Register::Eax,
        i32::from(layout.state_word_offset),
    )?;
    d.shift(7, Register::Ecx, 3)?;
    d.mov_store(Register::Edx, 0x118, Register::Ecx)?;
    d.mov_abs(Register::Eax, layout.control_pointer_va)?;
    d.mov_load(Register::Eax, Register::Eax, 0x104)?;
    d.test_reg(Register::Eax, Register::Eax)?;
    d.mov_store(Register::Ebp, -16, Register::Eax)?;
    d.branch8(0x75, layout.first_divide_va)?;
    d.alu_imm8(1, Register::Eax, -1)?;
    d.branch8(0xeb, layout.first_divide_join_va)?;
    d.at(layout.first_divide_va)?;
    reciprocal_body(&mut d, -8, -16, -8)?;
    d.at(layout.first_divide_join_va)?;
    d.mov_abs(Register::Ecx, layout.control_pointer_va)?;
    d.mov_store(Register::Ebp, -8, Register::Esi)?;
    d.test_reg(Register::Edi, Register::Edi)?;
    d.mov_store(Register::Ecx, 0x108, Register::Eax)?;
    d.branch8(0x75, layout.second_divide_va)?;
    d.alu_imm8(1, Register::Eax, -1)?;
    d.branch8(0xeb, layout.second_divide_join_va)?;
    d.at(layout.second_divide_va)?;
    reciprocal_body(&mut d, -8, -12, -8)?;
    d.at(layout.second_divide_join_va)?;
    d.mov_abs(Register::Edx, layout.control_pointer_va)?;
    d.mov_store(Register::Ebp, -12, Register::Esi)?;
    d.pop(Register::Edi)?;
    d.pop(Register::Esi)?;
    d.mov_store(Register::Edx, 0x11c, Register::Eax)?;
    d.mov_load(Register::Eax, Register::Ebp, -4)?;
    d.test_reg(Register::Eax, Register::Eax)?;
    d.branch8(0x75, layout.third_divide_va)?;
    if d.mov_store_imm(Register::Ebp, -12)? != -1 {
        return d.fail("wrong zero-divisor sentinel");
    }
    d.branch8(0xeb, layout.third_divide_join_va)?;
    d.at(layout.third_divide_va)?;
    reciprocal_body(&mut d, -12, -4, -12)?;
    d.mov_store(Register::Ebp, -12, Register::Eax)?;
    d.at(layout.third_divide_join_va)?;
    d.mov_load(Register::Eax, Register::Ebp, -12)?;
    d.mov_abs(Register::Edx, layout.global_word_va)?;
    d.imul_eax(Register::Edx)?;
    d.shrd(Register::Eax, Register::Edx, 16)?;
    d.mov_abs(Register::Ecx, layout.control_pointer_va)?;
    d.mov_store(Register::Ecx, 0x12c, Register::Eax)?;
    d.mov_reg(Register::Esp, Register::Ebp)?;
    d.pop(Register::Ebp)?;
    d.opcode(0xc3)?;
    d.at(layout.function_end_va)?;
    d.done()?;
    Ok(v)
}

fn reciprocal_body(
    d: &mut SemanticCursor<'_>,
    numerator_stack: i32,
    divisor_stack: i32,
    result_stack: i32,
) -> Result<(), AuxControlInitializerError> {
    d.mov_load(Register::Eax, Register::Ebp, numerator_stack)?;
    d.mov_load(Register::Ecx, Register::Ebp, divisor_stack)?;
    d.cdq()?;
    d.shld(Register::Edx, Register::Eax, 16)?;
    d.shift(4, Register::Eax, 16)?;
    d.idiv(Register::Ecx)?;
    d.mov_store(Register::Ebp, result_stack, Register::Eax)?;
    d.mov_load(Register::Eax, Register::Ebp, result_stack)
}

fn validate_layout(layout: &AuxControlInitializerLayout) -> Result<(), AuxControlInitializerError> {
    if layout != &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT {
        return Err(AuxControlInitializerError::Layout(
            "layout does not equal the canonical official-EU profile",
        ));
    }
    Ok(())
}

/// Extract neutral constants from an already identity-checked PE image.
pub fn extract_aux_control_constants(
    input: &[u8],
    layout: &AuxControlInitializerLayout,
) -> Result<AuxControlConstantWords, AuxControlInitializerError> {
    validate_layout(layout)?;
    let image = Pe32Image::parse(input)?;
    if image.image_base() != layout.image_base {
        return Err(AuxControlInitializerError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let len = usize::try_from(layout.function_end_va - layout.function_start_va)
        .map_err(|_| AuxControlInitializerError::Layout("function length overflow"))?;
    let body = image.read_code_va_range(layout.function_start_va, len)?;
    let values = decode_semantic_body(body, layout)?;
    Ok(constants_from_literals(&values))
}

fn constants_from_literals(v: &[i32; LITERAL_COUNT]) -> AuxControlConstantWords {
    let get = |id: Literal| v[id as usize];
    AuxControlConstantWords {
        common_words: [
            get(Literal::CommonD8),
            get(Literal::SeedEsi),
            get(Literal::CommonD8),
            get(Literal::SeedEcx),
            get(Literal::SeedEsi),
            get(Literal::CommonEc),
            get(Literal::SeedEdx),
            get(Literal::SeedEcx),
            get(Literal::Common104),
        ],
        q16_multipliers: [get(Literal::MultiplierF4), get(Literal::Multiplier100)],
        policy_one: AuxControlPolicyWords {
            words: [
                get(Literal::PolicyOneF8),
                get(Literal::PolicyOne10c),
                get(Literal::PolicyOne110),
                get(Literal::PolicyOne120),
                get(Literal::PolicyOne124),
                get(Literal::PolicyOne128),
                get(Literal::PolicyOne13c),
            ],
            reciprocal_divisor: get(Literal::SeedEdx),
            scaled_reciprocal_divisor: get(Literal::PolicyOneDivisorB),
        },
        policy_two: AuxControlPolicyWords {
            words: [
                get(Literal::SeedEsi),
                get(Literal::PolicyTwo10c),
                get(Literal::PolicyTwo110),
                get(Literal::PolicyTwo120),
                get(Literal::PolicyTwo124),
                get(Literal::PolicyTwo128),
                get(Literal::PolicyTwo13c),
            ],
            reciprocal_divisor: get(Literal::SeedEcx),
            scaled_reciprocal_divisor: get(Literal::PolicyTwoDivisorB),
        },
        default_policy: AuxControlPolicyWords {
            words: [
                get(Literal::SeedEdx),
                get(Literal::Default10c),
                get(Literal::Default110),
                get(Literal::Default120),
                get(Literal::Default124),
                get(Literal::Default128),
                get(Literal::SeedEdx),
            ],
            reciprocal_divisor: get(Literal::DefaultDivisorA),
            scaled_reciprocal_divisor: get(Literal::DefaultDivisorB),
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuxControlCodecError {
    WrongLength {
        expected: usize,
        actual: usize,
    },
    WrongMagic,
    UnsupportedVersion {
        version: u32,
    },
    WrongCount {
        field: &'static str,
        expected: u32,
        actual: u32,
    },
}

impl fmt::Display for AuxControlCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid CMRAUXC data: {self:?}")
    }
}

impl Error for AuxControlCodecError {}

#[must_use]
pub fn encode_aux_control_constants(
    constants: &AuxControlConstantWords,
) -> [u8; AUX_CONTROL_ENCODED_LEN] {
    let mut output = [0_u8; AUX_CONTROL_ENCODED_LEN];
    output[..8].copy_from_slice(&AUX_CONTROL_MAGIC);
    for (offset, value) in [
        (8, AUX_CONTROL_FORMAT_VERSION),
        (12, AUX_CONTROL_COMMON_WORD_COUNT as u32),
        (16, AUX_CONTROL_MULTIPLIER_COUNT as u32),
        (20, AUX_CONTROL_POLICY_COUNT as u32),
        (24, AUX_CONTROL_POLICY_WORD_COUNT as u32),
        (28, AUX_CONTROL_POLICY_DIVISOR_COUNT as u32),
    ] {
        output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cursor = 32;
    for word in constant_words(constants) {
        output[cursor..cursor + 4].copy_from_slice(&word.to_le_bytes());
        cursor += 4;
    }
    output
}

pub fn decode_aux_control_constants(
    input: &[u8],
) -> Result<AuxControlConstantWords, AuxControlCodecError> {
    if input.len() != AUX_CONTROL_ENCODED_LEN {
        return Err(AuxControlCodecError::WrongLength {
            expected: AUX_CONTROL_ENCODED_LEN,
            actual: input.len(),
        });
    }
    if input[..8] != AUX_CONTROL_MAGIC {
        return Err(AuxControlCodecError::WrongMagic);
    }
    let version = read_u32(input, 8);
    if version != AUX_CONTROL_FORMAT_VERSION {
        return Err(AuxControlCodecError::UnsupportedVersion { version });
    }
    for (offset, field, expected) in [
        (
            12,
            "common word count",
            AUX_CONTROL_COMMON_WORD_COUNT as u32,
        ),
        (16, "multiplier count", AUX_CONTROL_MULTIPLIER_COUNT as u32),
        (20, "policy count", AUX_CONTROL_POLICY_COUNT as u32),
        (
            24,
            "policy word count",
            AUX_CONTROL_POLICY_WORD_COUNT as u32,
        ),
        (
            28,
            "policy divisor count",
            AUX_CONTROL_POLICY_DIVISOR_COUNT as u32,
        ),
    ] {
        let actual = read_u32(input, offset);
        if actual != expected {
            return Err(AuxControlCodecError::WrongCount {
                field,
                expected,
                actual,
            });
        }
    }
    let mut cursor = 32;
    let mut next = || {
        let value = read_i32(input, cursor);
        cursor += 4;
        value
    };
    let common_words = std::array::from_fn(|_| next());
    let q16_multipliers = std::array::from_fn(|_| next());
    let mut policy = || AuxControlPolicyWords {
        words: std::array::from_fn(|_| next()),
        reciprocal_divisor: next(),
        scaled_reciprocal_divisor: next(),
    };
    Ok(AuxControlConstantWords {
        common_words,
        q16_multipliers,
        policy_one: policy(),
        policy_two: policy(),
        default_policy: policy(),
    })
}

fn constant_words(constants: &AuxControlConstantWords) -> impl Iterator<Item = i32> + '_ {
    constants
        .common_words
        .into_iter()
        .chain(constants.q16_multipliers)
        .chain(
            [
                constants.policy_one,
                constants.policy_two,
                constants.default_policy,
            ]
            .into_iter()
            .flat_map(|policy| {
                policy
                    .words
                    .into_iter()
                    .chain([policy.reciprocal_divisor, policy.scaled_reciprocal_divisor])
            }),
        )
}

fn read_u32(input: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        input[offset..offset + 4]
            .try_into()
            .expect("validated codec range"),
    )
}

fn read_i32(input: &[u8], offset: usize) -> i32 {
    read_u32(input, offset) as i32
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod test_support {
    use super::*;

    const PE_OFFSET: usize = 0x80;
    const OPTIONAL_SIZE: usize = 0xe0;
    const SECTION_TABLE: usize = PE_OFFSET + 4 + 20 + OPTIONAL_SIZE;
    const RAW_OFFSET: usize = 0x200;
    const SECTION_RVA: u32 = 0x7000;
    const RAW_SIZE: usize = 0x1000;

    struct Fixture {
        bytes: Vec<u8>,
        expected: AuxControlConstantWords,
        #[cfg(test)]
        body_offset: usize,
        #[cfg(test)]
        literal_offsets: [usize; LITERAL_COUNT],
    }

    struct SyntheticAssembler {
        bytes: Vec<u8>,
        literals: [usize; LITERAL_COUNT],
        start: u32,
    }

    impl SyntheticAssembler {
        fn new(start: u32) -> Self {
            Self {
                bytes: Vec::new(),
                literals: [usize::MAX; LITERAL_COUNT],
                start,
            }
        }
        fn va(&self) -> u32 {
            self.start + self.bytes.len() as u32
        }
        fn b(&mut self, byte: u8) {
            self.bytes.push(byte);
        }
        fn u32(&mut self, value: u32) {
            self.bytes.extend_from_slice(&value.to_le_bytes());
        }
        fn modrm(&mut self, mode: u8, reg: Register, rm: Register) {
            self.b((mode << 6) | ((reg as u8) << 3) | rm as u8);
        }
        fn push(&mut self, r: Register) {
            self.b(0x50 + r as u8);
        }
        fn pop(&mut self, r: Register) {
            self.b(0x58 + r as u8);
        }
        fn mov_reg(&mut self, dst: Register, src: Register) {
            // Deliberately use the equivalent 89 /r form rather than the
            // encoding observed in the reviewed executable.
            self.b(0x89);
            self.modrm(3, src, dst);
        }
        fn mov_imm(&mut self, dst: Register, id: Literal, values: &[i32; LITERAL_COUNT]) {
            self.b(0xb8 + dst as u8);
            self.literals[id as usize] = self.bytes.len();
            self.u32(values[id as usize] as u32);
        }
        fn mov_abs(&mut self, dst: Register, address: u32) {
            if matches!(dst, Register::Eax) {
                self.b(0xa1);
            } else {
                self.b(0x8b);
                self.modrm(0, dst, Register::Ebp);
            }
            self.u32(address);
        }
        fn disp(&mut self, displacement: i32) {
            if (-128..=127).contains(&displacement) {
                self.b(displacement as i8 as u8);
            } else {
                self.u32(displacement as u32);
            }
        }
        fn mov_load(&mut self, dst: Register, base: Register, disp: i32) {
            self.b(0x8b);
            self.modrm(if (-128..=127).contains(&disp) { 1 } else { 2 }, dst, base);
            self.disp(disp);
        }
        fn mov_store(&mut self, base: Register, disp: i32, src: Register) {
            self.b(0x89);
            self.modrm(if (-128..=127).contains(&disp) { 1 } else { 2 }, src, base);
            self.disp(disp);
        }
        fn mov_store_imm(
            &mut self,
            base: Register,
            disp: i32,
            id: Literal,
            values: &[i32; LITERAL_COUNT],
        ) {
            self.b(0xc7);
            self.modrm(
                if (-128..=127).contains(&disp) { 1 } else { 2 },
                Register::Eax,
                base,
            );
            self.disp(disp);
            self.literals[id as usize] = self.bytes.len();
            self.u32(values[id as usize] as u32);
        }
        fn store_minus_one(&mut self, base: Register, disp: i32) {
            self.b(0xc7);
            self.modrm(1, Register::Eax, base);
            self.disp(disp);
            self.u32(u32::MAX);
        }
        fn alu8(&mut self, ext: u8, dst: Register, imm: i8) {
            self.b(0x83);
            self.b(0xc0 | (ext << 3) | dst as u8);
            self.b(imm as u8);
        }
        fn branch8(&mut self, opcode: u8, target: u32) {
            let next = self.va() + 2;
            self.b(opcode);
            self.b((target as i64 - next as i64) as i8 as u8);
        }
        fn branch32(&mut self, two: bool, opcode: u8, target: u32) {
            let len = if two { 6 } else { 5 };
            let next = self.va() + len;
            if two {
                self.b(0x0f);
            }
            self.b(opcode);
            self.u32((target as i64 - next as i64) as i32 as u32);
        }
        fn shift(&mut self, ext: u8, r: Register, n: u8) {
            self.b(0xc1);
            self.b(0xc0 | (ext << 3) | r as u8);
            self.b(n);
        }
        fn q16_finish(&mut self) {
            self.b(0xf7);
            self.modrm(3, Register::Ebp, Register::Edx);
            self.b(0x0f);
            self.b(0xac);
            self.modrm(3, Register::Edx, Register::Eax);
            self.b(16);
        }
        fn reciprocal(&mut self, numerator: i32, divisor: i32, result: i32) {
            self.mov_load(Register::Eax, Register::Ebp, numerator);
            self.mov_load(Register::Ecx, Register::Ebp, divisor);
            self.b(0x99);
            self.b(0x0f);
            self.b(0xa4);
            self.modrm(3, Register::Eax, Register::Edx);
            self.b(16);
            self.shift(4, Register::Eax, 16);
            self.b(0xf7);
            self.modrm(3, Register::Edi, Register::Ecx);
            self.mov_store(Register::Ebp, result, Register::Eax);
            self.mov_load(Register::Eax, Register::Ebp, result);
        }
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn file_offset(va: u32) -> usize {
        RAW_OFFSET + usize::try_from(va - 0x0040_0000 - SECTION_RVA).unwrap()
    }

    fn literal_values() -> [i32; LITERAL_COUNT] {
        let mut values = std::array::from_fn(|index| index as i32 * -101 - 7);
        values[0] = i32::MIN;
        values[1] = i32::MAX;
        values[2] = 0;
        values[3] = -1;
        values
    }

    fn synthetic_body(
        layout: &AuxControlInitializerLayout,
        values: &[i32; LITERAL_COUNT],
    ) -> (Vec<u8>, [usize; LITERAL_COUNT]) {
        let mut a = SyntheticAssembler::new(layout.function_start_va);
        a.push(Register::Ebp);
        a.mov_reg(Register::Ebp, Register::Esp);
        a.alu8(5, Register::Esp, 0x10);
        a.mov_abs(Register::Ecx, layout.config_pointer_va);
        a.b(0xf6);
        a.modrm(1, Register::Eax, Register::Ecx);
        a.b(layout.config_flag_offset as u8);
        a.b(2);
        a.branch8(0x74, layout.lookup_va);
        a.b(0x31);
        a.modrm(3, Register::Eax, Register::Eax);
        a.branch8(0xeb, layout.select_va);
        assert_eq!(a.va(), layout.lookup_va);
        a.mov_abs(Register::Eax, layout.state_pointer_va);
        a.b(0x0f);
        a.b(0xbe);
        a.modrm(2, Register::Eax, Register::Eax);
        a.u32(u32::from(layout.state_index_offset));
        a.b(0x8d);
        a.modrm(0, Register::Edx, Register::Esp);
        a.b(0x80);
        a.shift(4, Register::Edx, 3);
        a.b(0x29);
        a.modrm(3, Register::Eax, Register::Edx);
        a.b(0x8a);
        a.modrm(2, Register::Eax, Register::Esp);
        a.b(0x91);
        a.u32(u32::from(layout.config_option_offset));
        a.alu8(4, Register::Eax, 7);
        assert_eq!(a.va(), layout.select_va);
        a.push(Register::Esi);
        a.b(0x48);
        a.push(Register::Edi);
        a.mov_imm(Register::Edx, Literal::SeedEdx, values);
        a.mov_imm(Register::Esi, Literal::SeedEsi, values);
        a.mov_imm(Register::Ecx, Literal::SeedEcx, values);
        a.branch32(true, 0x84, layout.policy_one_va);
        a.b(0x48);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.branch8(0x74, layout.policy_two_va);

        assert_eq!(a.va(), layout.default_policy_va);
        a.mov_store(Register::Eax, 0xf8, Register::Edx);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store_imm(Register::Ebp, -4, Literal::DefaultDivisorB, values);
        a.mov_imm(Register::Edi, Literal::DefaultDivisorA, values);
        a.mov_store(Register::Eax, 0x13c, Register::Edx);
        for (off, id) in [
            (0x10c, Literal::Default10c),
            (0x110, Literal::Default110),
            (0x120, Literal::Default120),
            (0x124, Literal::Default124),
            (0x128, Literal::Default128),
        ] {
            a.mov_abs(Register::Eax, layout.control_pointer_va);
            a.mov_store_imm(Register::Eax, off, id, values);
        }
        a.branch32(false, 0xe9, layout.common_va);

        assert_eq!(a.va(), layout.policy_two_va);
        a.mov_store(Register::Eax, 0xf8, Register::Esi);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store_imm(Register::Ebp, -4, Literal::PolicyTwoDivisorB, values);
        a.mov_reg(Register::Edi, Register::Ecx);
        a.mov_store_imm(Register::Eax, 0x13c, Literal::PolicyTwo13c, values);
        for (off, id) in [
            (0x10c, Literal::PolicyTwo10c),
            (0x110, Literal::PolicyTwo110),
            (0x120, Literal::PolicyTwo120),
            (0x124, Literal::PolicyTwo124),
            (0x128, Literal::PolicyTwo128),
        ] {
            a.mov_abs(Register::Eax, layout.control_pointer_va);
            a.mov_store_imm(Register::Eax, off, id, values);
        }
        a.branch8(0xeb, layout.common_va);

        assert_eq!(a.va(), layout.policy_one_va);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store_imm(Register::Ebp, -4, Literal::PolicyOneDivisorB, values);
        a.mov_reg(Register::Edi, Register::Edx);
        a.mov_store_imm(Register::Eax, 0xf8, Literal::PolicyOneF8, values);
        for (off, id) in [
            (0x13c, Literal::PolicyOne13c),
            (0x10c, Literal::PolicyOne10c),
            (0x110, Literal::PolicyOne110),
            (0x120, Literal::PolicyOne120),
            (0x124, Literal::PolicyOne124),
            (0x128, Literal::PolicyOne128),
        ] {
            a.mov_abs(Register::Eax, layout.control_pointer_va);
            a.mov_store_imm(Register::Eax, off, id, values);
        }

        assert_eq!(a.va(), layout.common_va);
        a.push(Register::Ebx);
        a.mov_abs(Register::Ebx, layout.control_pointer_va);
        a.mov_imm(Register::Eax, Literal::CommonD8, values);
        a.mov_store(Register::Ebp, -12, Register::Edi);
        a.mov_store(Register::Ebx, 0xd8, Register::Eax);
        a.mov_abs(Register::Ebx, layout.control_pointer_va);
        a.mov_store(Register::Ebx, 0xdc, Register::Esi);
        a.mov_abs(Register::Ebx, layout.control_pointer_va);
        a.mov_store(Register::Ebx, 0xe0, Register::Eax);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store(Register::Eax, 0xe4, Register::Ecx);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store(Register::Eax, 0xe8, Register::Esi);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store_imm(Register::Eax, 0xec, Literal::CommonEc, values);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store(Register::Eax, 0xf0, Register::Edx);
        a.mov_imm(Register::Eax, Literal::MultiplierF4, values);
        a.mov_abs(Register::Edx, layout.global_word_va);
        a.q16_finish();
        a.mov_abs(Register::Edx, layout.control_pointer_va);
        a.mov_store(Register::Edx, 0xf4, Register::Eax);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_store(Register::Eax, 0xfc, Register::Ecx);
        a.mov_imm(Register::Eax, Literal::Multiplier100, values);
        a.mov_abs(Register::Edx, layout.global_word_va);
        a.q16_finish();
        a.mov_abs(Register::Ecx, layout.control_pointer_va);
        a.mov_store(Register::Ebp, -8, Register::Esi);
        a.pop(Register::Ebx);
        a.mov_store(Register::Ecx, 0x100, Register::Eax);
        a.mov_abs(Register::Edx, layout.control_pointer_va);
        a.mov_store_imm(Register::Edx, 0x104, Literal::Common104, values);
        a.mov_abs(Register::Eax, layout.state_pointer_va);
        a.mov_abs(Register::Edx, layout.control_pointer_va);
        a.mov_load(
            Register::Ecx,
            Register::Eax,
            i32::from(layout.state_word_offset),
        );
        a.shift(7, Register::Ecx, 3);
        a.mov_store(Register::Edx, 0x118, Register::Ecx);
        a.mov_abs(Register::Eax, layout.control_pointer_va);
        a.mov_load(Register::Eax, Register::Eax, 0x104);
        a.b(0x85);
        a.modrm(3, Register::Eax, Register::Eax);
        a.mov_store(Register::Ebp, -16, Register::Eax);
        a.branch8(0x75, layout.first_divide_va);
        a.alu8(1, Register::Eax, -1);
        a.branch8(0xeb, layout.first_divide_join_va);
        assert_eq!(a.va(), layout.first_divide_va);
        a.reciprocal(-8, -16, -8);
        assert_eq!(a.va(), layout.first_divide_join_va);
        a.mov_abs(Register::Ecx, layout.control_pointer_va);
        a.mov_store(Register::Ebp, -8, Register::Esi);
        a.b(0x85);
        a.modrm(3, Register::Edi, Register::Edi);
        a.mov_store(Register::Ecx, 0x108, Register::Eax);
        a.branch8(0x75, layout.second_divide_va);
        a.alu8(1, Register::Eax, -1);
        a.branch8(0xeb, layout.second_divide_join_va);
        assert_eq!(a.va(), layout.second_divide_va);
        a.reciprocal(-8, -12, -8);
        assert_eq!(a.va(), layout.second_divide_join_va);
        a.mov_abs(Register::Edx, layout.control_pointer_va);
        a.mov_store(Register::Ebp, -12, Register::Esi);
        a.pop(Register::Edi);
        a.pop(Register::Esi);
        a.mov_store(Register::Edx, 0x11c, Register::Eax);
        a.mov_load(Register::Eax, Register::Ebp, -4);
        a.b(0x85);
        a.modrm(3, Register::Eax, Register::Eax);
        a.branch8(0x75, layout.third_divide_va);
        a.store_minus_one(Register::Ebp, -12);
        a.branch8(0xeb, layout.third_divide_join_va);
        assert_eq!(a.va(), layout.third_divide_va);
        a.reciprocal(-12, -4, -12);
        a.mov_store(Register::Ebp, -12, Register::Eax);
        assert_eq!(a.va(), layout.third_divide_join_va);
        a.mov_load(Register::Eax, Register::Ebp, -12);
        a.mov_abs(Register::Edx, layout.global_word_va);
        a.q16_finish();
        a.mov_abs(Register::Ecx, layout.control_pointer_va);
        a.mov_store(Register::Ecx, 0x12c, Register::Eax);
        a.mov_reg(Register::Esp, Register::Ebp);
        a.pop(Register::Ebp);
        a.b(0xc3);
        assert_eq!(a.va(), layout.function_end_va);
        (a.bytes, a.literals)
    }

    fn fixture() -> Fixture {
        let layout = OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT;
        let values = literal_values();
        let (body, literal_offsets) = synthetic_body(&layout, &values);
        #[cfg(not(test))]
        let _ = literal_offsets;

        let mut bytes = vec![0_u8; RAW_OFFSET + RAW_SIZE];
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE_OFFSET + 4, 0x14c);
        put_u16(&mut bytes, PE_OFFSET + 6, 1);
        put_u16(&mut bytes, PE_OFFSET + 20, OPTIONAL_SIZE as u16);
        put_u16(&mut bytes, PE_OFFSET + 24, 0x10b);
        put_u32(&mut bytes, PE_OFFSET + 24 + 28, layout.image_base);
        put_u32(&mut bytes, SECTION_TABLE + 8, RAW_SIZE as u32);
        put_u32(&mut bytes, SECTION_TABLE + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTION_TABLE + 16, RAW_SIZE as u32);
        put_u32(&mut bytes, SECTION_TABLE + 20, RAW_OFFSET as u32);
        put_u32(&mut bytes, SECTION_TABLE + 36, 0x6000_0020);
        let body_offset = file_offset(layout.function_start_va);
        bytes[body_offset..body_offset + body.len()].copy_from_slice(&body);
        Fixture {
            bytes,
            expected: constants_from_literals(&values),
            #[cfg(test)]
            body_offset,
            #[cfg(test)]
            literal_offsets,
        }
    }

    /// Build a source-owned synthetic PE that exercises the complete reviewed
    /// semantic instruction shape. Available only to tests opting into the
    /// `test-support` feature.
    #[must_use]
    pub fn synthetic_aux_control_pe() -> (Vec<u8>, AuxControlConstantWords) {
        let fixture = fixture();
        (fixture.bytes, fixture.expected)
    }

    #[cfg(test)]
    fn add_overlapping_section(bytes: &mut [u8], characteristics: u32) {
        put_u16(bytes, PE_OFFSET + 6, 2);
        let second = SECTION_TABLE + 40;
        put_u32(bytes, second + 8, RAW_SIZE as u32);
        put_u32(bytes, second + 12, SECTION_RVA);
        put_u32(bytes, second + 16, RAW_SIZE as u32);
        put_u32(bytes, second + 20, RAW_OFFSET as u32);
        put_u32(bytes, second + 36, characteristics);
    }

    #[test]
    fn synthetic_semantic_fixture_has_exact_profiled_extent() {
        let fixture = fixture();
        assert_eq!(fixture.literal_offsets.len(), LITERAL_COUNT);
        assert_eq!(
            OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT.function_end_va
                - OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT.function_start_va,
            799
        );
    }

    #[test]
    fn extracts_all_neutral_groups_and_signed_boundaries() {
        let fixture = fixture();
        let actual = extract_aux_control_constants(
            &fixture.bytes,
            &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT,
        )
        .unwrap();
        assert_eq!(actual, fixture.expected);
        assert_eq!(actual.default_policy.words[0], i32::MIN);
        assert_eq!(actual.common_words[1], i32::MAX);
        assert_eq!(actual.common_words[3], 0);
        assert_eq!(actual.default_policy.scaled_reciprocal_divisor, -1);
    }

    #[test]
    fn every_nonliteral_body_byte_is_structural() {
        let fixture = fixture();
        let mut literal_byte = [false; 799];
        for offset in fixture.literal_offsets {
            literal_byte[offset..offset + 4].fill(true);
        }
        for (offset, is_literal) in literal_byte.into_iter().enumerate() {
            if is_literal {
                continue;
            }
            let mut bytes = fixture.bytes.clone();
            bytes[fixture.body_offset + offset] ^= 1;
            assert!(
                matches!(
                    extract_aux_control_constants(
                        &bytes,
                        &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT
                    ),
                    Err(AuxControlInitializerError::Shape { .. })
                ),
                "structural byte {offset:#x} was admitted"
            );
        }
    }

    #[test]
    fn every_literal_slot_is_extracted_only_through_the_neutral_mapping() {
        let fixture = fixture();
        let mut expected_values = literal_values();
        for (index, offset) in fixture.literal_offsets.into_iter().enumerate() {
            let replacement = i32::MAX.wrapping_sub(index as i32 * 97);
            let mut bytes = fixture.bytes.clone();
            bytes[fixture.body_offset + offset..fixture.body_offset + offset + 4]
                .copy_from_slice(&replacement.to_le_bytes());
            expected_values[index] = replacement;
            assert_eq!(
                extract_aux_control_constants(&bytes, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT)
                    .unwrap(),
                constants_from_literals(&expected_values)
            );
            expected_values[index] = literal_values()[index];
        }
    }

    #[test]
    fn rejects_insertions_deletions_reordered_stores_and_source_changes() {
        let fixture = fixture();
        for delta in [100_usize, 300, 600] {
            let mut inserted = fixture.bytes.clone();
            let body = &mut inserted[fixture.body_offset..fixture.body_offset + 799];
            body.copy_within(delta..798, delta + 1);
            body[delta] = 0x90;
            assert!(
                extract_aux_control_constants(
                    &inserted,
                    &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT
                )
                .is_err()
            );

            let mut deleted = fixture.bytes.clone();
            let body = &mut deleted[fixture.body_offset..fixture.body_offset + 799];
            body.copy_within(delta + 1..799, delta);
            body[798] = 0;
            assert!(
                extract_aux_control_constants(
                    &deleted,
                    &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT
                )
                .is_err()
            );
        }

        // Swap equal-length policy store instructions.
        let mut reordered = fixture.bytes.clone();
        let first = file_offset(0x0040_72e1);
        let second = file_offset(0x0040_72f0);
        let a = reordered[first..first + 15].to_vec();
        let b = reordered[second..second + 15].to_vec();
        reordered[first..first + 15].copy_from_slice(&b);
        reordered[second..second + 15].copy_from_slice(&a);
        assert!(
            extract_aux_control_constants(&reordered, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT)
                .is_err()
        );

        for address in [0x0040_72b6, 0x0040_74eb, 0x0040_7519, 0x0040_754c] {
            let mut branch = fixture.bytes.clone();
            branch[file_offset(address) + 1] ^= 1;
            assert!(
                extract_aux_control_constants(&branch, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT)
                    .is_err()
            );
        }

        for address in [0x0040_7276, 0x0040_7470, 0x0040_7523] {
            let mut source = fixture.bytes.clone();
            source[file_offset(address) + 1] ^= 1;
            assert!(
                extract_aux_control_constants(&source, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT)
                    .is_err()
            );
        }
    }

    #[test]
    fn requires_exact_image_mapping_permissions_and_layout_schema() {
        let fixture = fixture();
        let mut image_base = fixture.bytes.clone();
        put_u32(&mut image_base, PE_OFFSET + 24 + 28, 0x0040_1000);
        assert!(matches!(
            extract_aux_control_constants(&image_base, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT),
            Err(AuxControlInitializerError::ImageBase { .. })
        ));

        let mut noncode = fixture.bytes.clone();
        put_u32(&mut noncode, SECTION_TABLE + 36, 0x4000_0040);
        assert!(matches!(
            extract_aux_control_constants(&noncode, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT),
            Err(AuxControlInitializerError::Pe(
                AuxControlPeError::SectionNotExecutable { .. }
            ))
        ));

        let mut ambiguous = fixture.bytes.clone();
        add_overlapping_section(&mut ambiguous, 0x6000_0020);
        assert!(matches!(
            extract_aux_control_constants(&ambiguous, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT),
            Err(AuxControlInitializerError::Pe(
                AuxControlPeError::AmbiguousRange { .. }
            ))
        ));

        let mut unmapped = fixture.bytes.clone();
        let body_delta = u32::try_from(fixture.body_offset - RAW_OFFSET).unwrap();
        let short = body_delta + 798;
        put_u32(&mut unmapped, SECTION_TABLE + 8, short);
        put_u32(&mut unmapped, SECTION_TABLE + 16, short);
        assert!(matches!(
            extract_aux_control_constants(&unmapped, &OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT),
            Err(AuxControlInitializerError::Pe(
                AuxControlPeError::UnmappedRange { .. }
            ))
        ));

        let mut layout = OFFICIAL_EU_AUX_CONTROL_INITIALIZER_LAYOUT;
        layout.output_offsets.swap(0, 1);
        assert_eq!(
            extract_aux_control_constants(&fixture.bytes, &layout),
            Err(AuxControlInitializerError::Layout(
                "layout does not equal the canonical official-EU profile"
            ))
        );
    }

    #[test]
    fn codec_is_fixed_versioned_and_signed_lossless() {
        let constants = fixture().expected;
        let encoded = encode_aux_control_constants(&constants);
        assert_eq!(encoded.len(), 184);
        assert_eq!(decode_aux_control_constants(&encoded).unwrap(), constants);
        assert_eq!(
            decode_aux_control_constants(&encoded[..183]),
            Err(AuxControlCodecError::WrongLength {
                expected: 184,
                actual: 183,
            })
        );
        let mut trailing = encoded.to_vec();
        trailing.push(0);
        assert_eq!(
            decode_aux_control_constants(&trailing),
            Err(AuxControlCodecError::WrongLength {
                expected: 184,
                actual: 185,
            })
        );
        let mut changed = encoded;
        changed[0] ^= 1;
        assert_eq!(
            decode_aux_control_constants(&changed),
            Err(AuxControlCodecError::WrongMagic)
        );
        let mut changed = encoded;
        put_u32(&mut changed, 8, 2);
        assert_eq!(
            decode_aux_control_constants(&changed),
            Err(AuxControlCodecError::UnsupportedVersion { version: 2 })
        );
        for (offset, field, expected) in [
            (12, "common word count", 9),
            (16, "multiplier count", 2),
            (20, "policy count", 3),
            (24, "policy word count", 7),
            (28, "policy divisor count", 2),
        ] {
            let mut changed = encoded;
            put_u32(&mut changed, offset, expected + 1);
            assert_eq!(
                decode_aux_control_constants(&changed),
                Err(AuxControlCodecError::WrongCount {
                    field,
                    expected,
                    actual: expected + 1,
                })
            );
        }
    }

    #[test]
    fn codec_wire_order_is_independent_and_pins_signed_endpoints() {
        let constants = AuxControlConstantWords {
            common_words: [i32::MIN, -8, -7, -6, -5, -4, -3, -2, -1],
            q16_multipliers: [0, i32::MAX],
            policy_one: AuxControlPolicyWords {
                words: [10, 11, 12, 13, 14, 15, 16],
                reciprocal_divisor: 17,
                scaled_reciprocal_divisor: 18,
            },
            policy_two: AuxControlPolicyWords {
                words: [20, 21, 22, 23, 24, 25, 26],
                reciprocal_divisor: 27,
                scaled_reciprocal_divisor: 28,
            },
            default_policy: AuxControlPolicyWords {
                words: [30, 31, 32, 33, 34, 35, 36],
                reciprocal_divisor: 37,
                scaled_reciprocal_divisor: 38,
            },
        };
        let expected_wire_words = [
            i32::MIN,
            -8,
            -7,
            -6,
            -5,
            -4,
            -3,
            -2,
            -1,
            0,
            i32::MAX,
            10,
            11,
            12,
            13,
            14,
            15,
            16,
            17,
            18,
            20,
            21,
            22,
            23,
            24,
            25,
            26,
            27,
            28,
            30,
            31,
            32,
            33,
            34,
            35,
            36,
            37,
            38,
        ];
        let encoded = encode_aux_control_constants(&constants);
        assert_eq!(&encoded[..8], b"CMRAUXC\0");
        assert_eq!(read_u32(&encoded, 8), 1);
        assert_eq!(
            std::array::from_fn::<_, AUX_CONTROL_TOTAL_WORD_COUNT, _>(|index| {
                read_i32(&encoded, 32 + index * 4)
            }),
            expected_wire_words
        );
        assert_eq!(decode_aux_control_constants(&encoded).unwrap(), constants);
    }
}
