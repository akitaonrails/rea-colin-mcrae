//! Game fixes, applied to the suspended process before it runs.
//!
//! Every fix replaces a few instructions at a known address of one exact
//! executable (see `edition`). The bytes written here are our own code; the
//! launcher checks that memory still matches the file on disk before it
//! writes anything, so a mismatch stops the launch instead of corrupting it.
//! Addresses and the reasons are documented in `docs/THIN_APP_PLAN.md`
//! (inventory F1-F5).

/// One fix: a list of code pieces written at absolute addresses.
#[derive(Debug, PartialEq, Eq)]
pub struct Fix {
    pub id: &'static str,
    pub what: &'static str,
    pub code: &'static [Piece],
}

#[derive(Debug, PartialEq, Eq)]
pub struct FixSet {
    pub fixes: &'static [Fix],
}

/// Where a piece of code goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum At {
    /// An address inside the game image.
    Image(u32),
    /// Our own page allocated in the game process (the "cave"); the value is
    /// the offset inside it.
    Cave(u32),
}

#[derive(Debug, PartialEq, Eq)]
pub struct Piece {
    pub at: At,
    pub ops: &'static [Op],
}

/// The handful of instructions the fixes need.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Raw(&'static [u8]),
    /// `jmp rel32` to an address.
    Jmp(At),
    /// `jcc rel32`; the value is the condition code (0x4 = e, 0x5 = ne).
    Jcc(u8, At),
}

pub const JE: u8 = 0x4;
pub const JNE: u8 = 0x5;
const NOP: u8 = 0x90;

impl Op {
    fn len(self) -> usize {
        match self {
            Op::Raw(b) => b.len(),
            Op::Jmp(_) => 5,
            Op::Jcc(..) => 6,
        }
    }
}

impl Piece {
    fn len(&self) -> usize {
        self.ops.iter().map(|op| op.len()).sum()
    }
}

/// A concrete write: bytes at an absolute address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Write {
    pub fix: &'static str,
    pub address: u32,
    pub bytes: Vec<u8>,
    /// True for writes into the game image (checked against the file and
    /// written over existing code); false for the cave.
    pub in_image: bool,
}

impl FixSet {
    /// Bytes the cave needs; 0 means no cave is allocated.
    pub fn cave_size(&self) -> u32 {
        self.pieces()
            .filter_map(|p| match p.at {
                At::Cave(off) => Some(off + p.len() as u32),
                At::Image(_) => None,
            })
            .max()
            .unwrap_or(0)
    }

    fn pieces(&self) -> impl Iterator<Item = &Piece> {
        self.fixes.iter().flat_map(|f| f.code.iter())
    }

    /// Resolves every piece to absolute bytes, given where the cave landed.
    pub fn assemble(&self, cave_base: u32) -> Vec<Write> {
        let resolve = |at: At| match at {
            At::Image(a) => a,
            At::Cave(off) => cave_base.wrapping_add(off),
        };
        let mut writes = Vec::new();
        for fix in self.fixes {
            for piece in fix.code {
                let start = resolve(piece.at);
                let mut bytes = Vec::with_capacity(piece.len());
                for op in piece.ops {
                    let here = start.wrapping_add(bytes.len() as u32);
                    match *op {
                        Op::Raw(b) => bytes.extend_from_slice(b),
                        Op::Jmp(to) => {
                            bytes.push(0xe9);
                            bytes.extend_from_slice(&rel32(here, 5, resolve(to)));
                        }
                        Op::Jcc(cc, to) => {
                            bytes.extend_from_slice(&[0x0f, 0x80 | cc]);
                            bytes.extend_from_slice(&rel32(here, 6, resolve(to)));
                        }
                    }
                }
                writes.push(Write {
                    fix: fix.id,
                    address: start,
                    bytes,
                    in_image: matches!(piece.at, At::Image(_)),
                });
            }
        }
        writes
    }
}

fn rel32(at: u32, len: u32, to: u32) -> [u8; 4] {
    to.wrapping_sub(at.wrapping_add(len)).to_le_bytes()
}

/// For executables that already carry equivalent fixes on disk.
pub static NO_FIXES: FixSet = FixSet { fixes: &[] };

