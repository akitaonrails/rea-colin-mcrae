//! Pure extraction and canonical storage of compiled initializer case words.
//!
//! The decoder accepts only the reviewed i386 instruction forms. Artifact
//! identity and filesystem publication belong to higher layers.

use std::{error::Error, fmt};

use crate::pe32::{Pe32Error, Pe32Image};

pub const COMPILED_CASE_COUNT: usize = 14;
pub const COMPILED_STATE_WORD_COUNT: usize = 8;
pub const COMPILED_CONTROL_WORD_COUNT: usize = 23;
pub const COMPILED_WORD_COUNT: usize = 31;
pub const SEED_CALL_TARGET_COUNT: usize = 6;
pub const COMPILED_CASE_MAGIC: [u8; 8] = *b"CMRCCASE";
pub const COMPILED_CASE_FORMAT_VERSION: u32 = 1;
pub const COMPILED_CASE_ENCODED_LEN: usize = 24 + COMPILED_CASE_COUNT * COMPILED_WORD_COUNT * 4;

pub const COMPILED_STATE_WORD_OFFSETS: [u16; COMPILED_STATE_WORD_COUNT] =
    [0x020, 0x024, 0x028, 0x638, 0x63c, 0x640, 0x648, 0x644];
pub const COMPILED_CONTROL_WORD_OFFSETS: [u16; COMPILED_CONTROL_WORD_COUNT] = [
    0x148, 0x158, 0x16c, 0x15c, 0x194, 0x198, 0x19c, 0x1a0, 0x1a4, 0x1b8, 0x1bc, 0x1c0, 0x1c4,
    0x1c8, 0x1d0, 0x1d4, 0x1d8, 0x1dc, 0x1e0, 0x1e4, 0x1e8, 0x1ec, 0x140,
];

/// Reviewed addresses and neutral destination schema for one supported image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompiledInitializerLayout {
    pub image_base: u32,
    pub zero_seed_va: u32,
    pub esi_seed_va: u32,
    pub edi_seed_va: u32,
    pub seed_call_targets: [u32; SEED_CALL_TARGET_COUNT],
    pub dispatch_va: u32,
    pub jump_table_va: u32,
    pub first_case_va: u32,
    pub common_join_va: u32,
    pub state_pointer_va: u32,
    pub control_pointer_va: u32,
    pub state_offsets: [u16; COMPILED_STATE_WORD_COUNT],
    pub control_offsets: [u16; COMPILED_CONTROL_WORD_COUNT],
}

/// Address-only official-EU decoding profile. It contains no case values.
pub const OFFICIAL_EU_COMPILED_INITIALIZER_LAYOUT: CompiledInitializerLayout =
    CompiledInitializerLayout {
        image_base: 0x0040_0000,
        zero_seed_va: 0x0040_447b,
        esi_seed_va: 0x0040_48a1,
        edi_seed_va: 0x0040_48a6,
        seed_call_targets: [
            0x0040_1060,
            0x0040_1000,
            0x0048_5bb0,
            0x0041_a820,
            0x0041_a840,
            0x0041_a860,
        ],
        dispatch_va: 0x0040_48d2,
        jump_table_va: 0x0040_7234,
        first_case_va: 0x0040_48e5,
        common_join_va: 0x0040_60d3,
        state_pointer_va: 0x0050_c5b4,
        control_pointer_va: 0x0050_c604,
        state_offsets: COMPILED_STATE_WORD_OFFSETS,
        control_offsets: COMPILED_CONTROL_WORD_OFFSETS,
    };

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompiledCaseWords {
    pub state_words: [i32; COMPILED_STATE_WORD_COUNT],
    pub control_words: [i32; COMPILED_CONTROL_WORD_COUNT],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledInitializerError {
    Pe(CompiledInitializerPeError),
    ImageBase {
        expected: u32,
        actual: u32,
    },
    Layout(&'static str),
    Dispatch {
        address: u32,
        detail: &'static str,
    },
    Seed {
        address: u32,
        detail: &'static str,
    },
    TableTarget {
        selector: u8,
        target: u32,
        detail: &'static str,
    },
    Case {
        selector: u8,
        address: u32,
        detail: &'static str,
    },
    Store {
        selector: u8,
        address: u32,
        store_index: usize,
        detail: &'static str,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledInitializerPeError {
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

impl From<Pe32Error> for CompiledInitializerError {
    fn from(value: Pe32Error) -> Self {
        Self::Pe(match value {
            Pe32Error::EmptyRange { virtual_address } => {
                CompiledInitializerPeError::EmptyRange { virtual_address }
            }
            Pe32Error::Truncated {
                offset,
                needed,
                input_len,
            } => CompiledInitializerPeError::Truncated {
                offset,
                needed,
                input_len,
            },
            Pe32Error::InvalidDosMagic { offset } => {
                CompiledInitializerPeError::InvalidDosMagic { offset }
            }
            Pe32Error::InvalidPeSignature { offset } => {
                CompiledInitializerPeError::InvalidPeSignature { offset }
            }
            Pe32Error::UnsupportedMachine { offset, machine } => {
                CompiledInitializerPeError::UnsupportedMachine { offset, machine }
            }
            Pe32Error::NotPe32 { offset, magic } => {
                CompiledInitializerPeError::NotPe32 { offset, magic }
            }
            Pe32Error::OptionalHeaderTooShort { offset, size } => {
                CompiledInitializerPeError::OptionalHeaderTooShort { offset, size }
            }
            Pe32Error::AddressBeforeImage {
                virtual_address,
                image_base,
            } => CompiledInitializerPeError::AddressBeforeImage {
                virtual_address,
                image_base,
            },
            Pe32Error::UnmappedRange {
                virtual_address,
                byte_len,
            } => CompiledInitializerPeError::UnmappedRange {
                virtual_address,
                byte_len,
            },
            Pe32Error::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            } => CompiledInitializerPeError::AmbiguousRange {
                virtual_address,
                first_section_offset,
                second_section_offset,
            },
            Pe32Error::SectionNotExecutable {
                virtual_address,
                section_offset,
                characteristics,
            } => CompiledInitializerPeError::SectionNotExecutable {
                virtual_address,
                section_offset,
                characteristics,
            },
            Pe32Error::ArithmeticOverflow { offset } => {
                CompiledInitializerPeError::ArithmeticOverflow { offset }
            }
        })
    }
}

impl fmt::Display for CompiledInitializerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pe(error) => write!(f, "PE mapping failed: {error:?}"),
            Self::ImageBase { expected, actual } => write!(
                f,
                "PE image base {actual:#010x} does not match {expected:#010x}"
            ),
            Self::Layout(detail) => write!(f, "invalid compiled-initializer layout: {detail}"),
            Self::Dispatch { address, detail } => {
                write!(f, "invalid dispatch at {address:#010x}: {detail}")
            }
            Self::Seed { address, detail } => {
                write!(f, "invalid seed at {address:#010x}: {detail}")
            }
            Self::TableTarget {
                selector,
                target,
                detail,
            } => write!(
                f,
                "invalid selector {selector} target {target:#010x}: {detail}"
            ),
            Self::Case {
                selector,
                address,
                detail,
            } => write!(
                f,
                "invalid selector {selector} case at {address:#010x}: {detail}"
            ),
            Self::Store {
                selector,
                address,
                store_index,
                detail,
            } => write!(
                f,
                "invalid selector {selector} store {store_index} at {address:#010x}: {detail}"
            ),
        }
    }
}

