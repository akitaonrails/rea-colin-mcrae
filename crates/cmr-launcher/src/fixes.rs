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
    /// `call rel32` to an address.
    Call(At),
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
            Op::Call(_) => 5,
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
                        Op::Call(to) => {
                            bytes.push(0xe8);
                            bytes.extend_from_slice(&rel32(here, 5, resolve(to)));
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

/// The community-patched EU v1.5 (`46500c8e…`) carries F1-F5 on disk; it
/// is the same build, so F6 applies at the same addresses.
pub static COMMUNITY_EU_15: FixSet = FixSet {
    fixes: &[
        F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16, F17, F18, F19,
    ],
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
        F12,
        F13,
        F14,
        F15,
        F16,
        F17,
        F18,
        F19,
        F20,
        F21,
        F22,
        F23,
        F24,
        F25,
        F26,
        F27,
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
        // JOYINFOEX.dwFlags 0x483 -> 0x4cf (also return Z, R and the
        // d-pad, which F23 reads).
        Piece {
            at: At::Image(0x0049_ce1b),
            ops: &[Op::Raw(&[0xcf])],
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
const F11_YS: u32 = 0x1a0;
const F11_K3: u32 = 0x1c0;
const F11_T: u32 = 0x1c8;
const _: () = assert!(
    F11_PROJ >= F10_SIZE + 8
        && F11_K >= F11_PROJ + 16
        && F11_YS >= F11_K + 8
        && F11_K3 >= F11_YS + 32
        && F11_T >= F11_K3 + 8
);

/// The culling wedges' sideways factor `t` (Q16): `tan` of half the
/// horizontal view, 1.0 at 4:3, aspect × 3/4 with F11 (filled by the
/// launcher from the race resolution), and the immediates it replaces: the
/// terrain cull's two (`0x4902c6`, `0x4902e5`) and the near-object
/// triangle's two (`0x4a097e`, `0x4a099f`).
pub const F11_CULL_T: u32 = 0xb90;
pub const F11_CULL_IMMS: [u32; 4] = [0x0049_02c6, 0x0049_02e5, 0x004a_097e, 0x004a_099f];
const F11_CULL_STUBS: u32 = 0xba0;
const _: () = assert!(F11_CULL_T >= F18_CAVE + 0xc0 + 20 && F11_CULL_STUBS >= F11_CULL_T + 4);

/// The culling factor `t` for a `w` x `h` race (Q16): aspect × 3/4, so 1.0
/// at 4:3; never narrower than the original.
pub fn cull_t(w: u32, h: u32) -> u32 {
    if w == 0 || h == 0 {
        return 0x10000;
    }
    ((u64::from(w) * 3 * 0x10000 / (u64::from(h) * 4)) as u32).max(0x10000)
}

/// The race projection (`FUN_00435d10`) feeds the game's own software
/// transform, which maps to pixels as `x * xs * W/2` and `y * ys * H/(2A)`
/// (A = W/H), with `xs` scaled by A. Right only at 4:3: at 16:9 the picture
/// comes out 1.78x wider and 0.75x shorter than at 4:3 (stretched). F11
/// makes it Hor+, so the pixel scale is the one of a 4:3 screen of the same
/// height and the wider screen shows more to the sides: the aspect term of
/// `xs` becomes (4/3)^2 / A (`0x435d44`) and `ys` is multiplied by A / (4/3)
/// (`0x435d87`). At 4:3 both are exactly the original values. The game's
/// culling (terrain cells `0x490040`, objects in `0x4a07d0`) keeps what is
/// inside 90° wedges, the 4:3 view; F11 widens their sideways extents by
/// `t` (aspect × 3/4), so scenery at the edges of a wider view isn't
/// culled. Opt-in (`widescreen`).
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
                // eax = the display struct: fild [H] ; fidiv [W]
                Op::Raw(&[0xdb, 0x40, 0x04, 0xda, 0x30]),
                Op::Raw(&[0xdc, 0x0d]), // fmul qword [65536 * 16/9]
                Op::Addr(At::Cave(F11_K)),
                Op::Jmp(At::Image(0x0043_5d4f)),
            ],
        },
        Piece {
            at: At::Cave(F11_K),
            // 65536.0 * 16/9 as a double
            ops: &[Op::Raw(&[0x1c, 0xc7, 0x71, 0x1c, 0xc7, 0x71, 0xfc, 0x40])],
        },
        // mov eax,0x2bae1 (the ys factor) -> jmp
        Piece {
            at: At::Image(0x0043_5d87),
            ops: &[Op::Jmp(At::Cave(F11_YS))],
        },
        Piece {
            at: At::Cave(F11_YS),
            ops: &[
                Op::Call(At::Cave(F11_READ)), // eax = the display state (3D view)
                Op::Raw(&[0xdb, 0x00, 0xda, 0x70, 0x04]), // fild [W] ; fidiv [H]
                Op::Raw(&[0xdc, 0x0d]),       // fmul qword [0x2bae1 * 3/4]
                Op::Addr(At::Cave(F11_K3)),
                Op::Raw(&[0xdb, 0x1d]), // fistp dword [t]
                Op::Addr(At::Cave(F11_T)),
                Op::Raw(&[0xa1]), // mov eax,[t]
                Op::Addr(At::Cave(F11_T)),
                Op::Jmp(At::Image(0x0043_5d8c)),
            ],
        },
        Piece {
            at: At::Cave(F11_K3),
            // 0x2bae1 * 0.75 as a double
            ops: &[Op::Raw(&[0x00, 0x00, 0x00, 0x00, 0x46, 0x61, 0x00, 0x41])],
        },
        Piece {
            at: At::Cave(F11_T),
            ops: &[Op::Raw(&[0; 4])],
        },
        // The culling wedges' sideways factor: the 1.0 immediates (filled by
        // the launcher with t) ...
        Piece {
            at: At::Image(F11_CULL_IMMS[0]),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])],
        },
        Piece {
            at: At::Image(F11_CULL_IMMS[1]),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])],
        },
        Piece {
            at: At::Image(F11_CULL_IMMS[2]),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])],
        },
        Piece {
            at: At::Image(F11_CULL_IMMS[3]),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])],
        },
        // ... the same t for the far-object triangle ...
        Piece {
            at: At::Cave(F11_CULL_T),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])], // 1.0 until filled
        },
        Piece {
            at: At::Cave(F11_READ),
            ops: &READ_VIEW,
        },
        // mov edx,[0x5022ec] (far-object extent, a side term) -> jmp ; nop
        Piece {
            at: At::Image(0x004a_0a21),
            ops: &[Op::Jmp(At::Cave(F11_CULL_STUBS)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F11_CULL_STUBS),
            ops: &[
                Op::Raw(&[0x8b, 0x15, 0xec, 0x22, 0x50, 0x00]), // mov edx,[0x5022ec]
                Op::Raw(&[0x50, 0x8b, 0xc2, 0xf7, 0x2d]), // push eax ; mov eax,edx ; imul dword [t]
                Op::Addr(At::Cave(F11_CULL_T)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10, 0x8b, 0xd0, 0x58]), // shrd eax,edx,16 ; mov edx,eax ; pop eax
                Op::Jmp(At::Image(0x004a_0a21 + 6)),
            ],
        },
        // mov edx,[0x5022ec] (far-object extent, a side term) -> jmp ; nop
        Piece {
            at: At::Image(0x004a_0a32),
            ops: &[Op::Jmp(At::Cave(F11_CULL_STUBS + 0x20)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F11_CULL_STUBS + 0x20),
            ops: &[
                Op::Raw(&[0x8b, 0x15, 0xec, 0x22, 0x50, 0x00]), // mov edx,[0x5022ec]
                Op::Raw(&[0x50, 0x8b, 0xc2, 0xf7, 0x2d]), // push eax ; mov eax,edx ; imul dword [t]
                Op::Addr(At::Cave(F11_CULL_T)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10, 0x8b, 0xd0, 0x58]), // shrd eax,edx,16 ; mov edx,eax ; pop eax
                Op::Jmp(At::Image(0x004a_0a32 + 6)),
            ],
        },
    ],
};

const F12_ARS: u32 = 0x1e0;
/// F12's factor, Q16 (0x10000 = 1.0), filled by the launcher.
pub const F12_FACTOR: u32 = 0x224;
const F12_TRIGGER: u32 = 0x230;
const _: () =
    assert!(F12_ARS >= F11_T + 4 && F12_FACTOR >= F12_ARS + 65 && F12_TRIGGER >= F12_FACTOR + 4);
/// The country-1 cap, 200.0 in Q16, compared and stored at two places each.
pub const F12_CAPS: [u32; 4] = [0x0043_5e91, 0x0043_5e98, 0x0043_5f6a, 0x0043_5f71];
pub const F12_CAP: u32 = 0x00c8_0000;

