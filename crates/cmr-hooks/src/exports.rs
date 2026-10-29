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
    one: Rect,
}

static mut HUD: Hud = Hud {
    depth: 0,
    real: (0, 0),
    canvas: None,
    dst: Rect::ZERO,
    clip: Rect::ZERO,
    one: Rect::ZERO,
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

/// Before a race HUD layout or draw call: show the game a 640x480 screen,
/// its side pieces anchored to the screen edges.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_begin() {
    begin(true);
}

/// Before a screen laid out as a whole (loading, results, pause, messages):
/// show the game a 640x480 screen, mapped centred.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_begin_whole() {
    begin(false);
}

fn begin(anchored: bool) {
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
    hud.canvas = Some(Canvas::new(real.0, real.1, anchored));
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
    let Some(canvas) = hud.canvas else { return };
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
    if display.is_null() {
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
    let Some(canvas) = hud.canvas else { return };
    if index >= 8 {
        return;
    }
    // SAFETY: the stub passes the caller's argument area.
    unsafe { scale_arg(args.add(index as usize), &canvas, &mut hud.one) };
}