impl Error for CompiledInitializerError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaseTag {
    State,
    Control,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RegisterValue {
    Unknown,
    Base(BaseTag),
    Constant(u32),
}

#[derive(Clone, Copy)]
struct Registers([RegisterValue; 8]);

impl Registers {
    fn seeded(esi: u32, edi: u32) -> Self {
        let mut values = [RegisterValue::Unknown; 8];
        values[3] = RegisterValue::Constant(0);
        values[6] = RegisterValue::Constant(esi);
        values[7] = RegisterValue::Constant(edi);
        Self(values)
    }
}

/// Extract all fourteen cases from already identity-checked caller-owned bytes.
pub fn extract_compiled_initializer_cases(
    input: &[u8],
    layout: &CompiledInitializerLayout,
) -> Result<[CompiledCaseWords; COMPILED_CASE_COUNT], CompiledInitializerError> {
    validate_layout(layout)?;
    let image = Pe32Image::parse(input)?;
    if image.image_base() != layout.image_base {
        return Err(CompiledInitializerError::ImageBase {
            expected: layout.image_base,
            actual: image.image_base(),
        });
    }
    let (esi, edi) = decode_seeds(&image, layout)?;
    decode_dispatch(&image, layout)?;
    let table = image.read_code_va_range(layout.jump_table_va, COMPILED_CASE_COUNT * 4)?;
    let mut targets = [0_u32; COMPILED_CASE_COUNT];
    for selector in 0..COMPILED_CASE_COUNT {
        let target = read_u32(table, selector * 4);
        targets[selector] = target;
        if target < layout.first_case_va || target >= layout.common_join_va {
            return Err(CompiledInitializerError::TableTarget {
                selector: selector as u8,
                target,
                detail: "outside case region",
            });
        }
        if targets[..selector].contains(&target) {
            return Err(CompiledInitializerError::TableTarget {
                selector: selector as u8,
                target,
                detail: "duplicate target",
            });
        }
        image.read_code_va_range(target, 1)?;
    }
    let mut physical =
        std::array::from_fn::<_, COMPILED_CASE_COUNT, _>(|index| (targets[index], index as u8));
    physical.sort_unstable_by_key(|entry| entry.0);
    if physical[0].0 != layout.first_case_va {
        return Err(CompiledInitializerError::Layout(
            "first case is not the lowest target",
        ));
    }

    let mut output = [CompiledCaseWords::default(); COMPILED_CASE_COUNT];
    for index in 0..COMPILED_CASE_COUNT {
        let (start, selector) = physical[index];
        let end = physical
            .get(index + 1)
            .map_or(layout.common_join_va, |entry| entry.0);
        let len = usize::try_from(
            end.checked_sub(start)
                .ok_or(CompiledInitializerError::Layout("overlapping bodies"))?,
        )
        .map_err(|_| CompiledInitializerError::Layout("body length overflow"))?;
        let body = image.read_code_va_range(start, len)?;
        output[selector as usize] = decode_body(
            body,
            start,
            selector,
            index + 1 == COMPILED_CASE_COUNT,
            Registers::seeded(esi, edi),
            layout,
        )?;
    }
    Ok(output)
}

fn validate_layout(layout: &CompiledInitializerLayout) -> Result<(), CompiledInitializerError> {
    if layout.first_case_va >= layout.common_join_va {
        return Err(CompiledInitializerError::Layout("empty case region"));
    }
    if layout.state_offsets != COMPILED_STATE_WORD_OFFSETS
        || layout.control_offsets != COMPILED_CONTROL_WORD_OFFSETS
    {
        return Err(CompiledInitializerError::Layout(
            "unsupported destination schema",
        ));
    }
    for (index, target) in layout.seed_call_targets.iter().enumerate() {
        if *target == 0 || layout.seed_call_targets[..index].contains(target) {
            return Err(CompiledInitializerError::Layout(
                "seed call-target allowlist must be nonzero and unique",
            ));
        }
    }
    Ok(())
}

fn decode_seeds(
    image: &Pe32Image<'_>,
    layout: &CompiledInitializerLayout,
) -> Result<(u32, u32), CompiledInitializerError> {
    let proof_len = usize::try_from(layout.dispatch_va.checked_sub(layout.zero_seed_va).ok_or(
        CompiledInitializerError::Layout("seed proof starts after dispatch"),
    )?)
    .map_err(|_| CompiledInitializerError::Layout("seed proof length overflow"))?;
    let proof = image.read_code_va_range(layout.zero_seed_va, proof_len)?;
    if proof.get(..2) != Some(&[0x33, 0xdb]) {
        return Err(CompiledInitializerError::Seed {
            address: layout.zero_seed_va,
            detail: "expected XOR EBX,EBX",
        });
    }
    let esi = image.read_code_va_range(layout.esi_seed_va, 5)?;
    if esi[0] != 0xbe {
        return Err(CompiledInitializerError::Seed {
            address: layout.esi_seed_va,
            detail: "expected MOV ESI,imm32",
        });
    }
    let edi = image.read_code_va_range(layout.edi_seed_va, 5)?;
    if edi[0] != 0xbf {
        return Err(CompiledInitializerError::Seed {
            address: layout.edi_seed_va,
            detail: "expected MOV EDI,imm32",
        });
    }
    let expected_esi = read_u32(esi, 1);
    let expected_edi = read_u32(edi, 1);
    prove_seed_reaching_definitions(proof, layout, expected_esi, expected_edi)?;
    Ok((expected_esi, expected_edi))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProofValue {
    Unknown,
    Zero,
    Constant(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProofState {
    ebx: ProofValue,
    esi: ProofValue,
    edi: ProofValue,
    esi_seed_seen: bool,
    edi_seed_seen: bool,
}

impl ProofState {
    const UNKNOWN: Self = Self {
        ebx: ProofValue::Unknown,
        esi: ProofValue::Unknown,
        edi: ProofValue::Unknown,
        esi_seed_seen: false,
        edi_seed_seen: false,
    };

    fn merge(self, other: Self) -> Self {
        Self {
            ebx: merge_proof_value(self.ebx, other.ebx),
            esi: merge_proof_value(self.esi, other.esi),
            edi: merge_proof_value(self.edi, other.edi),
            esi_seed_seen: self.esi_seed_seen && other.esi_seed_seen,
            edi_seed_seen: self.edi_seed_seen && other.edi_seed_seen,
        }
    }

    fn set_register(&mut self, register: usize, value: ProofValue) {
        match register {
            3 => self.ebx = value,
            6 => self.esi = value,
            7 => self.edi = value,
            _ => {}
        }
    }
}

fn merge_proof_value(left: ProofValue, right: ProofValue) -> ProofValue {
    if left == right {
        left
    } else {
        ProofValue::Unknown
    }
}

#[derive(Clone, Copy, Debug)]
enum ProofFlow {
    Next,
    Conditional(u32),
    Jump(u32),
}

#[derive(Clone, Copy, Debug)]
struct ProofInstruction {
    len: usize,
    flow: ProofFlow,
    write: Option<(usize, ProofValue)>,
}

fn prove_seed_reaching_definitions(
    bytes: &[u8],
    layout: &CompiledInitializerLayout,
    expected_esi: u32,
    expected_edi: u32,
) -> Result<(), CompiledInitializerError> {
    let mut incoming = vec![None; bytes.len() + 1];
    let mut instruction_owner = vec![None; bytes.len()];
    incoming[0] = Some(ProofState::UNKNOWN);
    let mut work = vec![0_usize];
    while let Some(offset) = work.pop() {
        let Some(mut state) = incoming[offset] else {
            continue;
        };
        if offset == bytes.len() {
            continue;
        }
        let address = layout.zero_seed_va + offset as u32;
        if instruction_owner[offset].is_some_and(|owner| owner != offset) {
            return Err(CompiledInitializerError::Seed {
                address,
                detail: "control flow targets the interior of an instruction",
            });
        }
        let instruction =
            decode_proof_instruction(bytes, offset, address, &layout.seed_call_targets)
                .map_err(|detail| CompiledInitializerError::Seed { address, detail })?;
        if let Some((register, value)) = instruction.write {
            if (register == 6 && address > layout.esi_seed_va)
                || (register == 7 && address > layout.edi_seed_va)
            {
                return Err(CompiledInitializerError::Seed {
                    address,
                    detail: "seed register is clobbered before dispatch",
                });
            }
            state.set_register(register, value);
            if register == 6 && address == layout.esi_seed_va {
                state.esi_seed_seen = true;
            }
            if register == 7 && address == layout.edi_seed_va {
                state.edi_seed_seen = true;
            }
        }
        let next = offset
            .checked_add(instruction.len)
            .ok_or(CompiledInitializerError::Layout(
                "seed proof offset overflow",
            ))?;
        for owner in &mut instruction_owner[offset..next] {
            if owner.is_some_and(|existing| existing != offset) {
                return Err(CompiledInitializerError::Seed {
                    address,
                    detail: "overlapping instructions in seed proof",
                });
            }
            *owner = Some(offset);
        }
        let mut successors = [None, None];
        match instruction.flow {
            ProofFlow::Next => successors[0] = Some(next),
            ProofFlow::Conditional(target) => {
                successors = [Some(next), Some(proof_offset(layout, target, bytes.len())?)];
            }
            ProofFlow::Jump(target) => {
                successors[0] = Some(proof_offset(layout, target, bytes.len())?);
            }
        }
        for successor in successors.into_iter().flatten() {
            if successor > bytes.len() {
                return Err(CompiledInitializerError::Seed {
                    address,
                    detail: "control flow exits the bounded seed proof",
                });
            }
            let merged = incoming[successor].map_or(state, |old| old.merge(state));
            if incoming[successor] != Some(merged) {
                incoming[successor] = Some(merged);
                work.push(successor);
            }
        }
    }
    let reached = incoming[bytes.len()].ok_or(CompiledInitializerError::Seed {
        address: layout.dispatch_va,
        detail: "dispatch is unreachable from seed proof start",
    })?;
    let expected = ProofState {
        ebx: ProofValue::Zero,
        esi: ProofValue::Constant(expected_esi),
        edi: ProofValue::Constant(expected_edi),
        esi_seed_seen: true,
        edi_seed_seen: true,
    };
    if reached != expected {
        return Err(CompiledInitializerError::Seed {
            address: layout.dispatch_va,
            detail: "seed constants do not reach dispatch on every admitted path",
        });
    }
    Ok(())
}

fn proof_offset(
    layout: &CompiledInitializerLayout,
    target: u32,
    proof_len: usize,
) -> Result<usize, CompiledInitializerError> {
    let offset = target
        .checked_sub(layout.zero_seed_va)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or(CompiledInitializerError::Seed {
            address: target,
            detail: "branch target precedes the bounded seed proof",
        })?;
    if offset > proof_len {
        return Err(CompiledInitializerError::Seed {
            address: target,
            detail: "branch target exits the bounded seed proof",
        });
    }
    Ok(offset)
}

fn decode_proof_instruction(
    bytes: &[u8],
    offset: usize,
    address: u32,
    call_targets: &[u32; SEED_CALL_TARGET_COUNT],
) -> Result<ProofInstruction, &'static str> {
    let opcode = *bytes.get(offset).ok_or("truncated seed-proof opcode")?;
    let next = |len| ProofInstruction {
        len,
        flow: ProofFlow::Next,
        write: None,
    };
    match opcode {
        0x50..=0x57 => Ok(next(1)),
        0x74 | 0x75 | 0x7c | 0xeb => {
            let displacement =
                i32::from(*bytes.get(offset + 1).ok_or("truncated rel8 branch")? as i8);
            let target = address
                .checked_add(2)
                .and_then(|value| value.checked_add_signed(displacement))
                .ok_or("seed-proof branch target overflow")?;
            Ok(ProofInstruction {
                len: 2,
                flow: if opcode == 0xeb {
                    ProofFlow::Jump(target)
                } else {
                    ProofFlow::Conditional(target)
                },
                write: None,
            })
        }
        0xa1 | 0xa3 => {
            proof_require(bytes, offset, 5)?;
            Ok(next(5))
        }
        0xb8..=0xbf => {
            proof_require(bytes, offset, 5)?;
            let register = usize::from(opcode - 0xb8);
            Ok(ProofInstruction {
                len: 5,
                flow: ProofFlow::Next,
                write: matches!(register, 3 | 6 | 7)
                    .then(|| (register, ProofValue::Constant(read_u32(bytes, offset + 1)))),
            })
        }
        0xe8 => {
            proof_require(bytes, offset, 5)?;
            let target = address
                .checked_add(5)
                .and_then(|value| value.checked_add_signed(read_i32(bytes, offset + 1)))
                .ok_or("seed-proof call target overflow")?;
            if !call_targets.contains(&target) {
                return Err("direct call target is outside the reviewed allowlist");
            }
            // The reviewed function uses 32-bit cdecl-compatible callees;
            // EBX, ESI, and EDI are nonvolatile across every direct call.
            Ok(next(5))
        }
        0x05 | 0x3d => {
            proof_require(bytes, offset, 5)?;
            Ok(next(5))
        }
        0x0f => decode_proof_two_byte(bytes, offset, address),
        0x66 => {
            if bytes.get(offset + 1) != Some(&0x89) {
                return Err("unsupported seed-proof operand-size instruction");
            }
            let (len, rm_register) = proof_modrm_len(bytes, offset + 2)?;
            Ok(ProofInstruction {
                len: 2 + len,
                flow: ProofFlow::Next,
                write: rm_register
                    .filter(|register| matches!(register, 3 | 6 | 7))
                    .map(|register| (register, ProofValue::Unknown)),
            })
        }
        0x03 | 0x33 | 0x39 | 0x3b | 0x85 | 0x88 | 0x89 | 0x8a | 0x8b | 0x8d | 0xf7 => {
            let (modrm_len, rm_register) = proof_modrm_len(bytes, offset + 1)?;
            let modrm = bytes[offset + 1];
            let reg = usize::from((modrm >> 3) & 7);
            let write_register = match opcode {
                0x03 | 0x33 | 0x8b | 0x8d => Some(reg),
                0x8a => proof_byte_parent(reg),
                0x88 => rm_register.and_then(proof_byte_parent),
                0x89 => rm_register,
                0xf7 if matches!(reg, 2 | 3) => rm_register,
                _ => None,
            };
            let value = if opcode == 0x33 && rm_register == Some(reg) {
                ProofValue::Zero
            } else {
                ProofValue::Unknown
            };
            Ok(ProofInstruction {
                len: 1 + modrm_len,
                flow: ProofFlow::Next,
                write: write_register
                    .filter(|register| matches!(register, 3 | 6 | 7))
                    .map(|register| (register, value)),
            })
        }
        0x81 | 0x83 | 0xc6 => {
            let (modrm_len, rm_register) = proof_modrm_len(bytes, offset + 1)?;
            let operation = (bytes[offset + 1] >> 3) & 7;
            if opcode == 0xc6 && operation != 0 {
                return Err("unsupported C6 operation in bounded seed proof");
            }
            let immediate_len = if opcode == 0x81 { 4 } else { 1 };
            proof_require(bytes, offset, 1 + modrm_len + immediate_len)?;
            let write_register = if opcode == 0xc6 {
                rm_register.and_then(proof_byte_parent)
            } else if operation == 7 {
                None
            } else {
                rm_register
            };
            Ok(ProofInstruction {
                len: 1 + modrm_len + immediate_len,
                flow: ProofFlow::Next,
                write: write_register
                    .filter(|register| matches!(register, 3 | 6 | 7))
                    .map(|register| (register, ProofValue::Unknown)),
            })
        }
        _ => Err("unsupported instruction in bounded seed proof"),
    }
}

fn proof_byte_parent(register: usize) -> Option<usize> {
    match register {
        3 | 7 => Some(3),
        _ => None,
    }
}

fn decode_proof_two_byte(
    bytes: &[u8],
    offset: usize,
    address: u32,
) -> Result<ProofInstruction, &'static str> {
    let opcode = *bytes
        .get(offset + 1)
        .ok_or("truncated two-byte seed-proof opcode")?;
    match opcode {
        0x84 | 0x85 => {
            proof_require(bytes, offset, 6)?;
            let target = address
                .checked_add(6)
                .and_then(|value| value.checked_add_signed(read_i32(bytes, offset + 2)))
                .ok_or("seed-proof branch target overflow")?;
            Ok(ProofInstruction {
                len: 6,
                flow: ProofFlow::Conditional(target),
                write: None,
            })
        }
        0xa4 => {
            let (modrm_len, rm_register) = proof_modrm_len(bytes, offset + 2)?;
            proof_require(bytes, offset, 2 + modrm_len + 1)?;
            Ok(ProofInstruction {
                len: 2 + modrm_len + 1,
                flow: ProofFlow::Next,
                write: rm_register
                    .filter(|register| matches!(register, 3 | 6 | 7))
                    .map(|register| (register, ProofValue::Unknown)),
            })
        }
        _ => Err("unsupported two-byte instruction in bounded seed proof"),
    }
}

/// Return the ModRM-plus-addressing length and a register destination when
/// `mod == 3` makes r/m a register rather than memory.
fn proof_modrm_len(bytes: &[u8], offset: usize) -> Result<(usize, Option<usize>), &'static str> {
    let modrm = *bytes.get(offset).ok_or("truncated seed-proof ModRM")?;
    let mode = modrm >> 6;
    let rm = usize::from(modrm & 7);
    if mode == 3 {
        return Ok((1, Some(rm)));
    }
    let mut len = 1;
    if rm == 4 {
        let sib = *bytes.get(offset + len).ok_or("truncated seed-proof SIB")?;
        len += 1;
        if mode == 0 && sib & 7 == 5 {
            len += 4;
        }
    }
    len += match mode {
        0 if rm == 5 => 4,
        0 => 0,
        1 => 1,
        2 => 4,
        _ => unreachable!(),
    };
    proof_require(bytes, offset, len)?;
    Ok((len, None))
}

fn proof_require(bytes: &[u8], offset: usize, len: usize) -> Result<(), &'static str> {
    if offset.checked_add(len).is_none_or(|end| end > bytes.len()) {
        Err("truncated instruction in bounded seed proof")
    } else {
        Ok(())
    }
}