/// Draw distance. Each stage's `.ars` file gives the object wedge, far
/// plane and LOD wedge (`0x4f6838`/`0x4f683c`/`0x4f6840`, usually 150, 150
/// and 100 m), section triggers set the far plane from the table at
/// `0x4ea38c` (50..250 m), and one country caps it at 200 m; the menu's
/// distance options then scale it every frame and the terrain and object
/// culls follow the far plane. F12 multiplies those raw sources by one
/// factor, so the game's own per-frame logic is unchanged: after the `.ars`
/// load (`0x4809a7`, before the shared tail re-applies the projection),
/// where the trigger table is read (`0x403ea7`), and the cap constants
/// (filled by the launcher). The front end is not touched. Opt-in
/// (`draw_distance`).
const F12: Fix = Fix {
    id: "F12",
    what: "longer draw distance (stage distances times a factor)",
    optional: true,
    code: &[
        // mov ecx,[0x4f6838] -> jmp ; nop
        Piece {
            at: At::Image(0x0048_09a7),
            ops: &[Op::Jmp(At::Cave(F12_ARS)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F12_ARS),
            ops: &[
                // [0x4f6838] = [0x4f6838] * factor (Q16)
                Op::Raw(&[0xa1, 0x38, 0x68, 0x4f, 0x00]), // mov eax,[g]
                Op::Raw(&[0xf7, 0x2d]),                   // imul dword [factor]
                Op::Addr(At::Cave(F12_FACTOR)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10]), // shrd eax,edx,16
                Op::Raw(&[0xa3, 0x38, 0x68, 0x4f, 0x00]), // mov [g],eax
                // [0x4f683c] = [0x4f683c] * factor (Q16)
                Op::Raw(&[0xa1, 0x3c, 0x68, 0x4f, 0x00]), // mov eax,[g]
                Op::Raw(&[0xf7, 0x2d]),                   // imul dword [factor]
                Op::Addr(At::Cave(F12_FACTOR)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10]), // shrd eax,edx,16
                Op::Raw(&[0xa3, 0x3c, 0x68, 0x4f, 0x00]), // mov [g],eax
                // [0x4f6840] = [0x4f6840] * factor (Q16)
                Op::Raw(&[0xa1, 0x40, 0x68, 0x4f, 0x00]), // mov eax,[g]
                Op::Raw(&[0xf7, 0x2d]),                   // imul dword [factor]
                Op::Addr(At::Cave(F12_FACTOR)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10]), // shrd eax,edx,16
                Op::Raw(&[0xa3, 0x40, 0x68, 0x4f, 0x00]), // mov [g],eax
                // the shared tail: projection and wedges from the globals
                Op::Jmp(At::Image(0x0048_09cb)),
            ],
        },
        Piece {
            at: At::Cave(F12_FACTOR),
            ops: &[Op::Raw(&[0x00, 0x00, 0x01, 0x00])], // 1.0 until filled
        },
        // mov eax,[eax*4+0x4ea38c] -> jmp ; nop ; nop
        Piece {
            at: At::Image(0x0040_3ea7),
            ops: &[Op::Jmp(At::Cave(F12_TRIGGER)), Op::Raw(&[NOP, NOP])],
        },
        Piece {
            at: At::Cave(F12_TRIGGER),
            ops: &[
                Op::Raw(&[0x8b, 0x04, 0x85, 0x8c, 0xa3, 0x4e, 0x00]), // the table read
                Op::Raw(&[0xf7, 0x2d]),                               // imul dword [factor]
                Op::Addr(At::Cave(F12_FACTOR)),
                Op::Raw(&[0x0f, 0xac, 0xd0, 0x10]), // shrd eax,edx,16
                Op::Jmp(At::Image(0x0040_3eae)),
            ],
        },
        // The cap's immediates; the launcher writes the scaled value.
        Piece {
            at: At::Image(F12_CAPS[0]),
            ops: &[Op::Raw(&F12_CAP.to_le_bytes())],
        },
        Piece {
            at: At::Image(F12_CAPS[1]),
            ops: &[Op::Raw(&F12_CAP.to_le_bytes())],
        },
        Piece {
            at: At::Image(F12_CAPS[2]),
            ops: &[Op::Raw(&F12_CAP.to_le_bytes())],
        },
        Piece {
            at: At::Image(F12_CAPS[3]),
            ops: &[Op::Raw(&F12_CAP.to_le_bytes())],
        },
    ],
};

/// F13's hook table: the exports below (`F13_EXPORTS`) of
/// `cmrhooks.dll`, filled by the launcher after it loads the library. A zero
/// entry makes its stub do nothing, so the game runs as without F13.
pub const F13_TABLE: u32 = 0x250;
/// The exports F13's table holds, in order (`slot::*` are their offsets).
pub const F13_EXPORTS: [&str; 20] = [
    "cmr_hud_begin",
    "cmr_hud_end",
    "cmr_hud_rects",
    "cmr_fe_present",
    "cmr_hud_rect_at",
    "cmr_update_begin",
    "cmr_real_begin",
    "cmr_real_end",
    "cmr_3d_begin",
    "cmr_present",
    "cmr_texture_desc",
    "cmr_texture_loaded",
    "cmr_ease_far",
    "cmr_pedals",
    "cmr_rumble",
    "cmr_3d_end",
    "cmr_pad_buttons",
    "cmr_depth_visible",
    "cmr_hd_load",
    "cmr_hd_load_2d",
];
/// The table's size in bytes (one address per export).
const F13_TABLE_LEN: usize = 4 * F13_EXPORTS.len();
const _: () =
    assert!(F13_TABLE >= F12_TRIGGER + 22 && F13_TABLE + F13_TABLE_LEN as u32 <= F14_CAVE);
/// Offsets of the exports in F13's table.
mod slot {
    pub const HUD: u32 = 0;
    pub const END: u32 = 4;
    pub const RECTS: u32 = 8;
    pub const RECT_AT: u32 = 16;
    pub const UPDATE: u32 = 20;
    pub const REAL: u32 = 24;
    pub const REAL_END: u32 = 28;
    pub const THREE_D: u32 = 32;
    pub const PRESENT: u32 = 36;
    pub const TEXTURE_DESC: u32 = 40;
    pub const TEXTURE_LOADED: u32 = 44;
    pub const EASE_FAR: u32 = 48;
    pub const PEDALS: u32 = 52;
    pub const RUMBLE: u32 = 56;
    pub const THREE_D_END: u32 = 60;
    pub const PAD_BUTTONS: u32 = 64;
    pub const DEPTH_VISIBLE: u32 = 68;
    pub const HD_LOAD: u32 = 72;
    pub const HD_LOAD_2D: u32 = 76;
}

/// With a race resolution the game never had (F10), the in-race 2D (HUD,
/// messages, pause, results, replay, loading screen) mixes 640x480
/// rectangles scaled by W/640, fractions of W/H and absolute pixel tables
/// chosen by width, and text is drawn at the font's native size. F13 keeps a
/// virtual 640x480 screen in the game's display state for the whole race,
/// the size all of its 2D was laid out for, and maps every rectangle drawn
/// by the 2D primitives (sprite and text queues `0x49b420`/`0x49b1d0`,
/// direct blit `0x49e3e0`, panel `0x49e200`) onto a 4:3 canvas centred on
/// the real screen (`cmrhooks.dll`, `sections`, `canvas`); full-screen
/// backdrops and fades span the whole screen. Only the code that needs the
/// real screen runs with it, in "real" sections with scaling paused:
/// display set-up `0x497be0`, 3D device and viewport set-up `0x4a6950`, the
/// clears `0x49ffc0`/`0x49ff80`, the 3D projection `0x435d10` and the scene
/// draw `0x49fe30`. The game updates (`0x443db0`) on a multimedia-timer
/// thread while the main thread renders, so updates and real sections take
/// one lock: an update never sees the real size. The race HUD's layout and
/// draw calls are marked so its side pieces anchor to the screen edges.
/// Applied with F10; the library leaves the game's own resolutions alone.
///
/// The pieces, generated by `hud_code!`:
/// - `calls`: `call target` at `site` goes through a stub that calls the
///   `begin` export, the original, then the `end` export (return address
///   kept at cave `ret`);
/// - `functions`: the same around a whole function, entered at `site`: its
///   first instructions `orig` (`5 + nops` bytes) move into the stub, which
///   continues the function at `resume`;
/// - `rects`: entry hooks on the 2D primitives; their first instructions
///   `orig` move to a stub that lets the library rescale the rectangle
///   arguments (`arg`: the clip index for the queues, the rectangle index
///   for `cmr_hud_rect_at`);
/// - the hook table.
macro_rules! hud_code {
    (
        calls: [$(($site:expr, $target:expr, $begin:expr, $end:expr, $stub:expr, $ret:expr)),* $(,)?],
        entries: [$(($nsite:expr, $nnops:expr, $nslot:expr, $nstub:expr, [$($norig:expr),*], $nresume:expr)),* $(,)?],
        functions: [$(($fsite:expr, $fnops:expr, $fbegin:expr, $fend:expr, $fstub:expr, $fret:expr, [$($forig:expr),*], $fresume:expr)),* $(,)?],
        views: [$(($vsite:expr, $vnops:expr, $vstub:expr, $vret:expr, [$($vorig:expr),*], $vresume:expr)),* $(,)?],
        reads: [$(($rsite:expr, $rnops:expr, $rstub:expr)),* $(,)?],
        rects: [$(($esite:expr, $nops:expr, $estub:expr, $slot:expr, $arg:expr, [$($orig:expr),*], $resume:expr)),* $(,)?] $(,)?
    ) => {
        &[
            $(
                Piece {
                    at: At::Image($site),
                    ops: &[Op::Call(At::Cave($stub))],
                },
                Piece {
                    at: At::Cave($stub),
                    ops: hud_code!(@around $begin, $end, $ret, Op::Call(At::Image($target)), []),
                },
            )*
            $(
                Piece {
                    at: At::Image($fsite),
                    ops: &[Op::Jmp(At::Cave($fstub)), Op::Raw(&[NOP; $fnops])],
                },
                Piece {
                    at: At::Cave($fstub),
                    // the moved instructions follow the 46-byte stub
                    ops: hud_code!(@around $fbegin, $fend, $fret,
                        Op::Call(At::Cave($fstub + 46)),
                        [$($forig,)* Op::Jmp(At::Image($fresume))]),
                },
            )*
            $(
                Piece {
                    at: At::Image($nsite),
                    ops: &[Op::Jmp(At::Cave($nstub)), Op::Raw(&[NOP; $nnops])],
                },
                Piece {
                    at: At::Cave($nstub),
                    ops: &[
                        Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[export]
                        Op::Addr(At::Cave(F13_TABLE + $nslot)),
                        Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xd1, 0x61]), // test ; jz ; call ecx ; popad
                        $($norig,)*
                        Op::Jmp(At::Image($nresume)),
                    ],
                },
            )*
            $(
                Piece {
                    at: At::Image($vsite),
                    ops: &[Op::Jmp(At::Cave($vstub)), Op::Raw(&[NOP; $vnops])],
                },
                Piece {
                    at: At::Cave($vstub),
                    // the moved instructions follow the 56-byte stub
                    ops: hud_code!(@view $vret, Op::Call(At::Cave($vstub + 56)),
                        [$($vorig,)* Op::Jmp(At::Image($vresume))]),
                },
            )*
            $(
                Piece {
                    at: At::Image($esite),
                    ops: &[Op::Jmp(At::Cave($estub)), Op::Raw(&[NOP; $nops])],
                },
                Piece {
                    at: At::Cave($estub),
                    ops: &[
                        Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[export]
                        Op::Addr(At::Cave(F13_TABLE + $slot)),
                        Op::Raw(&[0x85, 0xc9, 0x74, 0x09]), // test ecx,ecx ; jz +9
                        Op::Raw(&[0x6a, $arg]),             // push arg (imm8, below 0x80)
                        Op::Raw(&[0x8d, 0x44, 0x24, 0x28]), // lea eax,[esp+0x28] (argument 1)
                        Op::Raw(&[0x50, 0xff, 0xd1]),       // push eax ; call ecx
                        Op::Raw(&[0x61]),                   // popad
                        $($orig,)*                          // the replaced instructions
                        Op::Jmp(At::Image($resume)),
                    ],
                },
            )*
            Piece {
                at: At::Cave(F13_TABLE),
                // the F13_EXPORTS addresses, filled by the launcher
                ops: &[Op::Raw(&[0; F13_TABLE_LEN])],
            },
            $(
                Piece {
                    at: At::Image($rsite),
                    ops: &[Op::Call(At::Cave($rstub)), Op::Raw(&[NOP; $rnops])],
                },
            )*
            Piece {
                at: At::Cave(F13_VIEW),
                ops: &[Op::Raw(&[0; 12])],
            },
            Piece {
                at: At::Cave(F13_READ_EAX),
                ops: &READ_VIEW,
            },
            Piece {
                at: At::Cave(F13_READ_ECX),
                ops: &[
                    Op::Raw(&[0x50]), // push eax
                    Op::Call(At::Cave(F13_READ_EAX)),
                    Op::Raw(&[0x8b, 0xc8, 0x58, 0xc3]), // mov ecx,eax ; pop eax ; ret
                ],
            },
            Piece {
                at: At::Cave(F13_READ_EDX),
                ops: &[
                    Op::Raw(&[0x50]), // push eax
                    Op::Call(At::Cave(F13_READ_EAX)),
                    Op::Raw(&[0x8b, 0xd0, 0x58, 0xc3]), // mov edx,eax ; pop eax ; ret
                ],
            },
        ]
    };
    // A 56-byte stub around 3D work: `cmr_3d_begin(view)`, `call`,
    // `cmr_3d_end(view)`, back to the saved return address; then `tail`.
    (@view $ret:expr, $call:expr, [$($tail:expr),*]) => {
        &[
            Op::Raw(&[0x8f, 0x05]), // pop dword [ret]
            Op::Addr(At::Cave($ret)),
            Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_3d_begin]
            Op::Addr(At::Cave(F13_TABLE + slot::THREE_D)),
            Op::Raw(&[0x85, 0xc9, 0x74, 0x07, 0x68]), // test ; jz +7 ; push view
            Op::Addr(At::Cave(F13_VIEW)),
            Op::Raw(&[0xff, 0xd1, 0x61]), // call ecx ; popad
            $call,
            Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_3d_end]
            Op::Addr(At::Cave(F13_TABLE + slot::THREE_D_END)),
            Op::Raw(&[0x85, 0xc9, 0x74, 0x07, 0x68]), // test ; jz +7 ; push view
            Op::Addr(At::Cave(F13_VIEW)),
            Op::Raw(&[0xff, 0xd1, 0x61, 0xff, 0x35]), // call ecx ; popad ; push dword [ret]
            Op::Addr(At::Cave($ret)),
            Op::Raw(&[0xc3]),
            $($tail,)*
        ]
    };
    // A 46-byte stub: begin, `call`, end, back to the saved return address;
    // then `tail`.
    (@around $begin:expr, $end:expr, $ret:expr, $call:expr, [$($tail:expr),*]) => {
        &[
            Op::Raw(&[0x8f, 0x05]), // pop dword [ret]
            Op::Addr(At::Cave($ret)),
            Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[begin]
            Op::Addr(At::Cave(F13_TABLE + $begin)),
            Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xd1, 0x61]), // test ; jz ; call ecx ; popad
            $call,
            Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[end]
            Op::Addr(At::Cave(F13_TABLE + $end)),
            Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xd1, 0x61]), // test ; jz ; call ecx ; popad
            Op::Raw(&[0xff, 0x35]),                               // push dword [ret] ; ret
            Op::Addr(At::Cave($ret)),
            Op::Raw(&[0xc3]),
            $($tail,)*
        ]
    };
}

