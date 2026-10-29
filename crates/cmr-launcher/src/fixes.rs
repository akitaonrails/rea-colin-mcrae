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
    fixes: &[F6, F7, F8, F9, F10, F11, F12, F13, F14, F15, F16],
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

/// The race projection (`FUN_00435d10`) feeds the game's own software
/// transform, which maps to pixels as `x * xs * W/2` and `y * ys * H/(2A)`
/// (A = W/H), with `xs` scaled by A. Right only at 4:3: at 16:9 the picture
/// comes out 1.78x wider and 0.75x shorter than at 4:3 (stretched). F11
/// makes it Hor+, so the pixel scale is the one of a 4:3 screen of the same
/// height and the wider screen shows more to the sides: the aspect term of
/// `xs` becomes (4/3)^2 / A (`0x435d44`) and `ys` is multiplied by A / (4/3)
/// (`0x435d87`). At 4:3 both are exactly the original values. Opt-in
/// (`widescreen`).
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
                Op::Raw(&[0xa1, 0x18, 0x25, 0x50, 0x00]), // mov eax,[0x502518]
                Op::Raw(&[0xdb, 0x00, 0xda, 0x70, 0x04]), // fild [W] ; fidiv [H]
                Op::Raw(&[0xdc, 0x0d]),                   // fmul qword [0x2bae1 * 3/4]
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
/// The exports F13's table holds, in order.
pub const F13_EXPORTS: [&str; 6] = [
    "cmr_hud_begin",
    "cmr_hud_end",
    "cmr_hud_rects",
    "cmr_fe_present",
    "cmr_hud_rect_at",
    "cmr_hud_begin_whole",
];
const _: () = assert!(F13_TABLE >= F12_TRIGGER + 22 && F13_TABLE + 24 <= 0x280);
/// Table slots of the two begins: the race HUD (side pieces anchored to the
/// screen edges) and screens laid out as a whole (mapped centred).
const HUD: u32 = 0;
const WHOLE: u32 = 20;

/// With a race resolution the game never had (F10), the in-race 2D (HUD,
/// messages, pause, results, loading screen) mixes 640x480 rectangles scaled
/// by W/640, fractions of W/H and absolute pixel tables chosen by width, and
/// text is drawn at the font's native size. F13 shows these calls a virtual
/// 640x480 screen while they run, and maps every rectangle they draw (sprite
/// queues `0x49b420`/`0x49b1d0`, direct blit `0x49e3e0`, panel `0x49e200`)
/// onto a 4:3 canvas centred on the real screen (`cmrhooks.dll`, `canvas`);
/// full-screen backdrops and fades span the whole screen. The race HUD's side
/// pieces are anchored to the screen edges. Applied with F10; the library
/// leaves the game's own resolutions alone.
///
/// F13's pieces: every wrapped call (`call target` at `site` goes through a
/// stub that calls a begin (`HUD` or `WHOLE`), the original, then `cmr_hud_end`; the
/// return address is kept at cave `ret`), the entry hooks on the 2D
/// primitives (their first instructions `orig` move to a stub that lets the
/// library rescale the rectangle arguments, `arg` being the clip index for
/// the sprite queues and the rectangle index for `cmr_hud_rect_at`), and the
/// hook table.
macro_rules! hud_code {
    (
        wraps: [$(($site:expr, $target:expr, $begin:expr, $stub:expr, $ret:expr)),* $(,)?],
        entries: [$(($esite:expr, $nops:expr, $estub:expr, $slot:expr, $arg:expr, [$($orig:expr),*], $resume:expr)),* $(,)?] $(,)?
    ) => {
        &[
            $(
                Piece {
                    at: At::Image($site),
                    ops: &[Op::Call(At::Cave($stub))],
                },
                Piece {
                    at: At::Cave($stub),
                    ops: &[
                        Op::Raw(&[0x8f, 0x05]), // pop dword [ret]
                        Op::Addr(At::Cave($ret)),
                        Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[begin]
                        Op::Addr(At::Cave(F13_TABLE + $begin)),
                        Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xd1, 0x61]), // test ; jz ; call ecx ; popad
                        Op::Call(At::Image($target)),
                        Op::Raw(&[0x60, 0x8b, 0x0d]), // pushad ; mov ecx,[end]
                        Op::Addr(At::Cave(F13_TABLE + 4)),
                        Op::Raw(&[0x85, 0xc9, 0x74, 0x02, 0xff, 0xd1, 0x61]), // test ; jz ; call ecx ; popad
                        Op::Raw(&[0xff, 0x35]),                               // push dword [ret] ; ret
                        Op::Addr(At::Cave($ret)),
                        Op::Raw(&[0xc3]),
                    ],
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
                ops: &[Op::Raw(&[0; 24])],
            },
        ]
    };
}