fn decode_dispatch(
    image: &Pe32Image<'_>,
    layout: &CompiledInitializerLayout,
) -> Result<(), CompiledInitializerError> {
    let bytes = image.read_code_va_range(layout.dispatch_va, 19)?;
    if bytes[..6] != [0x8b, 0x45, 0x14, 0x83, 0xf8, 0x0d] {
        return Err(CompiledInitializerError::Dispatch {
            address: layout.dispatch_va,
            detail: "wrong selector load or bound",
        });
    }
    if bytes[6..8] != [0x0f, 0x87] {
        return Err(CompiledInitializerError::Dispatch {
            address: layout.dispatch_va + 6,
            detail: "expected unsigned JA",
        });
    }
    if rel_target(layout.dispatch_va + 6, 6, read_i32(bytes, 8))? != layout.common_join_va {
        return Err(CompiledInitializerError::Dispatch {
            address: layout.dispatch_va + 6,
            detail: "wrong default join",
        });
    }
    if bytes[12..15] != [0xff, 0x24, 0x85] || read_u32(bytes, 15) != layout.jump_table_va {
        return Err(CompiledInitializerError::Dispatch {
            address: layout.dispatch_va + 12,
            detail: "wrong indexed table jump",
        });
    }
    Ok(())
}

fn decode_body(
    body: &[u8],
    start: u32,
    selector: u8,
    final_body: bool,
    mut registers: Registers,
    layout: &CompiledInitializerLayout,
) -> Result<CompiledCaseWords, CompiledInitializerError> {
    let mut cursor = 0;
    let mut words = [0_u32; COMPILED_WORD_COUNT];
    let mut store_count = 0;
    let mut pointer_loads = 0;
    let mut literal_loads = 0;
    let mut pending = None;
    let mut terminated = false;

    while cursor < body.len() {
        let address = start + cursor as u32;
        if body[cursor] == 0xe9 {
            if final_body {
                return case_error(selector, address, "last physical body must fall through");
            }
            require_body(body, cursor, 5, selector, start)?;
            if rel_target(address, 5, read_i32(body, cursor + 1))? != layout.common_join_va
                || cursor + 5 != body.len()
            {
                return case_error(selector, address, "jump does not terminate at common join");
            }
            cursor += 5;
            terminated = true;
            break;
        }
        match body[cursor] {
            0xa1 => {
                require_body(body, cursor, 5, selector, start)?;
                set_pointer(
                    &mut registers,
                    &mut pending,
                    0,
                    read_u32(body, cursor + 1),
                    layout,
                    selector,
                    address,
                )?;
                pointer_loads += 1;
                cursor += 5;
            }
            0x8b => {
                require_body(body, cursor, 6, selector, start)?;
                let register = match body[cursor + 1] {
                    0x0d => 1,
                    0x15 => 2,
                    _ => return case_error(selector, address, "unsupported pointer load"),
                };
                set_pointer(
                    &mut registers,
                    &mut pending,
                    register,
                    read_u32(body, cursor + 2),
                    layout,
                    selector,
                    address,
                )?;
                pointer_loads += 1;
                cursor += 6;
            }
            0xb8 | 0xb9 => {
                require_body(body, cursor, 5, selector, start)?;
                let register = usize::from(body[cursor] - 0xb8);
                if pending.is_some_and(|(base, _)| base == register) {
                    return case_error(selector, address, "literal clobbers pending base");
                }
                registers.0[register] = RegisterValue::Constant(read_u32(body, cursor + 1));
                literal_loads += 1;
                cursor += 5;
            }
            0xc7 | 0x89 => {
                let (len, base_register, displacement, value) = decode_store(
                    body,
                    cursor,
                    selector,
                    start,
                    &registers,
                    body[cursor] == 0xc7,
                )?;
                let RegisterValue::Base(base) = registers.0[base_register] else {
                    return store_error(selector, address, store_count, "unproven base");
                };
                if pending != Some((base_register, base)) {
                    return store_error(
                        selector,
                        address,
                        store_count,
                        "store does not consume pending pointer",
                    );
                }
                validate_destination(layout, selector, address, store_count, base, displacement)?;
                words[store_count] = value;
                store_count += 1;
                pending = None;
                cursor += len;
            }
            _ => return case_error(selector, address, "unsupported instruction"),
        }
    }
    if cursor != body.len() {
        return case_error(selector, start + cursor as u32, "unconsumed bytes");
    }
    if terminated == final_body {
        return case_error(
            selector,
            start + cursor as u32,
            "wrong jump/fallthrough termination",
        );
    }
    if pending.is_some()
        || pointer_loads != COMPILED_WORD_COUNT
        || store_count != COMPILED_WORD_COUNT
    {
        return case_error(
            selector,
            start + cursor as u32,
            "expected exactly 31 load/store pairs",
        );
    }
    if !(1..=2).contains(&literal_loads) {
        return case_error(
            selector,
            start + cursor as u32,
            "expected one or two literal loads",
        );
    }
    Ok(CompiledCaseWords {
        state_words: std::array::from_fn(|index| {
            let sequence_index = if index < 3 {
                index
            } else {
                COMPILED_WORD_COUNT - COMPILED_STATE_WORD_COUNT + index
            };
            words[sequence_index] as i32
        }),
        control_words: std::array::from_fn(|index| words[3 + index] as i32),
    })
}