/// Cave areas of F13: call stubs (0x30 apart), function stubs (0x40 apart),
/// return slots, rectangle hooks (0x30 apart).
const F13_CALLS: u32 = 0x600;
const F13_FUNCTIONS: u32 = F13_CALLS + 6 * 0x30;
const F13_RETS: u32 = 0x980;
const F13_RECTS: u32 = 0x9d0;
/// The 3D view block (`depth`, `main` thread id, `shadow` pointer; filled by
/// `cmr_3d_begin`) and the stubs the 3D code's screen-size reads call, one
/// per register: the shadow's address for the main thread inside 3D work in
/// a race, else the game's display state (`[0x502518]`).
pub const F13_VIEW: u32 = 0x300;
const F13_READ_EAX: u32 = 0x310;
const F13_READ_ECX: u32 = 0x340;
const F13_READ_EDX: u32 = 0x370;
/// eax = the display state the calling code should see: the real-size
/// shadow for the main thread inside 3D work in a race (F13's view block),
/// else the game's own. Used by F13's read stubs and F11's own copy; with
/// F13 off the view block stays zero and this is a plain read.
const READ_VIEW: [Op; 10] = [
    Op::Raw(&[0xa1]), // mov eax,[view.depth]
    Op::Addr(At::Cave(F13_VIEW)),
    Op::Raw(&[0x85, 0xc0, 0x74, 0x14]), // test eax,eax ; jz game
    Op::Raw(&[0x64, 0xa1, 0x24, 0x00, 0x00, 0x00]), // mov eax,fs:[0x24] (thread id)
    Op::Raw(&[0x3b, 0x05]),             // cmp eax,[view.main]
    Op::Addr(At::Cave(F13_VIEW + 4)),
    Op::Raw(&[0x75, 0x06, 0xa1]), // jne game ; mov eax,[view.shadow]
    Op::Addr(At::Cave(F13_VIEW + 8)),
    Op::Raw(&[0xc3]),                               // ret
    Op::Raw(&[0xa1, 0x18, 0x25, 0x50, 0x00, 0xc3]), // game: mov eax,[0x502518] ; ret
];
/// F11's copy of `READ_VIEW`.
const F11_READ: u32 = 0xdf0;
const _: () = assert!(F11_READ >= F13_VIEWS + 4 * 0x50);

/// Cave area of the 3D work stubs (0x50 apart).
const F13_VIEWS: u32 = 0xcb0;
const _: () = assert!(
    F13_CALLS >= F16_RET + 4
        && F13_FUNCTIONS + 7 * 0x40 <= F13_RETS
        && F13_RETS + 13 * 4 <= F13_RECTS
);

const F13: Fix = Fix {
    id: "F13",
    what: "HUD, messages, results and loading screen scaled to the race resolution (needs cmrhooks.dll)",
    optional: true,
    code: hud_code! {
        calls: [
            // the race HUD: side pieces anchored to the screen edges
            (0x0042_22ab, 0x0046_2230, slot::HUD, slot::END, F13_CALLS, F13_RETS),            // layout (race start)
            (0x0042_22e7, 0x0047_fda0, slot::HUD, slot::END, F13_CALLS + 0x30, F13_RETS + 4), // timer font
            (0x0042_2e1c, 0x0046_4930, slot::HUD, slot::END, F13_CALLS + 0x60, F13_RETS + 8), // draw, each view
            (0x0047_65b2, 0x0046_4930, slot::HUD, slot::END, F13_CALLS + 0x90, F13_RETS + 12), // draw, replay
            (0x0042_2ea6, 0x0047_fe50, slot::HUD, slot::END, F13_CALLS + 0xc0, F13_RETS + 16), // stage time
            (0x0047_65c8, 0x0047_fe50, slot::HUD, slot::END, F13_CALLS + 0xf0, F13_RETS + 20), // stage time, replay
        ],
        entries: [
            // the game update (main or timer thread): mov eax,[0x96efcc]
            (0x0044_3db0, 0, slot::UPDATE, F13_FUNCTIONS, [Op::Raw(&[0xa1, 0xcc, 0xef, 0x96, 0x00])], 0x0044_3db5),
        ],
        functions: [
            // set-up, while a stage loads or the menus return: the 3D device
            // and viewport (push 0x7c ; call 0x4990d0) and the display (push
            // ebx ; push ebp ; push esi ; xor esi,esi ; push edi)
            (0x004a_6950, 2, slot::REAL, slot::REAL_END, F13_FUNCTIONS + 0x180, F13_RETS + 48,
                [Op::Raw(&[0x6a, 0x7c]), Op::Call(At::Image(0x0049_90d0))], 0x004a_6957),
            (0x0049_7be0, 1, slot::REAL, slot::REAL_END, F13_FUNCTIONS + 0x40, F13_RETS + 28,
                [Op::Raw(&[0x53, 0x55, 0x56, 0x33, 0xf6, 0x57])], 0x0049_7be6),
        ],
        views: [
            // per-frame 3D work: the clears (sub esp,0x10 ; call 0x4aa0c0 /
            // sub esp,0x10 ; xor eax,eax), the projection (push ebp ; mov
            // ebp,esp ; sub esp,8), the scene draw (mov eax,[0x50276c])
            (0x0049_ffc0, 3, F13_VIEWS, F13_RETS + 32,
                [Op::Raw(&[0x83, 0xec, 0x10]), Op::Call(At::Image(0x004a_a0c0))], 0x0049_ffc8),
            (0x0049_ff80, 0, F13_VIEWS + 0x50, F13_RETS + 36,
                [Op::Raw(&[0x83, 0xec, 0x10, 0x33, 0xc0])], 0x0049_ff85),
            (0x0043_5d10, 1, F13_VIEWS + 0xa0, F13_RETS + 40,
                [Op::Raw(&[0x55, 0x8b, 0xec, 0x83, 0xec, 0x08])], 0x0043_5d16),
            (0x0049_fe30, 0, F13_VIEWS + 0xf0, F13_RETS + 44,
                [Op::Raw(&[0xa1, 0x6c, 0x27, 0x50, 0x00])], 0x0049_fe35),
        ],
        reads: [
            // the 3D code's screen-size reads (mov reg,[0x502518]): the
            // projection, the clears, the scene draw, its bounds check and
            // the point projection it shares with the update thread
            (0x0043_5d3f, 0, F13_READ_EAX),
            (0x0049_ffd4, 0, F13_READ_EAX),
            (0x0049_ff8f, 0, F13_READ_EAX),
            (0x004a_1683, 0, F13_READ_EAX),
            (0x0048_f896, 0, F13_READ_EAX),
            (0x004a_2ab0, 1, F13_READ_ECX),
            (0x004b_0679, 1, F13_READ_ECX),
            (0x004b_069f, 1, F13_READ_EDX),
        ],
        rects: [
            // sprite and text queues: mov al,[esp+..] ; push esi
            (0x0049_b420, 0, F13_RECTS, slot::RECTS, 8, [Op::Raw(&[0x8a, 0x44, 0x24, 0x10, 0x56])], 0x0049_b425),
            (0x0049_b1d0, 0, F13_RECTS + 0x30, slot::RECTS, 9, [Op::Raw(&[0x8a, 0x44, 0x24, 0x14, 0x56])], 0x0049_b1d5),
            // direct blit (destination rectangle = argument 4): sub esp,0x12c
            (0x0049_e3e0, 1, F13_RECTS + 0x60, slot::RECT_AT, 3,
                [Op::Raw(&[0x81, 0xec, 0x2c, 0x01, 0x00, 0x00])], 0x0049_e3e6),
            // panel (rectangle = argument 2): sub esp,0x18 ; call 0x498af0
            (0x0049_e200, 3, F13_RECTS + 0x90, slot::RECT_AT, 1,
                [Op::Raw(&[0x83, 0xec, 0x18]), Op::Call(At::Image(0x0049_8af0))], 0x0049_e208),
        ],
    },
};

