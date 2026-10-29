//! The functions the launcher's stubs call (stdcall, 32-bit). They run on
//! the game's main thread, from its HUD code only.

use core::ptr::addr_of_mut;

use crate::canvas::{Canvas, Rect, VIRTUAL, is_native};

/// `cmr_hud_rects`' clip index for calls without a clip rectangle (the
/// stubs push 0x7f; any index from 16 up means none).
const NO_CLIP: u32 = 16;

/// The game's display state; `+0` width, `+4` height.
const DISPLAY: *const *mut i32 = 0x0050_2518 as *const *mut i32;

struct Hud {
    depth: u32,
    real: (i32, i32),
    canvas: Option<Canvas>,
    dst: Rect,
    clip: Rect,
}

static mut HUD: Hud = Hud {
    depth: 0,
    real: (0, 0),
    canvas: None,
    dst: Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
    clip: Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
};

fn hud() -> &'static mut Hud {
    // SAFETY: only the game's main thread calls these exports, never
    // re-entrantly beyond the depth counter.
    unsafe { &mut *addr_of_mut!(HUD) }
}

fn display() -> *mut i32 {
    // SAFETY: a fixed global of the one executable these fixes are keyed to.
    unsafe { *DISPLAY }
}

/// Before a HUD layout or draw call: show the game a 640x480 screen.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_begin() {
    let hud = hud();
    hud.depth += 1;
    if hud.depth > 1 {
        return;
    }
    let d = display();
    if d.is_null() {
        hud.canvas = None;
        return;
    }
    // SAFETY: the display struct starts with width and height.
    let real = unsafe { (*d, *d.add(1)) };
    if real.0 <= 0 || real.1 <= 0 || is_native(real.0, real.1) {
        hud.canvas = None;
        return;
    }
    hud.real = real;
    hud.canvas = Some(Canvas::new(real.0, real.1));
    unsafe {
        *d = VIRTUAL.0;
        *d.add(1) = VIRTUAL.1;
    }
}

/// After the call: the real screen again.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_end() {
    let hud = hud();
    if hud.depth == 0 {
        return;
    }
    hud.depth -= 1;
    if hud.depth > 0 || hud.canvas.is_none() {
        return;
    }
    let d = display();
    if !d.is_null() {
        unsafe {
            *d = hud.real.0;
            *d.add(1) = hud.real.1;
        }
    }
    hud.canvas = None;
}

/// At the entry of a sprite-queue call (`0x49b420`/`0x49b1d0`) or a panel
/// (`0x49e200`): `args` points at its arguments; argument 1 is the
/// destination rectangle and `clip` the index of the optional clip
/// rectangle (`NO_CLIP` for none). Both are replaced by scaled copies; the
/// game's own rectangles are not changed.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_rects(args: *mut *mut Rect, clip: u32) {
    let hud = hud();
    let Some(canvas) = hud.canvas else { return };
    // SAFETY: the stub passes the caller's argument area.
    unsafe {
        let dst = args.add(1);
        if !(*dst).is_null() {
            hud.dst = canvas.map(**dst);
            *dst = addr_of_mut!(hud.dst);
        }
        if clip >= NO_CLIP {
            return;
        }
        let c = args.add(clip as usize);
        if !(*c).is_null() {
            hud.clip = canvas.map(**c);
            *c = addr_of_mut!(hud.clip);
        }
    }
}
