//! The functions the launcher's stubs call (stdcall, 32-bit). They run on
//! the game's main thread.

use core::ptr::addr_of_mut;

use crate::canvas::{Canvas, Rect};
use crate::sections::{Screen, Sections};

/// `cmr_hud_rects`' clip index for calls without a clip rectangle (the
/// stubs push 0x7f; any index from 16 up means none).
const NO_CLIP: u32 = 16;

/// The game's display state; `+0` width, `+4` height.
const DISPLAY: *const *mut i32 = 0x0050_2518 as *const *mut i32;

/// The game's current top-level state (`0x5590a8`). States 4 (loading) to
/// 0xb are the race and its overlays (finish, results, replay); the front
/// end (1..=3) has its own 640x480 picture (F16) and is left alone.
const STATE: *const i32 = 0x0055_90a8 as *const i32;

/// The game's own memory, as `Sections` sees it.
struct Game;

impl Screen for Game {
    fn size(&self) -> Option<(i32, i32)> {
        // SAFETY: fixed globals of the one executable these fixes are keyed
        // to; the display struct starts with width and height.
        unsafe {
            let d = *DISPLAY;
            (!d.is_null()).then(|| (*d, *d.add(1)))
        }
    }

    fn set_size(&mut self, s: (i32, i32)) {
        // SAFETY: as in `size`.
        unsafe {
            let d = *DISPLAY;
            if !d.is_null() {
                *d = s.0;
                *d.add(1) = s.1;
            }
        }
    }

    fn in_race(&self) -> bool {
        // SAFETY: as in `size`.
        (4..=0xb).contains(&unsafe { *STATE })
    }
}

/// The scaling state and the scaled copies of the rectangles handed to the
/// game's 2D primitives.
struct Hud {
    sections: Sections,
    dst: Rect,
    clip: Rect,
    one: Rect,
}

static mut HUD: Hud = Hud {
    sections: Sections::new(),
    dst: Rect::ZERO,
    clip: Rect::ZERO,
    one: Rect::ZERO,
};

fn hud() -> &'static mut Hud {
    // SAFETY: only the game's main thread calls these exports.
    unsafe { &mut *addr_of_mut!(HUD) }
}

/// Around each game tick (`0x443e90`): in a race, the frame's 2D sees the
/// virtual screen.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_frame_begin() {
    hud().sections.enter(false, &mut Game);
}

/// Around the race HUD's layout and draw calls: as a frame, with the HUD's
/// side pieces anchored to the screen edges.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_begin() {
    hud().sections.enter(true, &mut Game);
}

/// After a frame or HUD section.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_end() {
    hud().sections.leave(&mut Game);
}

/// Around code that needs the real screen (display and 3D set-up, the 3D
/// projection and draw).
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_real_begin() {
    hud().sections.real_begin(&mut Game);
}

/// After a real section.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_real_end() {
    hud().sections.real_end(&mut Game);
}

/// Replaces the rectangle argument at `slot` (if not null) by its scaled
/// copy kept in `store`; the game's own rectangle is not changed.
///
/// # Safety
/// `slot` must point at a rectangle argument of the hooked call.
unsafe fn scale_arg(slot: *mut *mut Rect, canvas: &Canvas, store: &mut Rect) {
    unsafe {
        if !(*slot).is_null() {
            *store = canvas.map(**slot);
            *slot = store;
        }
    }
}

/// At the entry of a sprite-queue call (`0x49b420`/`0x49b1d0`): `args`
/// points at its arguments; argument 2 is the destination rectangle and
/// `clip` the index of the optional clip rectangle (`NO_CLIP` for none).
/// Both are replaced by scaled copies.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_rects(args: *mut *mut Rect, clip: u32) {
    let hud = hud();
    let Some(canvas) = hud.sections.canvas() else {
        return;
    };
    // SAFETY: the stub passes the caller's argument area.
    unsafe {
        scale_arg(args.add(1), &canvas, &mut hud.dst);
        if clip < NO_CLIP {
            scale_arg(args.add(clip as usize), &canvas, &mut hud.clip);
        }
    }
}

// IDirectDrawSurface (v1) methods, by vtable offset.
type Lock = unsafe extern "system" fn(*mut usize, *const u8, *mut u8, u32, usize) -> i32;
type Unlock = unsafe extern "system" fn(*mut usize, *mut u8) -> i32;
const LOCK: usize = 0x64 / 4;
const UNLOCK: usize = 0x80 / 4;
const DDLOCK_WAIT: u32 = 1;
const DDLOCK_READONLY: u32 = 0x10;
const DDLOCK_WRITEONLY: u32 = 0x20;