const F14_CAVE: u32 = 0x4b0;
const _: () = assert!(
    0x480 + 46 <= F14_CAVE && F14_CAVE + 23 <= 0x4d0,
    "F13 stubs and F14 apart"
);

/// In a race the control map (`FUN_00449580`) pauses on
/// `GetAsyncKeyState(VK_ESCAPE)` (`0x449714`), so a pad has no pause. F14
/// wraps that call: Escape also reads as held while the player's device
/// holds pad button 8 (Start on Xbox-style pads; bit `0x800` of the merged
/// level mask at `[ebp-4]`, a bit no keyboard binding uses). The game's own
/// latch and pause menu do the rest. Grouped with F7 (`gamepad_axes`).
const F14: Fix = Fix {
    id: "F14",
    what: "gamepads: Start pauses the race",
    optional: true,
    code: &[
        // push 0x1b ; call [GetAsyncKeyState] -> call the cave ; 3 x nop
        Piece {
            at: At::Image(0x0044_9714),
            ops: &[Op::Call(At::Cave(F14_CAVE)), Op::Raw(&[NOP; 3])],
        },
        Piece {
            at: At::Cave(F14_CAVE),
            ops: &[
                Op::Raw(&[0x6a, 0x1b, 0xff, 0x15, 0x6c, 0x62, 0x4e, 0x00]), // the original call
                Op::Raw(&[0xf7, 0x45, 0xfc, 0x00, 0x08, 0x00, 0x00]), // test dword [ebp-4],0x800
                Op::Raw(&[0x74, 0x05]),                               // jz +5
                Op::Raw(&[0x0d, 0x00, 0x80, 0x00, 0x00]),             // or eax,0x8000 (held)
                Op::Raw(&[0xc3]),                                     // ret
            ],
        },
    ],
};

/// The CPU clock check (`FUN_004ad240`) raises the process to real-time
/// priority and busy-waits one second of `QueryPerformanceCounter` while
/// counting `rdtsc` cycles in 32 bits, retrying whenever that count wraps.
/// On CPUs above ~2 GHz it wraps in under a second, so the check repeats
/// for several seconds, and the game calls it three times (start-up, the
/// default detail settings at `0x4b999c`, the exit-time profiler) — the long
/// black/legal screen at start and a slow quit. Its result (Hz, a multiple
/// of 10^6) is only compared with 198 MHz for the default detail level and
/// printed by the profiler. F15 returns 1 GHz at once: a fast CPU, which is
/// what the original measured on the hardware it was made for.
const F15: Fix = Fix {
    id: "F15",
    what: "no multi-second CPU clock check at start (and at exit)",
    optional: false,
    code: &[Piece {
        at: At::Image(0x004a_d240),
        // mov eax,1000000000 ; ret
        ops: &[Op::Raw(&[0xb8, 0x00, 0xca, 0x9a, 0x3b, 0xc3])],
    }],
};

const F16_MODE: u32 = 0x540;
const F16_PRESENT: u32 = 0x570;
const F16_LEGAL: u32 = 0x5a0;
const F16_RET: u32 = 0x5d0;
const _: () = assert!(
    F16_MODE >= 0x500 + 46
        && F16_PRESENT >= F16_MODE + 33
        && F16_LEGAL >= F16_PRESENT + 34
        && F16_RET >= F16_LEGAL + 37
);

/// The front end (intro, menus) is drawn at 640x480 and can't be drawn
/// larger; with a race resolution it sat small in a corner of the screen.
/// F16 sets the front end's screen mode to the race resolution while the
/// game keeps its 640x480 sizes (`0x497db9`, the 8-bit/front-end branch of
/// the display init, only when F10's size is filled), and replaces its
/// `Flip` (`0x498705`) with `cmr_fe_present`, which copies the 640x480
/// picture to the screen enlarged with sharp pixels into a centred 4:3 area.
/// The library returns 0 (and the game flips as before) whenever the game
/// already draws at the screen size, e.g. in races. The legal screen is
/// copied straight to the screen without a flip, so it is presented once
/// right after it is drawn (`FUN_004c2fb0`, called at `0x4c1e5f`). Applied
/// with F13.
const F16: Fix = Fix {
    id: "F16",
    what: "intro and menus enlarged to the screen (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // mov edx,[0x5022f4] ; cmp edx,esi -> jmp ; 3 x nop
        Piece {
            at: At::Image(0x0049_7db9),
            ops: &[Op::Jmp(At::Cave(F16_MODE)), Op::Raw(&[NOP; 3])],
        },
        Piece {
            at: At::Cave(F16_MODE),
            ops: &[
                Op::Raw(&[0x83, 0x3d]), // cmp dword [width],0
                Op::Addr(At::Cave(F10_SIZE)),
                Op::Raw(&[0x00, 0x74, 0x0b]), // ; je +11
                Op::Raw(&[0x8b, 0x0d]),       // mov ecx,[width]  (SetDisplayMode's W)
                Op::Addr(At::Cave(F10_SIZE)),
                Op::Raw(&[0xa1]), // mov eax,[height] (its H)
                Op::Addr(At::Cave(F10_SIZE + 4)),
                // the replaced instructions
                Op::Raw(&[0x8b, 0x15, 0xf4, 0x22, 0x50, 0x00, 0x3b, 0xd6]),
                Op::Jmp(At::Image(0x0049_7dc1)),
            ],
        },
        // call 0x4c2fb0 (draws the legal screen) -> call the stub
        Piece {
            at: At::Image(0x004c_1e5f),
            ops: &[Op::Call(At::Cave(F16_LEGAL))],
        },
        Piece {
            at: At::Cave(F16_LEGAL),
            ops: &[
                Op::Raw(&[0x8f, 0x05]), // pop dword [ret]
                Op::Addr(At::Cave(F16_RET)),
                Op::Call(At::Image(0x004c_2fb0)), // its argument is still pushed (stdcall)
                Op::Raw(&[0x60, 0xa1]),           // pushad ; mov eax,[fe_present]
                Op::Addr(At::Cave(F13_TABLE + 12)),
                Op::Raw(&[0x85, 0xc0, 0x74, 0x08]), // test eax,eax ; jz +8
                Op::Raw(&[0xff, 0x35, 0x18, 0x25, 0x50, 0x00]), // push dword [0x502518] (display)
                Op::Raw(&[0xff, 0xd0]),             // call eax
                Op::Raw(&[0x61, 0xff, 0x35]),       // popad ; push dword [ret]
                Op::Addr(At::Cave(F16_RET)),
                Op::Raw(&[0xc3]), // ret
            ],
        },
        Piece {
            at: At::Cave(F16_RET),
            ops: &[Op::Raw(&[0; 4])],
        },
    ],
};

/// The fixes that call into `cmrhooks.dll` (off without it).
pub const HOOK_FIXES: [&str; 10] = [
    "F13", "F16", "F17", "F18", "F20", "F21", "F22", "F23", "F26", "F27",
];

/// RUN JOYSTICK PANEL (a controls screen, `0x4cdbc0`) calls `0x49cb90`,
/// which tells the game window it lost focus and runs `control joy.cpl`
/// hidden, expecting Windows to bring the game back when the panel
/// closes. Under Wine (and gamescope) nothing brings it back and the game
/// stays frozen (C6). F24 makes the item do nothing there; the launcher
/// applies it only under Wine, so Windows keeps the panel.
const F24: Fix = Fix {
    id: "F24",
    what: "RUN JOYSTICK PANEL doesn't freeze the game under Wine",
    optional: true,
    // mov eax,[hwnd] -> ret
    code: &[Piece {
        at: At::Image(0x0049_cb90),
        ops: &[Op::Raw(&[0xc3])],
    }],
};

/// The game asks for a 16-bit depth buffer (`0x4a6e97`, the only one).
/// With a longer view (F12) its steps grow too coarse far away and distant
/// hills flicker through each other. F25 asks for 24 bits (256 times finer);
/// F26's depth test reads them. Grouped with F12 (`draw_distance`).
const F25: Fix = Fix {
    id: "F25",
    what: "24-bit depth buffer for longer views",
    optional: true,
    // mov dword [esp+0x24],0x10 (dwZBufferBitDepth) -> 24
    code: &[Piece {
        at: At::Image(0x004a_6e9b),
        ops: &[Op::Raw(&[0x18])],
    }],
};

const F26_CAVE: u32 = 0xe60;
const F27_CAVE: u32 = 0xe80;
const F27_TRAMPOLINE: u32 = F27_CAVE + 28;
const _: () = assert!(F27_CAVE >= F26_CAVE + 25);
/// The 2D surface loader's stub, after the texture loader's 39 bytes.
const F27_CAVE_2D: u32 = F27_CAVE + 40;
const F27_TRAMPOLINE_2D: u32 = F27_CAVE_2D + 32;

