//! Game fixes, applied to the suspended process before it runs.
//!
//! Every fix replaces a few instructions at a known address of one exact
//! executable (see `edition`). The bytes written here are our own code; the
//! launcher checks that memory still matches the file on disk before it
//! writes anything, so a mismatch stops the launch instead of corrupting it.
//! Addresses and the reasons are documented in `docs/THIN_APP_PLAN.md`
//! (inventory F1-F6).

/// One fix: a list of code pieces written at absolute addresses.
#[derive(Debug, PartialEq, Eq)]
pub struct Fix {
    pub id: &'static str,
    pub what: &'static str,
    /// The game runs without it; a setting can turn it off.
    pub optional: bool,
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
    /// A 32-bit absolute address (a jump-table entry).
    Addr(At),
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
            Op::Addr(_) => 4,
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
    /// The fixes to apply: all of them except the optional ones named in
    /// `skip` (required fixes are never skipped).
    pub fn active<'a>(&'a self, skip: &'a [&str]) -> impl Iterator<Item = &'a Fix> {
        self.fixes
            .iter()
            .filter(move |f| !(f.optional && skip.contains(&f.id)))
    }

    /// Bytes the cave needs; 0 means no cave is allocated.
    pub fn cave_size(&self, skip: &[&str]) -> u32 {
        self.active(skip)
            .flat_map(|f| f.code.iter())
            .filter_map(|p| match p.at {
                At::Cave(off) => Some(off + p.len() as u32),
                At::Image(_) => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// Resolves every active piece to absolute bytes, given where the cave
    /// landed.
    pub fn assemble(&self, cave_base: u32, skip: &[&str]) -> Vec<Write> {
        let resolve = |at: At| match at {
            At::Image(a) => a,
            At::Cave(off) => cave_base.wrapping_add(off),
        };
        let mut writes = Vec::new();
        for fix in self.active(skip) {
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
                        Op::Addr(to) => bytes.extend_from_slice(&resolve(to).to_le_bytes()),
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

/// The community-patched EU v1.5 (`46500c8e…`) carries F1-F5 on disk; it
/// is the same build, so F6 applies at the same addresses.
pub static COMMUNITY_EU_15: FixSet = FixSet {
    fixes: &[F6, F7, F8, F9, F10, F11],
};

/// Official EU v1.5 (`0eb6ea96…`).
pub static OFFICIAL_EU_15: FixSet = FixSet {
    fixes: &[
        Fix {
            id: "F1",
            what: "CD-ROM check always finds the disc (the files come from CDPath)",
            optional: false,
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
            optional: false,
            // Immediate of `mov edi,8` (the requested depth) becomes 16.
            code: &[Piece {
                at: At::Image(0x0049_7cef),
                ops: &[Op::Raw(&[0x10])],
            }],
        },
        Fix {
            id: "F3",
            what: "system-palette setup runs only when the caller passes a palette",
            optional: false,
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
            optional: false,
            // FUN_0049dcb0 returns 0 to accept: xor eax,eax ; ret
            code: &[Piece {
                at: At::Image(0x0049_dcb0),
                ops: &[Op::Raw(&[0x31, 0xc0, 0xc3])],
            }],
        },
        Fix {
            id: "F5",
            what: "skip the VGA palette read through I/O ports (privileged on NT)",
            optional: false,
            // FUN_004ae590(buffer), stdcall: ret 4
            code: &[Piece {
                at: At::Image(0x004a_e590),
                ops: &[Op::Raw(&[0xc2, 0x04, 0x00])],
            }],
        },
        F6,
        F7,
        F8,
        F9,
        F10,
        F11,
    ],
};

/// Cave offset of F6, after F3's 22 bytes.
const F6_CAVE: u32 = 0x20;
const _: () = assert!(F6_CAVE >= 22, "F6 must follow F3's cave code");

const F6: Fix = Fix {
    id: "F6",
    what: "closing the window (Alt+F4, the window manager) quits the game",
    optional: true,
    // The window procedure swallows WM_CLOSE: its jump-table entry at
    // 0x49de78 (shared with WM_ERASEBKGND) leads to `return 0`. The
    // entry now points to the cave, which on WM_CLOSE (ecx = msg)
    // sets the main loop's quit flag, so the game shuts down the way
    // its own Quit does; both messages still return 0.
    code: &[
        Piece {
            at: At::Image(0x0049_de78),
            ops: &[Op::Addr(At::Cave(F6_CAVE))],
        },
        Piece {
            at: At::Cave(F6_CAVE),
            ops: &[
                // cmp ecx,0x10 ; jne +10
                Op::Raw(&[0x83, 0xf9, 0x10, 0x75, 0x0a]),
                // mov dword [0x96efe8],1
                Op::Raw(&[0xc7, 0x05, 0xe8, 0xef, 0x96, 0x00, 0x01, 0x00, 0x00, 0x00]),
                Op::Jmp(At::Image(0x0049_dde6)),
            ],
        },
    ],
};

/// Cave offset of F7's code and of its per-joystick axis table (16 dwords,
/// one per WinMM id; filled by the launcher, see `F7_TABLE`).
const F7_CAVE: u32 = 0x40;
pub const F7_TABLE: u32 = 0xc0;
const _: () = assert!(F7_CAVE >= F6_CAVE + 20 && F7_TABLE >= F7_CAVE + F7_CODE_LEN);
const F7_CODE_LEN: u32 = 93;
/// `F7_TABLE` bits: use the Z axis (a pad's combined triggers) and the R
/// axis (a pad's right stick, up/down) for throttle and brake.
pub const AXIS_Z: u32 = 1;
pub const AXIS_R: u32 = 2;

/// The game's analog joystick read (`FUN_0049cdf0`) asks WinMM for X, Y and
/// buttons only, so on a modern pad only the left stick drives: X steers, Y
/// is throttle (up) and brake (down). F7 also asks for Z and R and feeds the
/// game's Y with whichever of left-stick Y, Z or R is pushed furthest from
/// centre, for axes the launcher enabled for that joystick (only axes that
/// exist and rest at centre, so wheel pedals are never picked up). Up on
/// the right stick and R2 read low like left-stick up, so they accelerate;
/// down and L2 brake. Menus still follow the left stick.
const F7: Fix = Fix {
    id: "F7",
    what: "gamepads: triggers and right stick work as throttle and brake",
    optional: true,
    code: &[
        // JOYINFOEX.dwFlags 0x483 -> 0x48f (also return Z and R).
        Piece {
            at: At::Image(0x0049_ce1b),
            ops: &[Op::Raw(&[0x8f])],
        },
        // `mov eax,[esp+0x14]` (Y) ; `push ebx` -> jmp to the cave.
        Piece {
            at: At::Image(0x0049_ce6f),
            ops: &[Op::Jmp(At::Cave(F7_CAVE))],
        },
        Piece {
            at: At::Cave(F7_CAVE),
            ops: &[
                // push ebx (replaced) ; then Y/Z/R sit at esp+0x18/0x1c/0x20
                Op::Raw(&[0x53]),
                // mov eax,[esi+0x10] (WinMM id) ; and eax,0xf
                Op::Raw(&[0x8b, 0x46, 0x10, 0x83, 0xe0, 0x0f]),
                // mov edx,[eax*4+table]
                Op::Raw(&[0x8b, 0x14, 0x85]),
                Op::Addr(At::Cave(F7_TABLE)),
                // mov eax,[esp+0x18] (best = Y)
                Op::Raw(&[0x8b, 0x44, 0x24, 0x18]),
                // lea ebx,[eax-0x8000] ; test ebx,ebx ; jns +2 ; neg ebx
                Op::Raw(&[
                    0x8d, 0x98, 0x00, 0x80, 0xff, 0xff, 0x85, 0xdb, 0x79, 0x02, 0xf7, 0xdb,
                ]),
                // test dl,AXIS_Z ; je +24
                Op::Raw(&[0xf6, 0xc2, AXIS_Z as u8, 0x74, 0x18]),
                // the candidate at [esp+0x1c] wins if further from centre
                Op::Raw(&TAKE_IF_FURTHER_Z),
                // test dl,AXIS_R ; je +24
                Op::Raw(&[0xf6, 0xc2, AXIS_R as u8, 0x74, 0x18]),
                Op::Raw(&TAKE_IF_FURTHER_R),
                // back to `lea ecx,[eax-0x8000]`, which scales Y
                Op::Jmp(At::Image(0x0049_ce74)),
            ],
        },
        // The table, zero (no extra axes) until the launcher fills it.
        Piece {
            at: At::Cave(F7_TABLE),
            ops: &[Op::Raw(&[0; 64])],
        },
    ],
};

const F8_CAVE: u32 = 0x100;
const F8_SEEN: u32 = 0x118;
const F9_CAVE: u32 = 0x120;
const _: () =
    assert!(F8_CAVE >= F7_TABLE + 64 && F8_SEEN >= F8_CAVE + 22 && F9_CAVE >= F8_SEEN + 4);

/// The joystick presence probe (`FUN_0049d5c0`) asks WinMM for every id each
/// call and reports only the ones that answer. Control Setup runs it every
/// frame and clamps CONTROL METHOD to the joysticks it reports, so one failed
/// read (Wine briefly returns JOYERR_UNPLUGGED while it rescans devices, and
/// wireless pads doze) drops the choice back to KEYBOARD or below; OK then
/// saves device 0 with the joystick's type (see F9). F8 makes a joystick that
/// has answered once stay present for the session.
const F8: Fix = Fix {
    id: "F8",
    what: "joysticks that answered once stay present (no random fall-back to keyboard)",
    optional: false,
    code: &[
        // pop edi ; pop esi ; mov eax,ebx ; pop ebp -> jmp to the cave
        Piece {
            at: At::Image(0x0049_d60e),
            ops: &[Op::Jmp(At::Cave(F8_CAVE))],
        },
        Piece {
            at: At::Cave(F8_CAVE),
            ops: &[
                Op::Raw(&[0x5f, 0x5e]), // pop edi ; pop esi
                Op::Raw(&[0x0b, 0x1d]), // or ebx,[seen]
                Op::Addr(At::Cave(F8_SEEN)),
                Op::Raw(&[0x89, 0x1d]), // mov [seen],ebx
                Op::Addr(At::Cave(F8_SEEN)),
                Op::Raw(&[0x8b, 0xc3, 0x5d]), // mov eax,ebx ; pop ebp
                Op::Jmp(At::Image(0x0049_d613)),
            ],
        },
        Piece {
            at: At::Cave(F8_SEEN),
            ops: &[Op::Raw(&[0; 4])],
        },
    ],
};

/// The device-type setter (`FUN_0049d670(record, type)`) writes any type into
/// any record. Records 0 and 1 are the two keyboards (their `+0x78` holds key
/// codes), but Control Setup's OK passes record 0 with the joystick's type
/// when CONTROL METHOD ended below KEYBOARD (F8), and the startup loop
/// (`0x4b9ed7`) re-applies whatever was saved. The game then reads the
/// keyboard record as an analog joystick: key codes as axes, full lock and
/// full brake, the car circles in reverse. F9 keeps records 0 and 1 of type 1
/// (keyboard), which also repairs a save already in that state.
const F9: Fix = Fix {
    id: "F9",
    what: "keyboard device records always stay keyboards",
    optional: false,
    code: &[
        // mov ecx,[esp+8] ; lea eax,[eax+eax*4] -> jmp to the cave ; nop ; nop
        Piece {
            at: At::Image(0x0049_d67d),
            ops: &[Op::Jmp(At::Cave(F9_CAVE)), Op::Raw(&[NOP, NOP])],
        },
        Piece {
            at: At::Cave(F9_CAVE),
            ops: &[
                Op::Raw(&[0x8b, 0x4c, 0x24, 0x08]),       // mov ecx,[esp+8] (type)
                Op::Raw(&[0x83, 0x7c, 0x24, 0x04, 0x01]), // cmp dword [esp+4],1 (record)
                Op::Raw(&[0x77, 0x05]),                   // ja +5
                Op::Raw(&[0xb9, 0x01, 0x00, 0x00, 0x00]), // mov ecx,1
                Op::Raw(&[0x8d, 0x04, 0x80]),             // lea eax,[eax+eax*4]
                Op::Jmp(At::Image(0x0049_d684)),
            ],
        },
    ],
};

const F10_CAVE: u32 = 0x140;
/// F10's data: the race width and height (two dwords), filled by the
/// launcher from the `resolution` setting.
pub const F10_SIZE: u32 = 0x168;
const _: () = assert!(F10_CAVE >= F9_CAVE + 24 && F10_SIZE >= F10_CAVE + 38);

/// Races re-create DirectDraw at the RESOLUTION setting (`FUN_00421da0` ->
/// `FUN_00497be0` with the 16-bit table), which offers four slots up to
/// 800x600; the front end always runs its own 640x480 init through another
/// branch. F10 hooks the race's slot load (`0x497d43`): slot 2, the one the
/// menu calls 640x480, returns the configured width and height instead, so
/// the race's surfaces, Z-buffer and viewport follow; the other slots and
/// the front end are untouched. Opt-in: applied only with a `resolution`
/// setting. The display must offer that mode (the game ignores a failed
/// SetDisplayMode).
const F10: Fix = Fix {
    id: "F10",
    what: "races at a chosen resolution (replaces the 640x480 slot)",
    optional: true,
    code: &[
        // mov ecx,[eax+0xa204f0] ; mov eax,[eax+0xa204f4] -> jmp ; 7 x nop
        Piece {
            at: At::Image(0x0049_7d43),
            ops: &[Op::Jmp(At::Cave(F10_CAVE)), Op::Raw(&[NOP; 7])],
        },
        Piece {
            at: At::Cave(F10_CAVE),
            ops: &[
                Op::Raw(&[0x83, 0xfb, 0x02, 0x75, 0x10]), // cmp ebx,2 ; jne +16
                Op::Raw(&[0x8b, 0x0d]),                   // mov ecx,[width]
                Op::Addr(At::Cave(F10_SIZE)),
                Op::Raw(&[0xa1]), // mov eax,[height]
                Op::Addr(At::Cave(F10_SIZE + 4)),
                Op::Jmp(At::Image(0x0049_7d4f)),
                // the original loads
                Op::Raw(&[0x8b, 0x88, 0xf0, 0x04, 0xa2, 0x00]),
                Op::Raw(&[0x8b, 0x80, 0xf4, 0x04, 0xa2, 0x00]),
                Op::Jmp(At::Image(0x0049_7d4f)),
            ],
        },
        Piece {
            at: At::Cave(F10_SIZE),
            ops: &[Op::Raw(&[0; 8])],
        },
    ],
};

const F11_PROJ: u32 = 0x180;
const F11_K: u32 = 0x190;
const F11_VIEW: u32 = 0x1a0;
const F11_Q: u32 = 0x1d0;
const _: () = assert!(
    F11_PROJ >= F10_SIZE + 8
        && F11_K >= F11_PROJ + 11
        && F11_VIEW >= F11_K + 8
        && F11_Q >= F11_VIEW + 46
);

/// The race projection (`FUN_00435d10`) scales its horizontal factor by the
/// screen's W/H, and the viewport (`0x4a6ed0`) clips x to [-1, 1] and y to
/// [-W/H, W/H]. Right only at 4:3: at 16:9 the view gets narrower and
/// taller (stretched). F11 makes it Hor+: the projection always uses the
/// 4:3 aspect, and the viewport keeps y at [-4/3, 4/3] and widens x to
/// [-A*3/4, A*3/4] (A = viewport W/H), so the vertical field of view is the
/// original one, the horizontal one grows with the screen and pixels stay
/// square. At 4:3 both give exactly the original values. Opt-in
/// (`widescreen`). Split-screen views use another path (not changed).
const F11: Fix = Fix {
    id: "F11",
    what: "widescreen: wider view instead of a stretched one (Hor+)",
    optional: true,
    code: &[
        // fild [W] ; fidiv [H] ; fmul [65536.0] -> jmp ; 6 x nop
        Piece {
            at: At::Image(0x0043_5d44),
            ops: &[Op::Jmp(At::Cave(F11_PROJ)), Op::Raw(&[NOP; 6])],
        },
        Piece {
            at: At::Cave(F11_PROJ),
            ops: &[
                Op::Raw(&[0xdd, 0x05]), // fld qword [65536 * 4/3]
                Op::Addr(At::Cave(F11_K)),
                Op::Jmp(At::Image(0x0043_5d4f)),
            ],
        },
        Piece {
            at: At::Cave(F11_K),
            // 65536.0 * 4/3 as a double
            ops: &[Op::Raw(&[0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0xf5, 0x40])],
        },
        // mov eax,[eax+0x14] ; push eax ; mov ecx,[eax] -> jmp ; nop
        Piece {
            at: At::Image(0x004a_6f75),
            ops: &[Op::Jmp(At::Cave(F11_VIEW)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F11_VIEW),
            ops: &[
                // edx = the D3DVIEWPORT2; dvClipY (+0x18) holds W/H here.
                Op::Raw(&[0xd9, 0x42, 0x18]), // fld dword [edx+0x18]
                Op::Raw(&[0xd8, 0x0d]),       // fmul dword [0.75]
                Op::Addr(At::Cave(F11_Q)),
                Op::Raw(&[0xd9, 0xc0, 0xd9, 0xe0]), // fld st0 ; fchs
                Op::Raw(&[0xd9, 0x5a, 0x14]),       // fstp [edx+0x14] dvClipX
                Op::Raw(&[0xdc, 0xc0]),             // fadd st0,st0
                Op::Raw(&[0xd9, 0x5a, 0x1c]),       // fstp [edx+0x1c] dvClipWidth
                // dvClipY = 4/3 ; dvClipHeight = 8/3
                Op::Raw(&[0xc7, 0x42, 0x18, 0xab, 0xaa, 0xaa, 0x3f]),
                Op::Raw(&[0xc7, 0x42, 0x20, 0xab, 0xaa, 0x2a, 0x40]),
                // the replaced instructions
                Op::Raw(&[0x8b, 0x40, 0x14, 0x50, 0x8b, 0x08]),
                Op::Jmp(At::Image(0x004a_6f7b)),
            ],
        },
        Piece {
            at: At::Cave(F11_Q),
            ops: &[Op::Raw(&[0x00, 0x00, 0x40, 0x3f])], // 0.75f
        },
    ],
};

/// 24 bytes: ecx = |[esp+d8] - 0x8000|; if ecx > ebx { ebx = ecx; eax = [esp+d8] }.
const fn take_if_further(d8: u8) -> [u8; 24] {
    [
        0x8b, 0x4c, 0x24, d8, // mov ecx,[esp+d8]
        0x81, 0xe9, 0x00, 0x80, 0x00, 0x00, // sub ecx,0x8000
        0x79, 0x02, // jns +2
        0xf7, 0xd9, // neg ecx
        0x3b, 0xcb, // cmp ecx,ebx
        0x7e, 0x06, // jle +6
        0x8b, 0xd9, // mov ebx,ecx
        0x8b, 0x44, 0x24, d8, // mov eax,[esp+d8]
    ]
}
const TAKE_IF_FURTHER_Z: [u8; 24] = take_if_further(0x1c);
const TAKE_IF_FURTHER_R: [u8; 24] = take_if_further(0x20);

#[cfg(test)]
mod tests {
    use super::*;

    const CAVE: u32 = 0x0060_0000;

    fn writes() -> Vec<Write> {
        OFFICIAL_EU_15.assemble(CAVE, &[])
    }

    fn at(address: u32) -> Vec<u8> {
        writes()
            .into_iter()
            .find(|w| w.address == address)
            .unwrap()
            .bytes
    }

    #[test]
    fn fixes_are_f1_to_f6_in_order() {
        let ids: Vec<_> = OFFICIAL_EU_15.fixes.iter().map(|f| f.id).collect();
        assert_eq!(
            ids,
            [
                "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11"
            ]
        );
    }

    #[test]
    fn f6_points_the_close_entry_at_the_cave() {
        assert_eq!(at(0x49de78), (CAVE + F6_CAVE).to_le_bytes());
        let cave = at(CAVE + F6_CAVE);
        assert_eq!(&cave[..5], [0x83, 0xf9, 0x10, 0x75, 0x0a]);
        // jne skips exactly the 10-byte store, landing on the jmp.
        assert_eq!(&cave[5..7], [0xc7, 0x05]);
        assert_eq!(cave[15], 0xe9);
        let rel = u32::from_le_bytes(cave[16..20].try_into().unwrap());
        assert_eq!((CAVE + F6_CAVE + 20).wrapping_add(rel), 0x49dde6);
    }

    #[test]
    fn f7_code_fits_and_reads_its_table() {
        let code = at(CAVE + F7_CAVE);
        assert_eq!(code.len() as u32, F7_CODE_LEN);
        assert_eq!(&code[7..10], [0x8b, 0x14, 0x85]);
        assert_eq!(code[10..14], (CAVE + F7_TABLE).to_le_bytes());
        // Both je skip exactly one 24-byte candidate block.
        assert_eq!(&code[30..35], [0xf6, 0xc2, 1, 0x74, 24]);
        assert_eq!(&code[59..64], [0xf6, 0xc2, 2, 0x74, 24]);
        assert_eq!(code[88], 0xe9);
        let rel = u32::from_le_bytes(code[89..93].try_into().unwrap());
        assert_eq!((CAVE + F7_CAVE + 93).wrapping_add(rel), 0x49ce74);
        assert_eq!(at(0x49ce1b), [0x8f]);
        assert_eq!(at(CAVE + F7_TABLE), [0; 64]);
    }

    #[test]
    fn f8_and_f9_jump_back_to_the_right_places() {
        let back = |code: &[u8], jmp_at: usize, base: u32| {
            assert_eq!(code[jmp_at], 0xe9);
            let rel = u32::from_le_bytes(code[jmp_at + 1..jmp_at + 5].try_into().unwrap());
            (base + jmp_at as u32 + 5).wrapping_add(rel)
        };
        let f8 = at(CAVE + F8_CAVE);
        assert_eq!(f8.len(), 22);
        assert_eq!(f8[4..8], (CAVE + F8_SEEN).to_le_bytes());
        assert_eq!(f8[10..14], (CAVE + F8_SEEN).to_le_bytes());
        assert_eq!(back(&f8, 17, CAVE + F8_CAVE), 0x49d613);
        let f9 = at(CAVE + F9_CAVE);
        assert_eq!(f9.len(), 24);
        // ja skips exactly the 5-byte `mov ecx,1`.
        assert_eq!(&f9[9..11], [0x77, 0x05]);
        assert_eq!(f9[11], 0xb9);
        assert_eq!(back(&f9, 19, CAVE + F9_CAVE), 0x49d684);
        assert_eq!(at(0x49d67d)[5..], [0x90, 0x90]);
    }

    #[test]
    fn f10_returns_the_configured_size_for_slot_2_only() {
        let code = at(CAVE + F10_CAVE);
        assert_eq!(code.len(), 38);
        assert_eq!(&code[..5], [0x83, 0xfb, 0x02, 0x75, 0x10]);
        assert_eq!(code[7..11], (CAVE + F10_SIZE).to_le_bytes());
        assert_eq!(code[12..16], (CAVE + F10_SIZE + 4).to_le_bytes());
        let back = |jmp_at: usize| {
            let rel = u32::from_le_bytes(code[jmp_at + 1..jmp_at + 5].try_into().unwrap());
            (CAVE + F10_CAVE + jmp_at as u32 + 5).wrapping_add(rel)
        };
        assert_eq!(code[16], 0xe9);
        assert_eq!(back(16), 0x497d4f);
        // jne +16 lands on the original loads.
        assert_eq!(&code[21..27], [0x8b, 0x88, 0xf0, 0x04, 0xa2, 0x00]);
        assert_eq!(back(33), 0x497d4f);
        assert_eq!(at(0x497d43).len(), 12);
    }

    #[test]
    fn f11_constants_and_jumps() {
        assert_eq!(
            f64::from_le_bytes(at(CAVE + F11_K).try_into().unwrap()),
            65536.0 * 4.0 / 3.0
        );
        assert_eq!(
            f32::from_le_bytes(at(CAVE + F11_Q).try_into().unwrap()),
            0.75
        );
        let view = at(CAVE + F11_VIEW);
        assert_eq!(view.len(), 46);
        assert_eq!(
            f32::from_le_bytes(view[24..28].try_into().unwrap()),
            4.0 / 3.0
        );
        assert_eq!(
            f32::from_le_bytes(view[31..35].try_into().unwrap()),
            8.0 / 3.0
        );
        let back = |code: &[u8], base: u32| {
            let n = code.len();
            assert_eq!(code[n - 5], 0xe9);
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            (base + n as u32).wrapping_add(rel)
        };
        assert_eq!(back(&view, CAVE + F11_VIEW), 0x4a6f7b);
        assert_eq!(back(&at(CAVE + F11_PROJ), CAVE + F11_PROJ), 0x435d4f);
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
        assert_eq!(cave.len(), 22);
        assert_eq!(OFFICIAL_EU_15.cave_size(&[]), F11_Q + 4);
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
    fn only_optional_fixes_can_be_skipped() {
        let skip = ["F6", "F7", "F10", "F11", "F1"];
        let ids: Vec<_> = OFFICIAL_EU_15.active(&skip).map(|f| f.id).collect();
        assert_eq!(ids, ["F1", "F2", "F3", "F4", "F5", "F8", "F9"]);
        assert_eq!(OFFICIAL_EU_15.cave_size(&skip), F9_CAVE + 24);
        assert!(
            OFFICIAL_EU_15
                .assemble(CAVE, &skip)
                .iter()
                .all(|w| w.fix != "F6")
        );
        assert_eq!(
            COMMUNITY_EU_15.cave_size(&["F6", "F7", "F10", "F11"]),
            F9_CAVE + 24
        );
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
        let a = OFFICIAL_EU_15.assemble(0x1000_0000, &[]);
        let b = OFFICIAL_EU_15.assemble(0x7000_0000, &[]);
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.bytes.len(), y.bytes.len());
        }
    }
}