fn set_pointer(
    registers: &mut Registers,
    pending: &mut Option<(usize, BaseTag)>,
    register: usize,
    pointer_va: u32,
    layout: &CompiledInitializerLayout,
    selector: u8,
    address: u32,
) -> Result<(), CompiledInitializerError> {
    if pending.is_some() {
        return case_error(selector, address, "pointer load before pending store");
    }
    let base = if pointer_va == layout.state_pointer_va {
        BaseTag::State
    } else if pointer_va == layout.control_pointer_va {
        BaseTag::Control
    } else {
        return case_error(selector, address, "unknown pointer global");
    };
    registers.0[register] = RegisterValue::Base(base);
    *pending = Some((register, base));
    Ok(())
}

fn decode_store(
    body: &[u8],
    cursor: usize,
    selector: u8,
    start: u32,
    registers: &Registers,
    immediate: bool,
) -> Result<(usize, usize, i32, u32), CompiledInitializerError> {
    require_body(body, cursor, 2, selector, start)?;
    let modrm = body[cursor + 1];
    let mode = modrm >> 6;
    let source = usize::from((modrm >> 3) & 7);
    let base = usize::from(modrm & 7);
    if !matches!(mode, 1 | 2) || base > 2 || (immediate && source != 0) {
        return case_error(selector, start + cursor as u32, "unsupported store form");
    }
    let disp_len = if mode == 1 { 1 } else { 4 };
    require_body(body, cursor, 2 + disp_len, selector, start)?;
    let displacement = if mode == 1 {
        i32::from(body[cursor + 2] as i8)
    } else {
        read_i32(body, cursor + 2)
    };
    let value_offset = cursor + 2 + disp_len;
    if immediate {
        require_body(body, value_offset, 4, selector, start)?;
        Ok((
            2 + disp_len + 4,
            base,
            displacement,
            read_u32(body, value_offset),
        ))
    } else {
        if !matches!(source, 0 | 1 | 3 | 6 | 7) {
            return case_error(
                selector,
                start + cursor as u32,
                "unsupported value register",
            );
        }
        let RegisterValue::Constant(value) = registers.0[source] else {
            return case_error(
                selector,
                start + cursor as u32,
                "value register is not constant",
            );
        };
        Ok((2 + disp_len, base, displacement, value))
    }
}