/// HD art (docs/UPSCALING.md). Every bitmap the game loads goes through
/// `0x499ec0(bmp, flags)` with the whole BMP in memory. F27 sends the load
/// to `cmr_hd_load`, which runs the game's own load (through a trampoline)
/// and, for a texture with an upscaled version made by `cmr-hd`, a second
/// load of that version, its twin. The race HUD is drawn with 2D blits from
/// these surfaces, through F13's blit hook, which copies from the twin with
/// the source rectangle scaled. The game's own surfaces, sizes and layout
/// are untouched. The front end's images (logos, cars, buttons) load as
/// plain surfaces through `0x499320(record, bmp, flags)`; F27 sends those
/// to `cmr_hd_load_2d` in the same way, and the menu picture draws their
/// twins at the screen's size (`cmr_fe_present`). Opt-in (`hd_art`).
const F27: Fix = Fix {
    id: "F27",
    what: "HD art: upscaled race HUD textures from the hd_art folder (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // the loader: mov eax,[esp+8] ; test al,1 -> jmp ; nop
        Piece {
            at: At::Image(0x0049_9ec0),
            ops: &[Op::Jmp(At::Cave(F27_CAVE)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F27_CAVE),
            ops: &[
                Op::Raw(&[0x8b, 0x0d]), // mov ecx,[cmr_hd_load]
                Op::Addr(At::Cave(F13_TABLE + slot::HD_LOAD)),
                // test ecx,ecx ; jz trampoline ; push flags ; push bmp ;
                // push trampoline ; call ecx ; ret 8 (its result is ours)
                Op::Raw(&[0x85, 0xc9, 0x74, 0x12]),
                Op::Raw(&[0xff, 0x74, 0x24, 0x08, 0xff, 0x74, 0x24, 0x08, 0x68]),
                Op::Addr(At::Cave(F27_TRAMPOLINE)),
                Op::Raw(&[0xff, 0xd1, 0xc2, 0x08, 0x00]),
                // the trampoline: the game's loader, from its moved start
                Op::Raw(&[0x8b, 0x44, 0x24, 0x08, 0xa8, 0x01]),
                Op::Jmp(At::Image(0x0049_9ec6)),
            ],
        },
        // the 2D loader: sub esp,0x3b0 -> jmp ; nop
        Piece {
            at: At::Image(0x0049_9320),
            ops: &[Op::Jmp(At::Cave(F27_CAVE_2D)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F27_CAVE_2D),
            ops: &[
                Op::Raw(&[0x8b, 0x0d]), // mov ecx,[cmr_hd_load_2d]
                Op::Addr(At::Cave(F13_TABLE + slot::HD_LOAD_2D)),
                // test ecx,ecx ; jz trampoline ; push flags ; push bmp ;
                // push record ; push trampoline ; call ecx ; ret 12
                Op::Raw(&[0x85, 0xc9, 0x74, 0x16]),
                Op::Raw(&[0xff, 0x74, 0x24, 0x0c, 0xff, 0x74, 0x24, 0x0c]),
                Op::Raw(&[0xff, 0x74, 0x24, 0x0c, 0x68]),
                Op::Addr(At::Cave(F27_TRAMPOLINE_2D)),
                Op::Raw(&[0xff, 0xd1, 0xc2, 0x0c, 0x00]),
                // the trampoline: the game's 2D loader, from its moved start
                Op::Raw(&[0x81, 0xec, 0xb0, 0x03, 0x00, 0x00]),
                Op::Jmp(At::Image(0x0049_9326)),
            ],
        },
    ],
};

/// The game's depth test `0x49fa70(x, y)` (is anything drawn in front at a
/// screen point; the only place depth pixels are read) locks the whole
/// depth buffer to read one value. Under Wine with a GPU that copies every
/// depth value back each frame (24 fps at 4K instead of 60). F26 sends it
/// to `cmr_depth_visible`, which gives the same answer from that one pixel
/// (16 or 24-bit depth); without the hook library the game's own test runs.
const F26: Fix = Fix {
    id: "F26",
    what: "the depth test reads one pixel, not the whole depth buffer (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // mov eax,[0x50283c] ; sub esp,0x6c -> jmp ; 3 x nop
        Piece {
            at: At::Image(0x0049_fa70),
            ops: &[Op::Jmp(At::Cave(F26_CAVE)), Op::Raw(&[NOP; 3])],
        },
        Piece {
            at: At::Cave(F26_CAVE),
            ops: &[
                Op::Raw(&[0x8b, 0x0d]), // mov ecx,[cmr_depth_visible]
                Op::Addr(At::Cave(F13_TABLE + slot::DEPTH_VISIBLE)),
                // test ecx,ecx ; jz +2 ; jmp ecx (same arguments, its ret 8)
                Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xe1]),
                // the game's own test: the moved instructions, then on
                Op::Raw(&[0xa1, 0x3c, 0x28, 0x50, 0x00, 0x83, 0xec, 0x6c]),
                Op::Jmp(At::Image(0x0049_fa78)),
            ],
        },
    ],
};

const F23_CAVE: u32 = 0xe20;
/// F23's map: for each of the game's 12 pad buttons, the physical button
/// (1-based) that feeds it, 0 for its own; filled by the launcher.
pub const F23_MAP: u32 = 0xe50;
const _: () = assert!(F23_CAVE >= F11_READ + 35 && F23_MAP >= F23_CAVE + 34);

/// The game turns a pad's physical button n into its button n
/// (`0x49cef6`, in the joystick read every menu and race uses). F23 hands
/// the buttons to `cmr_pad_buttons` first, which applies the player's
/// button layout (`buttons`, E11) and adds the d-pad as directions (C5).
/// Grouped with F7 (`gamepad_axes`).
const F23: Fix = Fix {
    id: "F23",
    what: "pad button layout and d-pad (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // mov eax,[esp+0x28] (buttons) ; test al,1 -> jmp ; nop
        Piece {
            at: At::Image(0x0049_cef6),
            ops: &[Op::Jmp(At::Cave(F23_CAVE)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F23_CAVE),
            ops: &[
                Op::Raw(&[0x8b, 0x44, 0x24, 0x28, 0x8b, 0x0d]), // mov eax,[esp+0x28] ; mov ecx,[pad]
                Op::Addr(At::Cave(F13_TABLE + slot::PAD_BUTTONS)),
                Op::Raw(&[0x85, 0xc9, 0x74, 0x0d, 0x68]), // test ecx,ecx ; jz +13 ; push map
                Op::Addr(At::Cave(F23_MAP)),
                // push esi (the record) ; lea eax,[esp+0x10] (JOYINFOEX) ;
                // push eax ; call ecx
                Op::Raw(&[0x56, 0x8d, 0x44, 0x24, 0x10, 0x50, 0xff, 0xd1]),
                Op::Raw(&[0xa8, 0x01]), // test al,1 (moved)
                Op::Jmp(At::Image(0x0049_cefc)),
            ],
        },
        Piece {
            at: At::Cave(F23_MAP),
            ops: &[Op::Raw(&[0; 12])],
        },
    ],
};

/// F17's settings block: frame-rate cap (frames per second, 0 none), the
/// `Flip` flags (0: the game's own) and the rumble strength (percent, F22),
/// filled by the launcher.
pub const F17_SETTINGS: u32 = 0x5e0;
const F17_PRESENT: u32 = F16_PRESENT;
const _: () = assert!(
    F17_PRESENT + 39 <= F16_LEGAL && F17_SETTINGS >= F16_RET + 4 && F17_SETTINGS + 8 <= F13_CALLS
);

/// Every frame, menus and races, is shown by `0x498660` with `Flip`
/// (`0x49870f`). F17 hands that moment to `cmr_present` (`cmrhooks.dll`):
/// it waits for the frame-rate cap, enlarges the front end's picture (F16)
/// and flips with the chosen vsync; without the library, or with neither
/// set, the game flips as it always did. Applied with F16 or when a cap or
/// vsync choice is set.
const F17: Fix = Fix {
    id: "F17",
    what: "frame-rate cap and vsync, front-end picture shown enlarged (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // mov eax,[esi+0x10] ; push 1 ; push 0 ; mov ecx,[eax] ; push eax ;
        // call [ecx+0x2c] (Flip) -> jmp ; 8 x nop
        Piece {
            at: At::Image(0x0049_8705),
            ops: &[Op::Jmp(At::Cave(F17_PRESENT)), Op::Raw(&[NOP; 8])],
        },
        Piece {
            at: At::Cave(F17_PRESENT),
            ops: &[
                Op::Raw(&[0xa1]), // mov eax,[cmr_present]
                Op::Addr(At::Cave(F13_TABLE + slot::PRESENT)),
                Op::Raw(&[0x85, 0xc0, 0x74, 0x0c]), // test eax,eax ; jz +12
                Op::Raw(&[0x68]),                   // push settings
                Op::Addr(At::Cave(F17_SETTINGS)),
                Op::Raw(&[0x56, 0xff, 0xd0]), // push esi (display) ; call eax
                Op::Raw(&[0x85, 0xc0, 0x75, 0x0d]), // test eax,eax ; jnz +13 (shown)
                // the replaced Flip
                Op::Raw(&[
                    0x8b, 0x46, 0x10, 0x6a, 0x01, 0x6a, 0x00, 0x8b, 0x08, 0x50, 0xff, 0x51, 0x2c,
                ]),
                Op::Jmp(At::Image(0x0049_8712)),
            ],
        },
        Piece {
            at: At::Cave(F17_SETTINGS),
            ops: &[Op::Raw(&[0; 12])],
        },
    ],
};

/// Cave area of F18's stubs (0x30 apart).
const F18_CAVE: u32 = 0xab0;
const _: () = assert!(F18_CAVE >= F13_RECTS + 0x90 + 34);

/// F18's pieces: the texture description hook, one stub after each
/// texture upload (`IDirect3DTexture2::Load`: the replaced instructions
/// `orig` end with the Load call; then `cmr_texture_loaded(dst)` with the
/// destination pushed by `push_dst`; back at `resume`), and the trilinear
/// minification stub.
macro_rules! f18_code {
    (loads: [$(($site:expr, $nops:expr, $stub:expr, [$($orig:expr),*], [$($push_dst:expr),*], $resume:expr)),* $(,)?]) => {
        &[
            // mov eax,[0x502518] -> jmp
            Piece {
                at: At::Image(0x0049_eb66),
                ops: &[Op::Jmp(At::Cave(F18_CAVE))],
            },
            Piece {
                at: At::Cave(F18_CAVE),
                ops: &[
                    Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_texture_desc]
                    Op::Addr(At::Cave(F13_TABLE + slot::TEXTURE_DESC)),
                    Op::Raw(&[0x85, 0xc9, 0x74, 0x07]), // test ecx,ecx ; jz +7
                    Op::Raw(&[0x8d, 0x44, 0x24, 0x34]), // lea eax,[esp+0x34] (the description)
                    Op::Raw(&[0x50, 0xff, 0xd1]),       // push eax ; call ecx
                    Op::Raw(&[0x61, 0xa1, 0x18, 0x25, 0x50, 0x00]), // popad ; mov eax,[0x502518]
                    Op::Jmp(At::Image(0x0049_eb6b)),
                ],
            },
            $(
                Piece {
                    at: At::Image($site),
                    ops: &[Op::Jmp(At::Cave($stub)), Op::Raw(&[NOP; $nops])],
                },
                Piece {
                    at: At::Cave($stub),
                    ops: &[
                        Op::Raw(&[$($orig),*]),
                        Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_texture_loaded]
                        Op::Addr(At::Cave(F13_TABLE + slot::TEXTURE_LOADED)),
                        Op::Raw(&[0x85, 0xc9, 0x74, 0x05]), // test ecx,ecx ; jz +5 (push + call)
                        Op::Raw(&[$($push_dst),*, 0xff, 0xd1]), // push dst ; call ecx
                        Op::Raw(&[0x61]),                       // popad
                        Op::Jmp(At::Image($resume)),
                    ],
                },
            )*
            // push esi ; push 0x12 ; mov eax,[edx+0x10] (TEXTUREMIN = mode) -> jmp ; nop
            Piece {
                at: At::Image(0x004a_243e),
                ops: &[Op::Jmp(At::Cave(F18_CAVE + 0xc0)), Op::Raw(&[NOP; 1])],
            },
            Piece {
                at: At::Cave(F18_CAVE + 0xc0),
                ops: &[
                    Op::Raw(&[0x83, 0xfe, 0x02, 0x75, 0x04]), // cmp esi,2 (linear) ; jne +4
                    Op::Raw(&[0x6a, 0x06, 0xeb, 0x01]),       // push 6 (LINEARMIPLINEAR) ; jmp +1
                    Op::Raw(&[0x56]),                         // push esi
                    Op::Raw(&[0x6a, 0x12, 0x8b, 0x42, 0x10]), // push 0x12 ; mov eax,[edx+0x10]
                    Op::Jmp(At::Image(0x004a_2444)),
                ],
            },
        ]
    };
}