/// Official EU v1.5 (`0eb6ea96…`).
pub static OFFICIAL_EU_15: FixSet = FixSet {
    fixes: &[
        Fix {
            id: "F1",
            what: "CD-ROM check always finds the disc (the files come from CDPath)",
            // FUN_00496ca0 (no arguments, returns nonzero when found):
            // mov eax,1 ; ret
            code: &[Piece {
                at: At::Image(0x0049_6ca0),
                ops: &[Op::Raw(&[0xb8, 0x01, 0x00, 0x00, 0x00, 0xc3])],
            }],
        },
        Fix {
            id: "F2",
            what: "front end uses a 16-bit display mode instead of 8-bit palettized",
            // Immediate of `mov edi,8` (the requested depth) becomes 16.
            code: &[Piece {
                at: At::Image(0x0049_7cef),
                ops: &[Op::Raw(&[0x10])],
            }],
        },
        Fix {
            id: "F3",
            what: "system-palette setup runs only when the caller passes a palette",
            // Replaces `jne 0x497f44` after the depth compare with a jump to
            // the cave, which keeps that branch and adds the palette test.
            code: &[
                Piece {
                    at: At::Image(0x0049_7df5),
                    ops: &[Op::Jmp(At::Cave(0)), Op::Raw(&[NOP])],
                },
                Piece {
                    at: At::Cave(0),
                    ops: &[
                        Op::Jcc(JNE, At::Image(0x0049_7f44)),
                        // cmp dword [esp+0x14],0  (the palette argument)
                        Op::Raw(&[0x83, 0x7c, 0x24, 0x14, 0x00]),
                        Op::Jcc(JE, At::Image(0x0049_7f44)),
                        Op::Jmp(At::Image(0x0049_7dfb)),
                    ],
                },
            ],
        },
        Fix {
            id: "F4",
            what: "operating-system check accepts Windows NT",
            // FUN_0049dcb0 returns 0 to accept: xor eax,eax ; ret
            code: &[Piece {
                at: At::Image(0x0049_dcb0),
                ops: &[Op::Raw(&[0x31, 0xc0, 0xc3])],
            }],
        },
        Fix {
            id: "F5",
            what: "skip the VGA palette read through I/O ports (privileged on NT)",
            // FUN_004ae590(buffer), stdcall: ret 4
            code: &[Piece {
                at: At::Image(0x004a_e590),
                ops: &[Op::Raw(&[0xc2, 0x04, 0x00])],
            }],
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;

    const CAVE: u32 = 0x0060_0000;

    fn writes() -> Vec<Write> {
        OFFICIAL_EU_15.assemble(CAVE)
    }

    fn at(address: u32) -> Vec<u8> {
        writes()
            .into_iter()
            .find(|w| w.address == address)
            .unwrap()
            .bytes
    }

    #[test]
    fn fixes_are_f1_to_f5_in_order() {
        let ids: Vec<_> = OFFICIAL_EU_15.fixes.iter().map(|f| f.id).collect();
        assert_eq!(ids, ["F1", "F2", "F3", "F4", "F5"]);
    }

    #[test]
    fn simple_stubs() {
        assert_eq!(at(0x496ca0), [0xb8, 1, 0, 0, 0, 0xc3]);
        assert_eq!(at(0x497cef), [0x10]);
        assert_eq!(at(0x49dcb0), [0x31, 0xc0, 0xc3]);
        assert_eq!(at(0x4ae590), [0xc2, 0x04, 0x00]);
    }

    #[test]
    fn f3_jumps_to_the_cave_and_back() {
        // jmp CAVE from 0x497df5: rel = CAVE - 0x497dfa
        let site = at(0x497df5);
        assert_eq!(site.len(), 6, "replaces the 6-byte jne exactly");
        assert_eq!(site[0], 0xe9);
        assert_eq!(
            u32::from_le_bytes(site[1..5].try_into().unwrap()),
            CAVE.wrapping_sub(0x497dfa)
        );
        assert_eq!(site[5], 0x90);

        let cave = at(CAVE);
        assert_eq!(cave.len() as u32, OFFICIAL_EU_15.cave_size());
        let target = |off: usize, len: u32| {
            let rel = u32::from_le_bytes(
                cave[off + len as usize - 4..off + len as usize]
                    .try_into()
                    .unwrap(),
            );
            (CAVE + off as u32 + len).wrapping_add(rel)
        };
        assert_eq!(&cave[0..2], [0x0f, 0x85]);
        assert_eq!(target(0, 6), 0x497f44);
        assert_eq!(&cave[6..11], [0x83, 0x7c, 0x24, 0x14, 0x00]);
        assert_eq!(&cave[11..13], [0x0f, 0x84]);
        assert_eq!(target(11, 6), 0x497f44);
        assert_eq!(cave[17], 0xe9);
        assert_eq!(target(17, 5), 0x497dfb);
    }

    #[test]
    fn image_writes_do_not_overlap() {
        let mut spans: Vec<_> = writes()
            .into_iter()
            .filter(|w| w.in_image)
            .map(|w| (w.address, w.address + w.bytes.len() as u32))
            .collect();
        spans.sort();
        for pair in spans.windows(2) {
            assert!(pair[0].1 <= pair[1].0, "{pair:x?}");
        }
    }

    #[test]
    fn cave_is_independent_of_its_base() {
        // Only the rel32 fields change with the base; lengths never do.
        let a = OFFICIAL_EU_15.assemble(0x1000_0000);
        let b = OFFICIAL_EU_15.assemble(0x7000_0000);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.bytes.len(), y.bytes.len());
        }
    }
}