fn validate_destination(
    layout: &CompiledInitializerLayout,
    selector: u8,
    address: u32,
    index: usize,
    base: BaseTag,
    displacement: i32,
) -> Result<(), CompiledInitializerError> {
    let expected = if index < 3 {
        (BaseTag::State, i32::from(layout.state_offsets[index]))
    } else if index < 3 + COMPILED_CONTROL_WORD_COUNT {
        (
            BaseTag::Control,
            i32::from(layout.control_offsets[index - 3]),
        )
    } else if index < COMPILED_WORD_COUNT {
        (
            BaseTag::State,
            i32::from(layout.state_offsets[index - COMPILED_CONTROL_WORD_COUNT]),
        )
    } else {
        return store_error(selector, address, index, "extra store");
    };
    if base != expected.0 {
        return store_error(selector, address, index, "wrong destination object");
    }
    if displacement != expected.1 {
        return store_error(selector, address, index, "wrong or reordered offset");
    }
    if displacement & 3 != 0 {
        return store_error(selector, address, index, "unaligned offset");
    }
    Ok(())
}

fn require_body(
    body: &[u8],
    offset: usize,
    needed: usize,
    selector: u8,
    start: u32,
) -> Result<(), CompiledInitializerError> {
    if offset
        .checked_add(needed)
        .is_none_or(|end| end > body.len())
    {
        return case_error(selector, start + offset as u32, "truncated instruction");
    }
    Ok(())
}

fn rel_target(address: u32, len: u32, displacement: i32) -> Result<u32, CompiledInitializerError> {
    address
        .checked_add(len)
        .and_then(|next| next.checked_add_signed(displacement))
        .ok_or(CompiledInitializerError::Layout("relative target overflow"))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated read"),
    )
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated read"),
    )
}

fn case_error<T>(
    selector: u8,
    address: u32,
    detail: &'static str,
) -> Result<T, CompiledInitializerError> {
    Err(CompiledInitializerError::Case {
        selector,
        address,
        detail,
    })
}