/// Race textures are uploaded once (`IDirect3DTexture2::Load` from a
/// system-memory copy) with no smaller levels, so they shimmer far away.
/// F18 (`texture_filter = smooth`) gives video-memory textures a mipmap
/// chain at the texture helper's `CreateSurface` (`0x49eb66`), fills it after
/// each of the three uploads (`cmrhooks.dll`, `mipmap`), and turns the
/// game's linear minification into trilinear (`0x4a243e`, in its filter
/// setter `0x4a23e0`); draws it sets to nearest (sprites, HUD) stay sharp.
const F18: Fix = Fix {
    id: "F18",
    what: "smooth textures: mipmaps and trilinear filtering (needs cmrhooks.dll)",
    optional: true,
    code: f18_code! {
        loads: [
            // mov eax,[ebx+0x18] ; mov edx,[esi] ; push edx ; push eax ;
            // mov ecx,[eax] ; call [ecx+0x14]; dst = [ebx+0x18]
            (0x0049_a142, 7, F18_CAVE + 0x30,
                [0x8b, 0x43, 0x18, 0x8b, 0x16, 0x52, 0x50, 0x8b, 0x08, 0xff, 0x51, 0x14],
                [0xff, 0x73, 0x18], 0x0049_a14e),
            // mov eax,[ebx] ; mov ecx,[ebp] ; push ecx ; push eax ;
            // mov edx,[eax] ; call [edx+0x14]; dst = [ebx]
            (0x0049_a448, 7, F18_CAVE + 0x60,
                [0x8b, 0x03, 0x8b, 0x4d, 0x00, 0x51, 0x50, 0x8b, 0x10, 0xff, 0x52, 0x14],
                [0xff, 0x33, 0x90], 0x0049_a454),
            // mov eax,[esi] ; mov edx,[edi] ; push edx ; push eax ;
            // mov ecx,[eax] ; call [ecx+0x14]; dst = [esi]
            (0x0049_acfa, 6, F18_CAVE + 0x90,
                [0x8b, 0x06, 0x8b, 0x17, 0x52, 0x50, 0x8b, 0x08, 0xff, 0x51, 0x14],
                [0xff, 0x36, 0x90], 0x0049_ad05),
        ]
    },
};

/// The game filters textures whenever the device can (`0x4a9fe0` returns the
/// capability). F19 (`texture_filter = sharp`) answers "can't", so every
/// texture is drawn with nearest pixels, like the game without filtering.
const F19: Fix = Fix {
    id: "F19",
    what: "sharp textures: nearest pixels everywhere",
    optional: true,
    code: &[Piece {
        at: At::Image(0x004a_9fe0),
        ops: &[Op::Raw(&[0x31, 0xc0, 0xc3])], // xor eax,eax ; ret
    }],
};

const F20_CAVE: u32 = 0xbe0;
const _: () = assert!(F20_CAVE >= F11_CULL_STUBS + 0x20 + 27);

/// Stage sections switch the far plane at once (`0x403eb0`, from 50 to 250
/// m), so a band of scenery pops in or out in one frame. Where the race
/// projection reads it (`mov edx,[0x4f683c]` at `site`, `0x435ffe` and
/// `0x436018`), F20 asks `cmr_ease_far` (`cmrhooks.dll`, `easing`) for a
/// value that follows it smoothly; it also sets the terrain cull radius.
/// Without the library the game's value is used. Opt-in
/// (`smooth_distance`).
macro_rules! f20_read {
    ($site:expr, $stub:expr) => {
        [
            Piece {
                at: At::Image($site),
                ops: &[Op::Jmp(At::Cave($stub)), Op::Raw(&[NOP])],
            },
            Piece {
                at: At::Cave($stub),
                ops: &[
                    Op::Raw(&[0x50, 0x51]),                         // push eax ; push ecx
                    Op::Raw(&[0x8b, 0x15, 0x3c, 0x68, 0x4f, 0x00]), // mov edx,[0x4f683c]
                    Op::Raw(&[0xa1]),                               // mov eax,[cmr_ease_far]
                    Op::Addr(At::Cave(F13_TABLE + slot::EASE_FAR)),
                    Op::Raw(&[0x85, 0xc0, 0x74, 0x05]), // test eax,eax ; jz +5
                    Op::Raw(&[0x52, 0xff, 0xd0, 0x8b, 0xd0]), // push edx ; call eax ; mov edx,eax
                    Op::Raw(&[0x59, 0x58]),             // pop ecx ; pop eax
                    Op::Jmp(At::Image($site + 6)),
                ],
            },
        ]
    };
}

const F20_READS: [[Piece; 2]; 2] = [
    f20_read!(0x0043_5ffe, F20_CAVE),
    f20_read!(0x0043_6018, F20_CAVE + 0x20),
];

const F20: Fix = Fix {
    id: "F20",
    what: "smooth view distance changes (needs cmrhooks.dll)",
    optional: true,
    code: &F20_PIECES,
};

const F20_PIECES: [Piece; 4] = {
    let [[a, b], [c, d]] = F20_READS;
    [a, b, c, d]
};

const F21_CAVE: u32 = 0xc20;
const F21_RET: u32 = 0xc70;
const F21_BLOCK: u32 = 0xc74;
const F21_PLAYER: u32 = 0xc78;
const F22_CAVE: u32 = 0xc80;
const _: () = assert!(
    F21_CAVE >= F20_CAVE + 0x20 + 29 && F21_RET >= F21_CAVE + 64 + 11 && F22_CAVE >= F21_PLAYER + 4
);

/// The in-race control map `0x449580(block, scheme, player)` writes each
/// car's controls; a pad's triggers reach it only through WinMM's combined
/// axis (F7), so pressing both cancels out. F21 wraps it: after it runs,
/// `cmr_pedals(block, player)` (`cmrhooks.dll`, `xinput`) raises throttle
/// (`+0x304`) and brake (`+0x308`) to the XInput pad's right and left
/// triggers. Keys and sticks still work. With `gamepad_axes`.
const F21: Fix = Fix {
    id: "F21",
    what: "gamepad triggers as separate pedals through XInput (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // push ebp ; mov ebp,esp ; sub esp,0xc -> jmp ; nop
        Piece {
            at: At::Image(0x0044_9580),
            ops: &[Op::Jmp(At::Cave(F21_CAVE)), Op::Raw(&[NOP])],
        },
        Piece {
            at: At::Cave(F21_CAVE),
            ops: &[
                Op::Raw(&[0x8f, 0x05]), // pop dword [ret]
                Op::Addr(At::Cave(F21_RET)),
                Op::Raw(&[0x60, 0x8b, 0x44, 0x24, 0x20, 0xa3]), // pushad ; mov eax,[esp+0x20] (block) ; mov [b],eax
                Op::Addr(At::Cave(F21_BLOCK)),
                Op::Raw(&[0x8b, 0x44, 0x24, 0x28, 0xa3]), // mov eax,[esp+0x28] (player) ; mov [p],eax
                Op::Addr(At::Cave(F21_PLAYER)),
                Op::Raw(&[0x61]), // popad
                Op::Call(At::Cave(F21_CAVE + 64)),
                Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_pedals]
                Op::Addr(At::Cave(F13_TABLE + slot::PEDALS)),
                Op::Raw(&[0x85, 0xc9, 0x74, 0x0e]), // test ecx,ecx ; jz +14
                Op::Raw(&[0xff, 0x35]),             // push [player]
                Op::Addr(At::Cave(F21_PLAYER)),
                Op::Raw(&[0xff, 0x35]), // push [block]
                Op::Addr(At::Cave(F21_BLOCK)),
                Op::Raw(&[0xff, 0xd1, 0x61, 0xff, 0x35]), // call ecx ; popad ; push [ret]
                Op::Addr(At::Cave(F21_RET)),
                Op::Raw(&[0xc3]), // ret
                // the moved instructions
                Op::Raw(&[0x55, 0x8b, 0xec, 0x83, 0xec, 0x0c]),
                Op::Jmp(At::Image(0x0044_9586)),
            ],
        },
        Piece {
            at: At::Cave(F21_RET),
            ops: &[Op::Raw(&[0; 12])],
        },
    ],
};