/// DDSURFACEDESC (108 bytes) as far as it's read here.
#[repr(C)]
struct SurfaceDesc {
    size: u32,
    flags: u32,
    height: u32,
    width: u32,
    pitch: i32,
    _rest1: [u32; 4],
    surface: *mut u8,
    _rest2: [u32; 8],
    // DDPIXELFORMAT at 0x48
    pf_size: u32,
    pf_flags: u32,
    pf_fourcc: u32,
    bits: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
    _alpha_mask: u32,
    _caps: [u32; 1],
}
const _: () = assert!(core::mem::size_of::<SurfaceDesc>() == 108);

unsafe fn method<T>(obj: *mut usize, index: usize) -> T {
    // SAFETY: COM objects start with a vtable pointer.
    unsafe { core::mem::transmute_copy(&*(*obj as *const usize).add(index)) }
}

/// Locks a surface; `flags` adds DDLOCK_READONLY or DDLOCK_WRITEONLY, which
/// spares the copy back from (or to) video memory a full lock costs.
unsafe fn lock(surface: *mut usize, flags: u32) -> Option<SurfaceDesc> {
    let mut desc: SurfaceDesc = unsafe { core::mem::zeroed() };
    desc.size = core::mem::size_of::<SurfaceDesc>() as u32;
    let f: Lock = unsafe { method(surface, LOCK) };
    let hr = unsafe {
        f(
            surface,
            core::ptr::null(),
            (&raw mut desc).cast(),
            DDLOCK_WAIT | flags,
            0,
        )
    };
    (hr == 0 && !desc.surface.is_null()).then_some(desc)
}

unsafe fn unlock(surface: *mut usize) {
    let f: Unlock = unsafe { method(surface, UNLOCK) };
    unsafe { f(surface, core::ptr::null_mut()) };
}

/// Instead of the front end's `Flip` (`0x49870f`): when the game draws at
/// 640x480 but the screen is larger, copy the picture to the screen enlarged
/// and return 1 (the caller then skips the flip). Otherwise return 0 and the
/// game flips as it always did.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_fe_present(display: *mut u8) -> u32 {
    // A race frame's virtual 640x480 is not the front end's picture.
    if display.is_null() || hud().sections.active() {
        return 0;
    }
    // SAFETY: the game's display struct: +0 W, +4 H, +0x10 primary, +0x428
    // back buffer (IDirectDrawSurface pointers).
    unsafe {
        let (w, h) = (*(display as *const i32), *(display.add(4) as *const i32));
        let primary = *(display.add(0x10) as *const *mut usize);
        let back = *(display.add(0x428) as *const *mut usize);
        if (w, h) != (640, 480) || primary.is_null() || back.is_null() {
            return 0;
        }
        let Some(src) = lock(back, DDLOCK_READONLY) else {
            return 0;
        };
        let done = match lock(primary, DDLOCK_WRITEONLY) {
            Some(dst)
                if dst.width > 640
                    && dst.pitch > 0
                    && src.pitch > 0
                    && src.bits == 16
                    && (dst.bits == 16 || dst.bits == 32) =>
            {
                crate::present::blit(
                    src.surface,
                    src.pitch as usize,
                    src.green_mask == 0x7e0,
                    dst.surface,
                    dst.pitch as usize,
                    dst.bits,
                    dst.width,
                    dst.height,
                );
                unlock(primary);
                1
            }
            Some(_) => {
                unlock(primary);
                0
            }
            None => 0,
        };
        unlock(back);
        done
    }
}

/// At the entry of a call that takes one destination rectangle among its
/// arguments (the direct blit `0x49e3e0`, argument 4; a panel `0x49e200`,
/// argument 2): `args` points at argument 1, `index` is the rectangle's
/// 0-based position. Replaced by a scaled copy while the HUD canvas is on.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_rect_at(args: *mut *mut Rect, index: u32) {
    let hud = hud();
    let Some(canvas) = hud.sections.canvas() else {
        return;
    };
    if index >= 8 {
        return;
    }
    // SAFETY: the stub passes the caller's argument area.
    unsafe { scale_arg(args.add(index as usize), &canvas, &mut hud.one) };
}