fn store_error<T>(
    selector: u8,
    address: u32,
    store_index: usize,
    detail: &'static str,
) -> Result<T, CompiledInitializerError> {
    Err(CompiledInitializerError::Store {
        selector,
        address,
        store_index,
        detail,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompiledCaseCodecError {
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

impl fmt::Display for CompiledCaseCodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid CMRCCASE data: {self:?}")
    }
}

impl Error for CompiledCaseCodecError {}

#[must_use]
pub fn encode_compiled_cases(
    cases: &[CompiledCaseWords; COMPILED_CASE_COUNT],
) -> [u8; COMPILED_CASE_ENCODED_LEN] {
    let mut output = [0_u8; COMPILED_CASE_ENCODED_LEN];
    output[..8].copy_from_slice(&COMPILED_CASE_MAGIC);
    for (offset, value) in [
        (8, COMPILED_CASE_FORMAT_VERSION),
        (12, COMPILED_CASE_COUNT as u32),
        (16, COMPILED_STATE_WORD_COUNT as u32),
        (20, COMPILED_CONTROL_WORD_COUNT as u32),
    ] {
        output[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut cursor = 24;
    for case in cases {
        for word in case.state_words.into_iter().chain(case.control_words) {
            output[cursor..cursor + 4].copy_from_slice(&word.to_le_bytes());
            cursor += 4;
        }
    }
    output
}

pub fn decode_compiled_cases(
    input: &[u8],
) -> Result<[CompiledCaseWords; COMPILED_CASE_COUNT], CompiledCaseCodecError> {
    if input.len() != COMPILED_CASE_ENCODED_LEN {
        return Err(CompiledCaseCodecError::WrongLength {
            expected: COMPILED_CASE_ENCODED_LEN,
            actual: input.len(),
        });
    }
    if input[..8] != COMPILED_CASE_MAGIC {
        return Err(CompiledCaseCodecError::WrongMagic);
    }
    let version = read_u32(input, 8);
    if version != COMPILED_CASE_FORMAT_VERSION {
        return Err(CompiledCaseCodecError::UnsupportedVersion { version });
    }
    for (offset, field, expected) in [
        (12, "selector count", COMPILED_CASE_COUNT as u32),
        (16, "state word count", COMPILED_STATE_WORD_COUNT as u32),
        (20, "control word count", COMPILED_CONTROL_WORD_COUNT as u32),
    ] {
        let actual = read_u32(input, offset);
        if actual != expected {
            return Err(CompiledCaseCodecError::WrongCount {
                field,
                expected,
                actual,
            });
        }
    }
    let mut cursor = 24;
    Ok(std::array::from_fn(|_| {
        let state_words = std::array::from_fn(|_| {
            let value = read_i32(input, cursor);
            cursor += 4;
            value
        });
        let control_words = std::array::from_fn(|_| {
            let value = read_i32(input, cursor);
            cursor += 4;
            value
        });
        CompiledCaseWords {
            state_words,
            control_words,
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_BASE: u32 = 0x0040_0000;
    const SECTION_RVA: u32 = 0x1000;
    const CODE_VA: u32 = IMAGE_BASE + SECTION_RVA;
    const RAW_OFFSET: usize = 0x200;
    const PE_OFFSET: usize = 0x80;
    const OPTIONAL_SIZE: usize = 0xe0;
    const SECTION_TABLE: usize = PE_OFFSET + 4 + 20 + OPTIONAL_SIZE;
    const PHYSICAL_ORDER: [u8; COMPILED_CASE_COUNT] =
        [0, 5, 4, 2, 1, 3, 8, 7, 6, 11, 10, 9, 12, 13];

    struct Fixture {
        bytes: Vec<u8>,
        layout: CompiledInitializerLayout,
        expected: [CompiledCaseWords; COMPILED_CASE_COUNT],
        targets: [u32; COMPILED_CASE_COUNT],
    }

    fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn file_offset(va: u32) -> usize {
        RAW_OFFSET + usize::try_from(va - CODE_VA).unwrap()
    }

    fn add_second_section(
        bytes: &mut [u8],
        rva: u32,
        size: u32,
        raw_offset: u32,
        characteristics: u32,
    ) {
        put_u16(bytes, PE_OFFSET + 6, 2);
        let header = SECTION_TABLE + 40;
        put_u32(bytes, header + 8, size);
        put_u32(bytes, header + 12, rva);
        put_u32(bytes, header + 16, size);
        put_u32(bytes, header + 20, raw_offset);
        put_u32(bytes, header + 36, characteristics);
    }

    fn emit_pointer(body: &mut Vec<u8>, register: usize, pointer_va: u32) {
        match register {
            0 => body.push(0xa1),
            1 => body.extend_from_slice(&[0x8b, 0x0d]),
            2 => body.extend_from_slice(&[0x8b, 0x15]),
            _ => unreachable!(),
        }
        body.extend_from_slice(&pointer_va.to_le_bytes());
    }

    fn emit_register_store(body: &mut Vec<u8>, base: usize, source: usize, offset: u16) {
        let mode = if offset <= 0x7f { 1 } else { 2 };
        body.extend_from_slice(&[0x89, (mode << 6) | ((source as u8) << 3) | base as u8]);
        if mode == 1 {
            body.push(offset as u8);
        } else {
            body.extend_from_slice(&u32::from(offset).to_le_bytes());
        }
    }

    fn emit_immediate_store(body: &mut Vec<u8>, base: usize, offset: u16, value: i32) {
        let mode = if offset <= 0x7f { 1 } else { 2 };
        body.extend_from_slice(&[0xc7, (mode << 6) | base as u8]);
        if mode == 1 {
            body.push(offset as u8);
        } else {
            body.extend_from_slice(&u32::from(offset).to_le_bytes());
        }
        body.extend_from_slice(&value.to_le_bytes());
    }

    fn synthetic_pairs(bytes: &[u8], start: u32, end: u32) -> Vec<(usize, usize)> {
        let mut cursor = file_offset(start);
        let end = file_offset(end);
        let mut pointer = None;
        let mut pairs = Vec::new();
        while cursor < end {
            match bytes[cursor] {
                0xa1 => {
                    pointer = Some(cursor);
                    cursor += 5;
                }
                0x8b => {
                    pointer = Some(cursor);
                    cursor += 6;
                }
                0xb8 | 0xb9 => cursor += 5,
                0x89 | 0xc7 => {
                    let store = cursor;
                    let mode = bytes[cursor + 1] >> 6;
                    cursor += if bytes[store] == 0xc7 {
                        if mode == 1 { 7 } else { 10 }
                    } else if mode == 1 {
                        3
                    } else {
                        6
                    };
                    pairs.push((pointer.take().unwrap(), store));
                }
                0xe9 => break,
                opcode => panic!("unexpected synthetic opcode {opcode:#x}"),
            }
        }
        pairs
    }

    fn synthetic_body(
        selector: u8,
        state_pointer_va: u32,
        control_pointer_va: u32,
    ) -> (Vec<u8>, CompiledCaseWords) {
        let mut body = Vec::new();
        let eax_word = i32::from(selector) * 10_000 - 777;
        let ecx_word = i32::from(selector) * -10_000 + 333;
        body.push(0xb8);
        body.extend_from_slice(&eax_word.to_le_bytes());
        let mut values = [0_i32; COMPILED_WORD_COUNT];
        for index in 0..COMPILED_WORD_COUNT {
            let (base_tag, offset) = if index < 3 {
                (BaseTag::State, COMPILED_STATE_WORD_OFFSETS[index])
            } else if index < 3 + COMPILED_CONTROL_WORD_COUNT {
                (BaseTag::Control, COMPILED_CONTROL_WORD_OFFSETS[index - 3])
            } else {
                (
                    BaseTag::State,
                    COMPILED_STATE_WORD_OFFSETS[index - COMPILED_CONTROL_WORD_COUNT],
                )
            };
            let pointer = if base_tag == BaseTag::State {
                state_pointer_va
            } else {
                control_pointer_va
            };
            match index {
                0 => {
                    emit_pointer(&mut body, 1, pointer);
                    emit_register_store(&mut body, 1, 0, offset);
                    values[index] = eax_word;
                }
                1 => {
                    emit_pointer(&mut body, 2, pointer);
                    emit_register_store(&mut body, 2, 6, offset);
                    values[index] = i32::MIN;
                }
                2 => {
                    emit_pointer(&mut body, 1, pointer);
                    emit_register_store(&mut body, 1, 7, offset);
                    values[index] = i32::MAX;
                }
                3 => {
                    body.push(0xb9);
                    body.extend_from_slice(&ecx_word.to_le_bytes());
                    emit_pointer(&mut body, 2, pointer);
                    emit_register_store(&mut body, 2, 1, offset);
                    values[index] = ecx_word;
                }
                4 => {
                    emit_pointer(&mut body, 0, pointer);
                    emit_register_store(&mut body, 0, 3, offset);
                    values[index] = 0;
                }
                _ => {
                    let base = index % 3;
                    let value = i32::from(selector) * 1_000 + index as i32 - 500;
                    emit_pointer(&mut body, base, pointer);
                    emit_immediate_store(&mut body, base, offset, value);
                    values[index] = value;
                }
            }
        }
        (
            body,
            CompiledCaseWords {
                state_words: std::array::from_fn(|index| {
                    values[if index < 3 {
                        index
                    } else {
                        COMPILED_WORD_COUNT - COMPILED_STATE_WORD_COUNT + index
                    }]
                }),
                control_words: std::array::from_fn(|index| values[3 + index]),
            },
        )
    }

    fn fixture() -> Fixture {
        let zero_seed_va = CODE_VA;
        let seed_call_targets =
            std::array::from_fn(|index| IMAGE_BASE + 0x0010_0000 + index as u32 * 0x100);
        let esi_seed_va = CODE_VA + 0x30;
        let edi_seed_va = CODE_VA + 0x35;
        let dispatch_va = CODE_VA + 0x3c;
        let first_case_va = CODE_VA + 0x100;
        let jump_table_va = CODE_VA + 0x3000;
        let state_pointer_va = IMAGE_BASE + 0x9000;
        let control_pointer_va = IMAGE_BASE + 0x9010;
        let mut bodies = Vec::new();
        let mut expected = [CompiledCaseWords::default(); COMPILED_CASE_COUNT];
        for (selector, expected_case) in expected.iter_mut().enumerate() {
            let (body, words) =
                synthetic_body(selector as u8, state_pointer_va, control_pointer_va);
            bodies.push(body);
            *expected_case = words;
        }
        let mut targets = [0_u32; COMPILED_CASE_COUNT];
        let mut next_va = first_case_va;
        for selector in PHYSICAL_ORDER {
            targets[selector as usize] = next_va;
            next_va += bodies[selector as usize].len() as u32;
            if selector != 13 {
                next_va += 5;
            }
        }
        let common_join_va = next_va;
        let raw_size = 0x3400_usize;
        let mut bytes = vec![0_u8; RAW_OFFSET + raw_size];
        bytes[..2].copy_from_slice(b"MZ");
        put_u32(&mut bytes, 0x3c, PE_OFFSET as u32);
        bytes[PE_OFFSET..PE_OFFSET + 4].copy_from_slice(b"PE\0\0");
        put_u16(&mut bytes, PE_OFFSET + 4, 0x14c);
        put_u16(&mut bytes, PE_OFFSET + 6, 1);
        put_u16(&mut bytes, PE_OFFSET + 20, OPTIONAL_SIZE as u16);
        put_u16(&mut bytes, PE_OFFSET + 24, 0x10b);
        put_u32(&mut bytes, PE_OFFSET + 24 + 28, IMAGE_BASE);
        put_u32(&mut bytes, SECTION_TABLE + 8, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 12, SECTION_RVA);
        put_u32(&mut bytes, SECTION_TABLE + 16, raw_size as u32);
        put_u32(&mut bytes, SECTION_TABLE + 20, RAW_OFFSET as u32);
        put_u32(&mut bytes, SECTION_TABLE + 36, 0x6000_0020);

        let proof_offset = file_offset(zero_seed_va);
        bytes[proof_offset..proof_offset + 11].copy_from_slice(&[
            0x33, 0xdb, // XOR EBX,EBX: dominating definition
            0x74, 0x07, // bypass the clobber/re-zero arm
            0xbb, 1, 2, 3, 4, // MOV EBX,imm32: clobber on the other arm
            0x33, 0xdb, // XOR EBX,EBX: restore before the merge
        ]);
        for (index, target) in seed_call_targets.iter().copied().enumerate() {
            let call_va = zero_seed_va + 11 + index as u32 * 5;
            let call_offset = file_offset(call_va);
            bytes[call_offset] = 0xe8;
            let displacement = (i64::from(target) - i64::from(call_va + 5)) as i32;
            put_u32(&mut bytes, call_offset + 1, displacement as u32);
        }
        let duplicate_offset = proof_offset + 41;
        bytes[duplicate_offset] = 0xb8;
        put_u32(&mut bytes, duplicate_offset + 1, i32::MIN as u32);
        bytes[duplicate_offset + 5..duplicate_offset + 7].copy_from_slice(&[0x89, 0xc0]);
        let esi_offset = file_offset(esi_seed_va);
        bytes[esi_offset] = 0xbe;
        put_u32(&mut bytes, esi_offset + 1, i32::MIN as u32);
        let edi_offset = file_offset(edi_seed_va);
        bytes[edi_offset] = 0xbf;
        put_u32(&mut bytes, edi_offset + 1, i32::MAX as u32);
        bytes[edi_offset + 5..edi_offset + 7].copy_from_slice(&[0x89, 0xc0]);

        let dispatch_offset = file_offset(dispatch_va);
        bytes[dispatch_offset..dispatch_offset + 19].copy_from_slice(&[
            0x8b, 0x45, 0x14, 0x83, 0xf8, 0x0d, 0x0f, 0x87, 0, 0, 0, 0, 0xff, 0x24, 0x85, 0, 0, 0,
            0,
        ]);
        let default_displacement = (i64::from(common_join_va) - i64::from(dispatch_va + 12)) as i32;
        put_u32(&mut bytes, dispatch_offset + 8, default_displacement as u32);
        put_u32(&mut bytes, dispatch_offset + 15, jump_table_va);
        for (selector, target) in targets.iter().copied().enumerate() {
            put_u32(
                &mut bytes,
                file_offset(jump_table_va) + selector * 4,
                target,
            );
        }
        for (physical_index, selector) in PHYSICAL_ORDER.into_iter().enumerate() {
            let target = targets[selector as usize];
            let offset = file_offset(target);
            let body = &bodies[selector as usize];
            bytes[offset..offset + body.len()].copy_from_slice(body);
            if physical_index + 1 != COMPILED_CASE_COUNT {
                bytes[offset + body.len()] = 0xe9;
                let jump_va = target + body.len() as u32;
                let displacement = (i64::from(common_join_va) - i64::from(jump_va + 5)) as i32;
                put_u32(&mut bytes, offset + body.len() + 1, displacement as u32);
            }
        }
        Fixture {
            bytes,
            layout: CompiledInitializerLayout {
                image_base: IMAGE_BASE,
                zero_seed_va,
                esi_seed_va,
                edi_seed_va,
                seed_call_targets,
                dispatch_va,
                jump_table_va,
                first_case_va,
                common_join_va,
                state_pointer_va,
                control_pointer_va,
                state_offsets: COMPILED_STATE_WORD_OFFSETS,
                control_offsets: COMPILED_CONTROL_WORD_OFFSETS,
            },
            expected,
            targets,
        }
    }

    #[test]
    fn extracts_nonphysical_selector_order_and_all_instruction_forms() {
        let fixture = fixture();
        let actual = extract_compiled_initializer_cases(&fixture.bytes, &fixture.layout).unwrap();
        assert_eq!(actual, fixture.expected);
        assert_eq!(actual[0].state_words[1], i32::MIN);
        assert_eq!(actual[13].state_words[2], i32::MAX);
        assert_eq!(actual[7].control_words[1], 0);
    }

    #[test]
    fn codec_is_exact_length_versioned_and_signed_lossless() {
        let fixture = fixture();
        let encoded = encode_compiled_cases(&fixture.expected);
        assert_eq!(encoded.len(), 1_760);
        assert_eq!(decode_compiled_cases(&encoded).unwrap(), fixture.expected);
        assert_eq!(
            decode_compiled_cases(&encoded[..encoded.len() - 1]),
            Err(CompiledCaseCodecError::WrongLength {
                expected: 1_760,
                actual: 1_759,
            })
        );
        let mut trailing = encoded.to_vec();
        trailing.push(0);
        assert_eq!(
            decode_compiled_cases(&trailing),
            Err(CompiledCaseCodecError::WrongLength {
                expected: 1_760,
                actual: 1_761,
            })
        );
        let mut changed = encoded;
        changed[0] ^= 1;
        assert_eq!(
            decode_compiled_cases(&changed),
            Err(CompiledCaseCodecError::WrongMagic)
        );
        let mut changed = encoded;
        put_u32(&mut changed, 8, 2);
        assert_eq!(
            decode_compiled_cases(&changed),
            Err(CompiledCaseCodecError::UnsupportedVersion { version: 2 })
        );
        for (offset, field, expected) in [
            (12, "selector count", 14),
            (16, "state word count", 8),
            (20, "control word count", 23),
        ] {
            let mut changed = encoded;
            put_u32(&mut changed, offset, expected + 1);
            assert_eq!(
                decode_compiled_cases(&changed),
                Err(CompiledCaseCodecError::WrongCount {
                    field,
                    expected,
                    actual: expected + 1,
                })
            );
        }
        let decoded = decode_compiled_cases(&encoded).unwrap();
        assert_eq!(decoded[0].state_words[1], i32::MIN);
        assert_eq!(decoded[13].state_words[2], i32::MAX);
        assert_ne!(decoded[0], decoded[13]);
    }

    #[test]
    fn rejects_dispatch_and_table_mutations() {
        let fixture = fixture();
        let dispatch = file_offset(fixture.layout.dispatch_va);
        for bound in [12_u8, 14] {
            let mut bytes = fixture.bytes.clone();
            bytes[dispatch + 5] = bound;
            assert!(matches!(
                extract_compiled_initializer_cases(&bytes, &fixture.layout),
                Err(CompiledInitializerError::Dispatch { .. })
            ));
        }
        let mut signed = fixture.bytes.clone();
        signed[dispatch + 7] = 0x8f;
        assert!(matches!(
            extract_compiled_initializer_cases(&signed, &fixture.layout),
            Err(CompiledInitializerError::Dispatch { .. })
        ));
        for delta in [8_usize, 14, 15] {
            let mut bytes = fixture.bytes.clone();
            bytes[dispatch + delta] ^= 1;
            assert!(matches!(
                extract_compiled_initializer_cases(&bytes, &fixture.layout),
                Err(CompiledInitializerError::Dispatch { .. })
            ));
        }
        let mut duplicate = fixture.bytes.clone();
        put_u32(
            &mut duplicate,
            file_offset(fixture.layout.jump_table_va) + 4,
            fixture.targets[0],
        );
        assert!(matches!(
            extract_compiled_initializer_cases(&duplicate, &fixture.layout),
            Err(CompiledInitializerError::TableTarget {
                detail: "duplicate target",
                ..
            })
        ));
        for target in [
            fixture.layout.first_case_va - 1,
            fixture.layout.common_join_va,
            fixture.targets[0] + 1,
        ] {
            let mut bytes = fixture.bytes.clone();
            put_u32(
                &mut bytes,
                file_offset(fixture.layout.jump_table_va),
                target,
            );
            assert!(extract_compiled_initializer_cases(&bytes, &fixture.layout).is_err());
        }
    }

    #[test]
    fn jump_table_requires_unique_executable_mapping() {
        let fixture = fixture();
        let table_rva = fixture.layout.jump_table_va - IMAGE_BASE;
        let table_raw = file_offset(fixture.layout.jump_table_va) as u32;

        let mut noncode = fixture.bytes.clone();
        put_u32(&mut noncode, SECTION_TABLE + 8, 0x2f00);
        put_u32(&mut noncode, SECTION_TABLE + 16, 0x2f00);
        add_second_section(&mut noncode, table_rva, 0x100, table_raw, 0x4000_0040);
        assert!(matches!(
            extract_compiled_initializer_cases(&noncode, &fixture.layout),
            Err(CompiledInitializerError::Pe(
                CompiledInitializerPeError::SectionNotExecutable { .. }
            ))
        ));

        let mut ambiguous = fixture.bytes.clone();
        add_second_section(&mut ambiguous, table_rva, 0x100, table_raw, 0x6000_0020);
        assert!(matches!(
            extract_compiled_initializer_cases(&ambiguous, &fixture.layout),
            Err(CompiledInitializerError::Pe(
                CompiledInitializerPeError::AmbiguousRange { .. }
            ))
        ));
    }

    #[test]
    fn rejects_seed_and_section_permission_mutations() {
        let fixture = fixture();
        let mut seed = fixture.bytes.clone();
        seed[file_offset(fixture.layout.zero_seed_va)] = 0x90;
        assert!(matches!(
            extract_compiled_initializer_cases(&seed, &fixture.layout),
            Err(CompiledInitializerError::Seed { .. })
        ));

        // The branch at +2 bypasses the later re-zero but retains the
        // dominating zero from +0. The alternate path must restore EBX.
        let mut missing_rezero = fixture.bytes.clone();
        missing_rezero[file_offset(fixture.layout.zero_seed_va) + 9
            ..file_offset(fixture.layout.zero_seed_va) + 11]
            .copy_from_slice(&[0x89, 0xc0]);
        assert!(matches!(
            extract_compiled_initializer_cases(&missing_rezero, &fixture.layout),
            Err(CompiledInitializerError::Seed { .. })
        ));

        let mut post_seed_clobber = fixture.bytes.clone();
        post_seed_clobber[file_offset(fixture.layout.edi_seed_va) + 6] = 0xc6;
        assert!(matches!(
            extract_compiled_initializer_cases(&post_seed_clobber, &fixture.layout),
            Err(CompiledInitializerError::Seed { .. })
        ));
        let mut noncode = fixture.bytes.clone();
        put_u32(&mut noncode, SECTION_TABLE + 36, 0x4000_0040);
        assert!(matches!(
            extract_compiled_initializer_cases(&noncode, &fixture.layout),
            Err(CompiledInitializerError::Pe(
                CompiledInitializerPeError::SectionNotExecutable { .. }
            ))
        ));
    }

    #[test]
    fn seed_proof_accepts_a_bypass_of_only_the_later_rezero() {
        let fixture = fixture();
        // The synthetic conditional edge jumps over an EBX clobber and the
        // later re-zero. The dominating entry XOR still reaches both arms.
        assert_eq!(
            extract_compiled_initializer_cases(&fixture.bytes, &fixture.layout).unwrap(),
            fixture.expected
        );
    }

    #[test]
    fn seed_proof_rejects_same_value_from_an_unprofiled_origin() {
        let fixture = fixture();
        let proof = file_offset(fixture.layout.zero_seed_va);
        let mut bytes = fixture.bytes.clone();
        bytes[proof + 41] = 0xbe; // Duplicate MOV ESI with the same immediate.
        bytes[proof + 46..proof + 48].copy_from_slice(&[0xeb, 0x05]);
        assert!(matches!(
            extract_compiled_initializer_cases(&bytes, &fixture.layout),
            Err(CompiledInitializerError::Seed {
                address,
                detail: "seed constants do not reach dispatch on every admitted path",
            }) if address == fixture.layout.dispatch_va
        ));
    }

    #[test]
    fn seed_proof_rejects_a_call_retargeted_inside_the_local_graph() {
        let fixture = fixture();
        let call_va = fixture.layout.zero_seed_va + 11;
        let call_offset = file_offset(call_va);
        let mut bytes = fixture.bytes.clone();
        let local_interior = fixture.layout.zero_seed_va + 1;
        let displacement = (i64::from(local_interior) - i64::from(call_va + 5)) as i32;
        put_u32(&mut bytes, call_offset + 1, displacement as u32);
        assert!(matches!(
            extract_compiled_initializer_cases(&bytes, &fixture.layout),
            Err(CompiledInitializerError::Seed {
                detail: "direct call target is outside the reviewed allowlist",
                ..
            })
        ));
    }

    #[test]
    fn rejects_store_and_instruction_mutations_atomically() {
        let fixture = fixture();
        let first = file_offset(fixture.targets[0]);
        let pairs = synthetic_pairs(&fixture.bytes, fixture.targets[0], fixture.targets[5]);
        let mut wrong_offset = fixture.bytes.clone();
        wrong_offset[first + 13] = 0x24;
        assert!(matches!(
            extract_compiled_initializer_cases(&wrong_offset, &fixture.layout),
            Err(CompiledInitializerError::Store {
                selector: 0,
                store_index: 0,
                ..
            })
        ));
        let mut unknown_source = fixture.bytes.clone();
        unknown_source[first + 12] = 0x51;
        assert!(matches!(
            extract_compiled_initializer_cases(&unknown_source, &fixture.layout),
            Err(CompiledInitializerError::Case { selector: 0, .. })
        ));
        let mut unsupported = fixture.bytes.clone();
        unsupported[first] = 0x90;
        assert!(extract_compiled_initializer_cases(&unsupported, &fixture.layout).is_err());

        let mut duplicate = fixture.bytes.clone();
        duplicate[pairs[1].1 + 2] = COMPILED_STATE_WORD_OFFSETS[0] as u8;
        assert!(matches!(
            extract_compiled_initializer_cases(&duplicate, &fixture.layout),
            Err(CompiledInitializerError::Store { selector: 0, .. })
        ));

        let mut reordered_tail = fixture.bytes.clone();
        let first_tail = reordered_tail[pairs[29].1 + 2..pairs[29].1 + 6].to_vec();
        let second_tail = reordered_tail[pairs[30].1 + 2..pairs[30].1 + 6].to_vec();
        reordered_tail[pairs[29].1 + 2..pairs[29].1 + 6].copy_from_slice(&second_tail);
        reordered_tail[pairs[30].1 + 2..pairs[30].1 + 6].copy_from_slice(&first_tail);
        assert!(matches!(
            extract_compiled_initializer_cases(&reordered_tail, &fixture.layout),
            Err(CompiledInitializerError::Store {
                selector: 0,
                store_index: 29,
                ..
            })
        ));

        let mut wrong_object = fixture.bytes.clone();
        let pointer = pairs[3].0;
        let immediate = if wrong_object[pointer] == 0xa1 {
            pointer + 1
        } else {
            pointer + 2
        };
        put_u32(
            &mut wrong_object,
            immediate,
            fixture.layout.state_pointer_va,
        );
        assert!(matches!(
            extract_compiled_initializer_cases(&wrong_object, &fixture.layout),
            Err(CompiledInitializerError::Store {
                selector: 0,
                store_index: 3,
                ..
            })
        ));

        let mut unaligned = fixture.bytes.clone();
        unaligned[pairs[0].1 + 2] = 0x21;
        assert!(extract_compiled_initializer_cases(&unaligned, &fixture.layout).is_err());

        let mut partial = fixture.bytes.clone();
        partial[pairs[0].1] = 0x88;
        assert!(extract_compiled_initializer_cases(&partial, &fixture.layout).is_err());

        let mut wrong_base = fixture.bytes.clone();
        wrong_base[pairs[0].1 + 1] = (wrong_base[pairs[0].1 + 1] & !7) | 3;
        assert!(extract_compiled_initializer_cases(&wrong_base, &fixture.layout).is_err());

        let mut pending_clobber = fixture.bytes.clone();
        pending_clobber[pairs[0].1] = 0xb9;
        assert!(matches!(
            extract_compiled_initializer_cases(&pending_clobber, &fixture.layout),
            Err(CompiledInitializerError::Case {
                selector: 0,
                detail: "literal clobbers pending base",
                ..
            })
        ));

        let mut memory_source = fixture.bytes.clone();
        memory_source[pairs[0].1] = 0x8b;
        assert!(extract_compiled_initializer_cases(&memory_source, &fixture.layout).is_err());

        let mut memory_derived_value = fixture.bytes.clone();
        memory_derived_value[first] = 0xa1;
        assert!(
            extract_compiled_initializer_cases(&memory_derived_value, &fixture.layout).is_err()
        );

        for opcode in [0xe8, 0x75, 0xc3] {
            let mut control_flow = fixture.bytes.clone();
            control_flow[first] = opcode;
            assert!(extract_compiled_initializer_cases(&control_flow, &fixture.layout).is_err());
        }

        let terminator = file_offset(fixture.targets[5]) - 5;
        let mut fallthrough = fixture.bytes.clone();
        fallthrough[terminator] = 0x89;
        assert!(extract_compiled_initializer_cases(&fallthrough, &fixture.layout).is_err());
        let mut wrong_join = fixture.bytes.clone();
        wrong_join[terminator + 1] ^= 1;
        assert!(extract_compiled_initializer_cases(&wrong_join, &fixture.layout).is_err());

        let mut missing = fixture.bytes.clone();
        put_u32(
            &mut missing,
            file_offset(fixture.layout.jump_table_va) + 5 * 4,
            fixture.targets[5] - 5,
        );
        assert!(extract_compiled_initializer_cases(&missing, &fixture.layout).is_err());
        let mut extra = fixture.bytes.clone();
        put_u32(
            &mut extra,
            file_offset(fixture.layout.jump_table_va) + 5 * 4,
            fixture.targets[5] + 1,
        );
        assert!(extract_compiled_initializer_cases(&extra, &fixture.layout).is_err());

        let mut last = fixture.bytes.clone();
        let last_start = file_offset(fixture.targets[13]);
        last[last_start] = 0xe9;
        let displacement =
            (i64::from(fixture.layout.common_join_va) - i64::from(fixture.targets[13] + 5)) as i32;
        put_u32(&mut last, last_start + 1, displacement as u32);
        assert!(extract_compiled_initializer_cases(&last, &fixture.layout).is_err());
    }

    #[test]
    fn rejects_wrong_image_base_and_schema() {
        let fixture = fixture();
        let mut wrong_base = fixture.bytes.clone();
        put_u32(&mut wrong_base, PE_OFFSET + 24 + 28, IMAGE_BASE + 0x1000);
        assert!(matches!(
            extract_compiled_initializer_cases(&wrong_base, &fixture.layout),
            Err(CompiledInitializerError::ImageBase { .. })
        ));
        let mut layout = fixture.layout;
        layout.state_offsets.swap(0, 1);
        assert_eq!(
            extract_compiled_initializer_cases(&fixture.bytes, &layout),
            Err(CompiledInitializerError::Layout(
                "unsupported destination schema"
            ))
        );
    }
}