/// The impact sound `0x469e80(severity, position, kind)` plays player 1's
/// collisions; F22 hands each one's severity (Q16) to `cmr_rumble`, which
/// rumbles the pad and lets it fade (each frame, in `cmr_present`, F17).
/// Opt-in (`rumble`).
const F22: Fix = Fix {
    id: "F22",
    what: "gamepad rumble on impacts (needs cmrhooks.dll)",
    optional: true,
    code: &[
        // push ebp ; mov ebp,esp ; cmp dword [ebp+8],0x10000 -> jmp ; 5 x nop
        Piece {
            at: At::Image(0x0046_9e80),
            ops: &[Op::Jmp(At::Cave(F22_CAVE)), Op::Raw(&[NOP; 5])],
        },
        Piece {
            at: At::Cave(F22_CAVE),
            ops: &[
                Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[cmr_rumble]
                Op::Addr(At::Cave(F13_TABLE + slot::RUMBLE)),
                Op::Raw(&[0x85, 0xc9, 0x74, 0x0b]), // test ecx,ecx ; jz +11
                Op::Raw(&[0x68]),                   // push settings
                Op::Addr(At::Cave(F17_SETTINGS)),
                Op::Raw(&[0xff, 0x74, 0x24, 0x28]), // push dword [esp+0x28] (severity)
                Op::Raw(&[0xff, 0xd1, 0x61]),       // call ecx ; popad
                // the moved instructions
                Op::Raw(&[0x55, 0x8b, 0xec, 0x81, 0x7d, 0x08, 0x00, 0x00, 0x01, 0x00]),
                Op::Jmp(At::Image(0x0046_9e8a)),
            ],
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
    fn fixes_are_in_order() {
        let ids: Vec<_> = OFFICIAL_EU_15.fixes.iter().map(|f| f.id).collect();
        let expected: Vec<_> = (1..=ids.len()).map(|i| format!("F{i}")).collect();
        assert_eq!(ids, expected);
        assert_eq!(ids.len(), 27);
    }

    #[test]
    fn f23_hands_the_buttons_to_the_hook_and_resumes_at_the_first_test() {
        let site = at(0x49cef6);
        assert_eq!(site[0], 0xe9);
        let rel = u32::from_le_bytes(site[1..5].try_into().unwrap());
        assert_eq!(0x49cefbu32.wrapping_add(rel), CAVE + F23_CAVE);
        assert_eq!(site[5], NOP);
        let cave = at(CAVE + F23_CAVE);
        assert_eq!(&cave[..6], [0x8b, 0x44, 0x24, 0x28, 0x8b, 0x0d]);
        assert_eq!(
            cave[6..10],
            (CAVE + F13_TABLE + slot::PAD_BUTTONS).to_le_bytes()
        );
        // jz skips the 13 bytes of the call to the moved test.
        assert_eq!(&cave[10..14], [0x85, 0xc9, 0x74, 0x0d]);
        assert_eq!(cave[15..19], (CAVE + F23_MAP).to_le_bytes());
        assert_eq!(&cave[27..29], [0xa8, 0x01]);
        let rel = u32::from_le_bytes(cave[30..34].try_into().unwrap());
        assert_eq!((CAVE + F23_CAVE + 34).wrapping_add(rel), 0x49cefc);
        assert_eq!(at(CAVE + F23_MAP), [0; 12]);
    }

    #[test]
    fn f27_loads_through_the_hook_or_the_trampoline() {
        let jump_at = |code: &[u8], at: u32, off: usize| {
            assert_eq!(code[off], 0xe9);
            let rel = u32::from_le_bytes(code[off + 1..off + 5].try_into().unwrap());
            (at + off as u32 + 5).wrapping_add(rel)
        };
        let site = at(0x499ec0);
        assert_eq!(jump_at(&site, 0x499ec0, 0), CAVE + F27_CAVE);
        let c = at(CAVE + F27_CAVE);
        assert_eq!(c.len(), 39);
        assert_eq!(c[2..6], (CAVE + F13_TABLE + slot::HD_LOAD).to_le_bytes());
        // jz lands on the trampoline (28): 10 + 0x12
        assert_eq!(&c[6..10], [0x85, 0xc9, 0x74, 0x12]);
        assert_eq!(c[19..23], (CAVE + F27_TRAMPOLINE).to_le_bytes());
        assert_eq!(&c[23..28], [0xff, 0xd1, 0xc2, 0x08, 0x00]);
        assert_eq!(&c[28..34], [0x8b, 0x44, 0x24, 0x08, 0xa8, 0x01]);
        assert_eq!(jump_at(&c, CAVE + F27_CAVE, 34), 0x499ec6);

        let site = at(0x499320);
        assert_eq!(site.len(), 6);
        assert_eq!(jump_at(&site, 0x499320, 0), CAVE + F27_CAVE_2D);
        let c = at(CAVE + F27_CAVE_2D);
        assert_eq!(c.len(), 43);
        assert_eq!(c[2..6], (CAVE + F13_TABLE + slot::HD_LOAD_2D).to_le_bytes());
        // jz lands on the trampoline (32): 10 + 0x16
        assert_eq!(&c[6..10], [0x85, 0xc9, 0x74, 0x16]);
        assert_eq!(c[23..27], (CAVE + F27_TRAMPOLINE_2D).to_le_bytes());
        assert_eq!(&c[27..32], [0xff, 0xd1, 0xc2, 0x0c, 0x00]);
        assert_eq!(&c[32..38], [0x81, 0xec, 0xb0, 0x03, 0x00, 0x00]);
        assert_eq!(jump_at(&c, CAVE + F27_CAVE_2D, 38), 0x499326);
    }

    #[test]
    fn f25_asks_for_24_bits() {
        assert_eq!(at(0x4a6e9b), [0x18]);
    }

    #[test]
    fn f26_hands_the_depth_test_to_the_hook_or_runs_the_games() {
        let site = at(0x49fa70);
        let rel = u32::from_le_bytes(site[1..5].try_into().unwrap());
        assert_eq!(0x49fa75u32.wrapping_add(rel), CAVE + F26_CAVE);
        assert_eq!(&site[5..], [NOP; 3]);
        let c = at(CAVE + F26_CAVE);
        assert_eq!(c.len(), 25);
        assert_eq!(
            c[2..6],
            (CAVE + F13_TABLE + slot::DEPTH_VISIBLE).to_le_bytes()
        );
        // jz +2 skips exactly the jmp ecx
        assert_eq!(&c[6..12], [0x85, 0xc9, 0x74, 0x02, 0xff, 0xe1]);
        assert_eq!(&c[12..20], [0xa1, 0x3c, 0x28, 0x50, 0x00, 0x83, 0xec, 0x6c]);
        let rel = u32::from_le_bytes(c[21..25].try_into().unwrap());
        assert_eq!((CAVE + F26_CAVE + 25).wrapping_add(rel), 0x49fa78);
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
        assert_eq!(at(0x49ce1b), [0xcf]);
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
            65536.0 * 16.0 / 9.0
        );
        assert_eq!(
            f64::from_le_bytes(at(CAVE + F11_K3).try_into().unwrap()),
            0x2bae1 as f64 * 0.75
        );
        let back = |code: &[u8], base: u32| {
            let n = code.len();
            assert_eq!(code[n - 5], 0xe9);
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            (base + n as u32).wrapping_add(rel)
        };
        let proj = at(CAVE + F11_PROJ);
        assert_eq!(proj.len(), 16);
        assert_eq!(back(&proj, CAVE + F11_PROJ), 0x435d4f);
        let ys = at(CAVE + F11_YS);
        assert_eq!(ys.len(), 32);
        assert_eq!(back(&ys, CAVE + F11_YS), 0x435d8c);
        // At 4:3 the factors are the original ones.
        let a43: f64 = 4.0 / 3.0;
        assert!(((3.0 / 4.0) * 65536.0 * 16.0 / 9.0 - a43 * 65536.0).abs() < 1e-6);
        assert!((a43 * 0x2bae1 as f64 * 0.75 - 0x2bae1 as f64).abs() < 1e-6);
    }

    #[test]
    fn f12_scales_the_three_ars_values_and_the_trigger_read() {
        let ars = at(CAVE + F12_ARS);
        assert_eq!(ars.len(), 65);
        for (i, g) in [0x4f6838u32, 0x4f683c, 0x4f6840].iter().enumerate() {
            let b = &ars[i * 20..i * 20 + 20];
            assert_eq!(b[0], 0xa1);
            assert_eq!(b[1..5], g.to_le_bytes());
            assert_eq!(b[7..11], (CAVE + F12_FACTOR).to_le_bytes());
            assert_eq!(b[15], 0xa3);
            assert_eq!(b[16..20], g.to_le_bytes());
        }
        let rel = u32::from_le_bytes(ars[61..65].try_into().unwrap());
        assert_eq!((CAVE + F12_ARS + 65).wrapping_add(rel), 0x4809cb);
        let trig = at(CAVE + F12_TRIGGER);
        assert_eq!(trig.len(), 22);
        let rel = u32::from_le_bytes(trig[18..22].try_into().unwrap());
        assert_eq!((CAVE + F12_TRIGGER + 22).wrapping_add(rel), 0x403eae);
        for cap in F12_CAPS {
            assert_eq!(
                at(cap),
                F12_CAP.to_le_bytes(),
                "placeholder is the original"
            );
        }
    }

    #[test]
    fn f16_jumps_back_and_keeps_the_flip() {
        let back = |code: &[u8], base: u32| {
            let n = code.len();
            assert_eq!(code[n - 5], 0xe9);
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            (base + n as u32).wrapping_add(rel)
        };
        let mode = at(CAVE + F16_MODE);
        assert_eq!(mode.len(), 33);
        // je +11 skips exactly the two size loads.
        assert_eq!(&mode[7..9], [0x74, 11]);
        assert_eq!(mode[9..11], [0x8b, 0x0d]);
        assert_eq!(mode[15], 0xa1);
        assert_eq!(back(&mode, CAVE + F16_MODE), 0x497dc1);
        let present = at(CAVE + F17_PRESENT);
        assert_eq!(present.len(), 39);
        assert_eq!(
            present[1..5],
            (CAVE + F13_TABLE + slot::PRESENT).to_le_bytes()
        );
        // jz +12 lands on the original Flip, jnz +13 jumps over it.
        assert_eq!(&present[5..7], [0x85, 0xc0]);
        assert_eq!(&present[7..9], [0x74, 12]);
        assert_eq!(present[9], 0x68, "push the settings block");
        assert_eq!(present[10..14], (CAVE + F17_SETTINGS).to_le_bytes());
        assert_eq!(&present[14..17], [0x56, 0xff, 0xd0]);
        assert_eq!(&present[19..21], [0x75, 13]);
        assert_eq!(&present[21..24], [0x8b, 0x46, 0x10]);
        assert_eq!(back(&present, CAVE + F17_PRESENT), 0x498712);
        assert_eq!(at(0x498705).len(), 13);
        assert_eq!(at(0x497db9).len(), 8);
        let legal = at(CAVE + F16_LEGAL);
        assert_eq!(legal.len(), 37);
        // jz +8 skips exactly the display push and the call.
        assert_eq!(&legal[19..21], [0x74, 8]);
        assert_eq!(&legal[21..23], [0xff, 0x35]);
        assert_eq!(&legal[27..29], [0xff, 0xd0]);
    }

    #[test]
    fn f13_entry_hooks_relocate_and_return() {
        let rel = |code: &[u8], end: usize, base: u32| {
            let r = u32::from_le_bytes(code[end - 4..end].try_into().unwrap());
            (base + end as u32).wrapping_add(r)
        };
        // Panel: the replaced call is redone, then back after it.
        let panel = at(CAVE + F13_RECTS + 0x90);
        assert_eq!(panel.len(), 34);
        assert_eq!(panel[3..7], (CAVE + F13_TABLE + 16).to_le_bytes());
        assert_eq!(&panel[11..13], [0x6a, 1], "rectangle = argument 2");
        assert_eq!(&panel[21..24], [0x83, 0xec, 0x18]);
        assert_eq!(panel[24], 0xe8);
        assert_eq!(rel(&panel, 29, CAVE + F13_RECTS + 0x90), 0x498af0);
        assert_eq!(rel(&panel, 34, CAVE + F13_RECTS + 0x90), 0x49e208);
        assert_eq!(at(0x49e200).len(), 8, "jmp + 3 nops covers the call");
        // Direct blit: destination rectangle = argument 4.
        let blit = at(CAVE + F13_RECTS + 0x60);
        assert_eq!(&blit[11..13], [0x6a, 3]);
        assert_eq!(rel(&blit, blit.len(), CAVE + F13_RECTS + 0x60), 0x49e3e6);
        assert_eq!(at(0x49e3e0).len(), 6);
    }

    #[test]
    fn f13_function_wraps_run_the_moved_instructions() {
        let rel = |code: &[u8], end: usize, base: u32| {
            let r = u32::from_le_bytes(code[end - 4..end].try_into().unwrap());
            (base + end as u32).wrapping_add(r)
        };
        // The update: an entry hook, nothing waits.
        let base = CAVE + F13_FUNCTIONS;
        let update = at(base);
        assert_eq!(
            update[3..7],
            (CAVE + F13_TABLE + slot::UPDATE).to_le_bytes()
        );
        assert_eq!(
            &update[14..19],
            [0xa1, 0xcc, 0xef, 0x96, 0x00],
            "moved prologue"
        );
        assert_eq!(rel(&update, update.len(), base), 0x443db5);
        assert_eq!(at(0x443db0).len(), 5);
        // D3D set-up: a real-size section; the moved call keeps its target.
        let base = CAVE + F13_FUNCTIONS + 0x180;
        let d3d = at(base);
        assert_eq!(d3d[9..13], (CAVE + F13_TABLE + slot::REAL).to_le_bytes());
        assert_eq!(
            d3d[28..32],
            (CAVE + F13_TABLE + slot::REAL_END).to_le_bytes()
        );
        assert_eq!(&d3d[46..48], [0x6a, 0x7c]);
        assert_eq!(rel(&d3d, 53, base), 0x4990d0);
        assert_eq!(rel(&d3d, 58, base), 0x4a6957);
        // Scene draw: 3D work, the view block handed to begin and end.
        let base = CAVE + F13_VIEWS + 0xf0;
        let draw = at(base);
        assert_eq!(
            draw[9..13],
            (CAVE + F13_TABLE + slot::THREE_D).to_le_bytes()
        );
        assert_eq!(
            &draw[13..18],
            [0x85, 0xc9, 0x74, 7, 0x68],
            "jz skips push and call"
        );
        assert_eq!(draw[18..22], (CAVE + F13_VIEW).to_le_bytes());
        assert_eq!(
            rel(&draw, 30, base),
            base + 56,
            "calls the moved instructions"
        );
        assert_eq!(
            draw[33..37],
            (CAVE + F13_TABLE + slot::THREE_D_END).to_le_bytes()
        );
        assert_eq!(&draw[56..61], [0xa1, 0x6c, 0x27, 0x50, 0x00]);
        assert_eq!(rel(&draw, draw.len(), base), 0x49fe35);
    }

    #[test]
    fn f13_screen_size_reads_see_the_3d_view() {
        let read = at(CAVE + F13_READ_EAX);
        assert_eq!(read.len(), 35);
        assert_eq!(&read[7..9], [0x74, 0x14], "depth 0: to the game's read");
        assert_eq!(
            &read[9..15],
            [0x64, 0xa1, 0x24, 0x00, 0x00, 0x00],
            "thread id"
        );
        assert_eq!(
            &read[21..23],
            [0x75, 6],
            "another thread: to the game's read"
        );
        assert_eq!(read[24..28], (CAVE + F13_VIEW + 8).to_le_bytes());
        assert_eq!(&read[29..35], [0xa1, 0x18, 0x25, 0x50, 0x00, 0xc3]);
        assert_eq!(at(CAVE + F11_READ), read, "F11's own copy");
        for (site, n, stub) in [
            (0x435d3f, 5, F13_READ_EAX),
            (0x49ffd4, 5, F13_READ_EAX),
            (0x48f896, 5, F13_READ_EAX),
            (0x4a2ab0, 6, F13_READ_ECX),
            (0x4b069f, 6, F13_READ_EDX),
        ] {
            let call = at(site);
            assert_eq!(call.len(), n);
            assert_eq!(call[0], 0xe8);
            let r = u32::from_le_bytes(call[1..5].try_into().unwrap());
            assert_eq!((site + 5).wrapping_add(r), CAVE + stub);
        }
    }

    #[test]
    fn f18_hooks_texture_creation_uploads_and_filtering() {
        let back = |code: &[u8], base: u32| {
            let n = code.len();
            assert_eq!(code[n - 5], 0xe9);
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            (base + n as u32).wrapping_add(rel)
        };
        let desc = at(CAVE + F18_CAVE);
        assert_eq!(
            desc[3..7],
            (CAVE + F13_TABLE + slot::TEXTURE_DESC).to_le_bytes()
        );
        assert_eq!(
            &desc[7..11],
            [0x85, 0xc9, 0x74, 7],
            "jz skips lea, push and call"
        );
        assert_eq!(back(&desc, CAVE + F18_CAVE), 0x49eb6b);
        // Each upload stub: the replaced bytes end with the Load call, then
        // push dst + call is exactly the 5 bytes jz skips.
        for (off, orig, resume) in [
            (0x30, 12, 0x49a14e),
            (0x60, 12, 0x49a454),
            (0x90, 11, 0x49ad05),
        ] {
            let stub = at(CAVE + F18_CAVE + off);
            assert_eq!(
                &stub[orig - 3..orig],
                &[0xff, stub[orig - 2], 0x14],
                "Load at {off:#x}"
            );
            assert_eq!(&stub[orig + 7..orig + 11], [0x85, 0xc9, 0x74, 5]);
            assert_eq!(&stub[orig + 14..orig + 16], [0xff, 0xd1]);
            assert_eq!(back(&stub, CAVE + F18_CAVE + off), resume);
        }
        let tri = at(CAVE + F18_CAVE + 0xc0);
        assert_eq!(
            &tri[..9],
            [0x83, 0xfe, 0x02, 0x75, 0x04, 0x6a, 0x06, 0xeb, 0x01]
        );
        assert_eq!(back(&tri, CAVE + F18_CAVE + 0xc0), 0x4a2444);
        assert_eq!(at(0x4a243e).len(), 6);
        assert_eq!(at(0x4a9fe0), [0x31, 0xc0, 0xc3], "F19: no filtering");
    }

    #[test]
    fn cull_factor_follows_the_aspect() {
        assert_eq!(cull_t(1440, 1080), 0x10000);
        assert_eq!(cull_t(640, 480), 0x10000);
        assert_eq!(cull_t(1920, 1080), 0x15555);
        assert_eq!(cull_t(3440, 1440), 0x1caaa);
        assert_eq!(cull_t(1280, 1024), 0x10000, "narrower than 4:3: unchanged");
    }

    #[test]
    fn f11_widens_the_culling_wedges() {
        for imm in F11_CULL_IMMS {
            assert_eq!(
                at(imm),
                0x10000u32.to_le_bytes(),
                "1.0 until the launcher fills t"
            );
        }
        for (site, stub) in [
            (0x4a0a21, F11_CULL_STUBS),
            (0x4a0a32, F11_CULL_STUBS + 0x20),
        ] {
            assert_eq!(at(site).len(), 6);
            let code = at(CAVE + stub);
            assert_eq!(
                &code[..6],
                [0x8b, 0x15, 0xec, 0x22, 0x50, 0x00],
                "the replaced load"
            );
            assert_eq!(code[11..15], (CAVE + F11_CULL_T).to_le_bytes());
            let n = code.len();
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            assert_eq!((CAVE + stub + n as u32).wrapping_add(rel), site + 6);
        }
    }

    #[test]
    fn f20_eases_both_far_plane_reads() {
        for (site, stub) in [(0x435ffe, F20_CAVE), (0x436018, F20_CAVE + 0x20)] {
            assert_eq!(at(site).len(), 6);
            let code = at(CAVE + stub);
            assert_eq!(
                &code[2..8],
                [0x8b, 0x15, 0x3c, 0x68, 0x4f, 0x00],
                "the game's value first"
            );
            assert_eq!(
                code[9..13],
                (CAVE + F13_TABLE + slot::EASE_FAR).to_le_bytes()
            );
            assert_eq!(
                &code[13..17],
                [0x85, 0xc0, 0x74, 5],
                "jz skips push, call, mov"
            );
            let n = code.len();
            let rel = u32::from_le_bytes(code[n - 4..].try_into().unwrap());
            assert_eq!((CAVE + stub + n as u32).wrapping_add(rel), site + 6);
        }
    }

    #[test]
    fn f21_and_f22_pass_their_arguments() {
        let ctl = at(CAVE + F21_CAVE);
        assert_eq!(ctl.len(), 64 + 11);
        assert_eq!(
            &ctl[6..11],
            [0x60, 0x8b, 0x44, 0x24, 0x20],
            "block = argument 1"
        );
        assert_eq!(
            &ctl[16..20],
            [0x8b, 0x44, 0x24, 0x28],
            "player = argument 3"
        );
        assert_eq!(&ctl[38..42], [0x85, 0xc9, 0x74, 14]);
        assert_eq!(
            &ctl[64..70],
            [0x55, 0x8b, 0xec, 0x83, 0xec, 0x0c],
            "moved prologue"
        );
        let rel = u32::from_le_bytes(ctl[71..75].try_into().unwrap());
        assert_eq!((CAVE + F21_CAVE + 75).wrapping_add(rel), 0x449586);
        let call = u32::from_le_bytes(ctl[27..31].try_into().unwrap());
        assert_eq!(
            (CAVE + F21_CAVE + 31).wrapping_add(call),
            CAVE + F21_CAVE + 64
        );
        let imp = at(CAVE + F22_CAVE);
        assert_eq!(&imp[7..11], [0x85, 0xc9, 0x74, 11]);
        assert_eq!(imp[12..16], (CAVE + F17_SETTINGS).to_le_bytes());
        assert_eq!(
            &imp[16..20],
            [0xff, 0x74, 0x24, 0x28],
            "severity = argument 1"
        );
        let rel = u32::from_le_bytes(imp[34..38].try_into().unwrap());
        assert_eq!((CAVE + F22_CAVE + 38).wrapping_add(rel), 0x469e8a);
    }

    #[test]
    fn simple_stubs() {
        assert_eq!(at(0x496ca0), [0xb8, 1, 0, 0, 0, 0xc3]);
        assert_eq!(at(0x497cef), [0x10]);
        assert_eq!(at(0x49dcb0), [0x31, 0xc0, 0xc3]);
        assert_eq!(at(0x4ae590), [0xc2, 0x04, 0x00]);
        assert_eq!(at(0x4ad240), [0xb8, 0x00, 0xca, 0x9a, 0x3b, 0xc3]);
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
        // the last piece is F27's loader stub
        assert_eq!(OFFICIAL_EU_15.cave_size(&[]), F27_CAVE_2D + 43);
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
        let skip = [
            "F6", "F7", "F10", "F11", "F12", "F13", "F14", "F16", "F17", "F18", "F19", "F20",
            "F21", "F22", "F23", "F24", "F25", "F26", "F27", "F1",
        ];
        let ids: Vec<_> = OFFICIAL_EU_15.active(&skip).map(|f| f.id).collect();
        assert_eq!(ids, ["F1", "F2", "F3", "F4", "F5", "F8", "F9", "F15"]);
        assert_eq!(OFFICIAL_EU_15.cave_size(&skip), F9_CAVE + 24);
        assert!(
            OFFICIAL_EU_15
                .assemble(CAVE, &skip)
                .iter()
                .all(|w| w.fix != "F6")
        );
        assert_eq!(
            COMMUNITY_EU_15.cave_size(&[
                "F6", "F7", "F10", "F11", "F12", "F13", "F14", "F16", "F17", "F18", "F19"
            ]),
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