/// Cave area of F13's wrap stubs (0x30 apart) and their return slots.
const F13_STUBS: u32 = 0x600;
const F13_RETS: u32 = 0x980;
/// Cave area of the entry-hook stubs (0x30 apart).
const F13_ENTRIES: u32 = 0x9d0;
const _: () = assert!(
    F13_STUBS >= F16_RET + 4
        && F13_STUBS + 18 * 0x30 <= F13_RETS
        && F13_RETS + 18 * 4 <= F13_ENTRIES
);

const F13: Fix = Fix {
    id: "F13",
    what: "HUD, messages and loading screen scaled to the race resolution (needs cmrhooks.dll)",
    optional: true,
    code: hud_code! {
        wraps: [
            (0x0042_22ab, 0x0046_2230, HUD, F13_STUBS, F13_RETS),              // HUD layout (race start)
            (0x0042_22e7, 0x0047_fda0, HUD, F13_STUBS + 0x30, F13_RETS + 4),   // timer font
            (0x0042_22ec, 0x0043_6a50, WHOLE, F13_STUBS + 0x60, F13_RETS + 8),   // pause fonts
            (0x0042_2e1c, 0x0046_4930, HUD, F13_STUBS + 0x90, F13_RETS + 12),  // HUD, each view
            (0x0047_65b2, 0x0046_4930, HUD, F13_STUBS + 0xc0, F13_RETS + 16),  // HUD, replay
            (0x0042_2ea6, 0x0047_fe50, HUD, F13_STUBS + 0xf0, F13_RETS + 20),  // stage time
            (0x0047_65c8, 0x0047_fe50, HUD, F13_STUBS + 0x120, F13_RETS + 24), // stage time, replay
            (0x0042_2f83, 0x0042_3570, WHOLE, F13_STUBS + 0x150, F13_RETS + 28), // countdown
            (0x0042_2f95, 0x0042_3700, WHOLE, F13_STUBS + 0x180, F13_RETS + 32), // GO
            (0x0042_3405, 0x0046_8dd0, WHOLE, F13_STUBS + 0x1b0, F13_RETS + 36), // pause menu
            (0x0042_32db, 0x0042_ae80, WHOLE, F13_STUBS + 0x1e0, F13_RETS + 40), // results pages
            (0x0042_2f71, 0x0042_34b0, WHOLE, F13_STUBS + 0x210, F13_RETS + 44), // STAGE COMPLETE
            (0x0042_2e33, 0x0046_e4f0, WHOLE, F13_STUBS + 0x240, F13_RETS + 48), // rally school text
            (0x0042_2f13, 0x0047_a9b0, WHOLE, F13_STUBS + 0x270, F13_RETS + 52), // replay overlay
            (0x0042_2dd1, 0x0042_a930, WHOLE, F13_STUBS + 0x2a0, F13_RETS + 56), // in-race text
            (0x0042_2ddb, 0x0046_92a0, WHOLE, F13_STUBS + 0x2d0, F13_RETS + 60), // in-race panel
            (0x0044_e678, 0x0044_e6d0, WHOLE, F13_STUBS + 0x300, F13_RETS + 64), // loading screen
            (0x0044_e694, 0x0044_e6d0, WHOLE, F13_STUBS + 0x330, F13_RETS + 68), // loading screen
        ],
        entries: [
            // sprite queues: mov al,[esp+..] ; push esi
            (0x0049_b420, 0, F13_ENTRIES, 8, 8, [Op::Raw(&[0x8a, 0x44, 0x24, 0x10, 0x56])], 0x0049_b425),
            (0x0049_b1d0, 0, F13_ENTRIES + 0x30, 8, 9, [Op::Raw(&[0x8a, 0x44, 0x24, 0x14, 0x56])], 0x0049_b1d5),
            // direct blit (destination rectangle = argument 4): sub esp,0x12c
            (0x0049_e3e0, 1, F13_ENTRIES + 0x60, 16, 3,
                [Op::Raw(&[0x81, 0xec, 0x2c, 0x01, 0x00, 0x00])], 0x0049_e3e6),
            // panel (rectangle = argument 2): sub esp,0x18 ; call 0x498af0
            (0x0049_e200, 3, F13_ENTRIES + 0x90, 16, 1,
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
        // mov eax,[esi+0x10] ; push 1 ; push 0 ; mov ecx,[eax] ; push eax ;
        // call [ecx+0x2c] (Flip) -> jmp ; 8 x nop
        Piece {
            at: At::Image(0x0049_8705),
            ops: &[Op::Jmp(At::Cave(F16_PRESENT)), Op::Raw(&[NOP; 8])],
        },
        Piece {
            at: At::Cave(F16_PRESENT),
            ops: &[
                Op::Raw(&[0xa1]), // mov eax,[fe_present]
                Op::Addr(At::Cave(F13_TABLE + 12)),
                Op::Raw(&[0x85, 0xc0, 0x74, 0x07]), // test eax,eax ; jz +7
                Op::Raw(&[0x56, 0xff, 0xd0]),       // push esi (display) ; call eax
                Op::Raw(&[0x85, 0xc0, 0x75, 0x0d]), // test eax,eax ; jnz +13 (presented)
                // the replaced Flip
                Op::Raw(&[
                    0x8b, 0x46, 0x10, 0x6a, 0x01, 0x6a, 0x00, 0x8b, 0x08, 0x50, 0xff, 0x51, 0x2c,
                ]),
                Op::Jmp(At::Image(0x0049_8712)),
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
        assert_eq!(ids.len(), 16);
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
        let present = at(CAVE + F16_PRESENT);
        assert_eq!(present.len(), 34);
        assert_eq!(present[1..5], (CAVE + F13_TABLE + 12).to_le_bytes());
        // jz +7 lands on the original Flip, jnz +13 jumps over it.
        assert_eq!(&present[7..9], [0x74, 7]);
        assert_eq!(&present[14..16], [0x75, 13]);
        assert_eq!(&present[16..19], [0x8b, 0x46, 0x10]);
        assert_eq!(back(&present, CAVE + F16_PRESENT), 0x498712);
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
        let panel = at(CAVE + F13_ENTRIES + 0x90);
        assert_eq!(panel.len(), 34);
        assert_eq!(panel[3..7], (CAVE + F13_TABLE + 16).to_le_bytes());
        assert_eq!(&panel[11..13], [0x6a, 1], "rectangle = argument 2");
        assert_eq!(&panel[21..24], [0x83, 0xec, 0x18]);
        assert_eq!(panel[24], 0xe8);
        assert_eq!(rel(&panel, 29, CAVE + F13_ENTRIES + 0x90), 0x498af0);
        assert_eq!(rel(&panel, 34, CAVE + F13_ENTRIES + 0x90), 0x49e208);
        assert_eq!(at(0x49e200).len(), 8, "jmp + 3 nops covers the call");
        // Direct blit: destination rectangle = argument 4.
        let blit = at(CAVE + F13_ENTRIES + 0x60);
        assert_eq!(&blit[11..13], [0x6a, 3]);
        assert_eq!(rel(&blit, blit.len(), CAVE + F13_ENTRIES + 0x60), 0x49e3e6);
        assert_eq!(at(0x49e3e0).len(), 6);
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
        // the last piece is the panel entry stub (34 bytes)
        assert_eq!(OFFICIAL_EU_15.cave_size(&[]), F13_ENTRIES + 0x90 + 34);
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
        let skip = ["F6", "F7", "F10", "F11", "F12", "F13", "F14", "F16", "F1"];
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
            COMMUNITY_EU_15.cave_size(&["F6", "F7", "F10", "F11", "F12", "F13", "F14", "F16"]),
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
