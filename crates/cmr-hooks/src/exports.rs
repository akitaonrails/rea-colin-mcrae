//! The functions the launcher's stubs call (stdcall, 32-bit). They run on
//! the game's main thread.

use core::ptr::addr_of_mut;

use crate::canvas::{Canvas, Rect};
use crate::pacing::Pacer;
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

/// A Win32 `CRITICAL_SECTION` (24 bytes on x86), recursive per thread.
#[repr(C, align(4))]
struct CriticalSection([u32; 6]);

#[link(name = "kernel32")]
unsafe extern "system" {
    fn InitializeCriticalSection(lock: *mut CriticalSection);
    fn EnterCriticalSection(lock: *mut CriticalSection);
    fn LeaveCriticalSection(lock: *mut CriticalSection);
    fn GetCurrentThreadId() -> u32;
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
    fn Sleep(ms: u32);
}

/// The scaling state, the lock that keeps updates and real sections apart,
/// and the scaled copies of the rectangles handed to the 2D primitives.
struct Hud {
    sections: Sections,
    lock: CriticalSection,
    /// The thread drawing the race HUD (the main thread).
    hud_thread: u32,
    dst: Rect,
    clip: Rect,
    one: Rect,
}

static mut HUD: Hud = Hud {
    sections: Sections::new(),
    lock: CriticalSection([0; 6]),
    hud_thread: 0,
    dst: Rect::ZERO,
    clip: Rect::ZERO,
    one: Rect::ZERO,
};

fn hud() -> &'static mut Hud {
    // SAFETY: `sections` changes only inside the lock or on the main thread
    // (HUD sections); readers outside it see whole values.
    unsafe { &mut *addr_of_mut!(HUD) }
}

fn enter() {
    // SAFETY: initialised when the library is loaded (`init`).
    unsafe { EnterCriticalSection(&raw mut hud().lock) }
}

fn leave() {
    // SAFETY: as in `enter`.
    unsafe { LeaveCriticalSection(&raw mut hud().lock) }
}

/// The 2D mapping for the calling thread, while scaling is on.
fn canvas() -> Option<Canvas> {
    let hud = hud();
    // SAFETY: a plain Win32 call.
    let anchored = unsafe { GetCurrentThreadId() } == hud.hud_thread;
    hud.sections.canvas(anchored)
}

/// Called once when the library is loaded.
pub fn init() {
    // SAFETY: before any export can run.
    unsafe { InitializeCriticalSection(&raw mut hud().lock) }
}

/// Before each game update (`0x443db0`, on the game's timer thread or the
/// main thread): keeps the race's virtual screen in place; never runs while
/// a real section has the real screen.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_update_begin() {
    enter();
    hud().sections.update(&mut Game);
}

/// After a game update.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_update_end() {
    leave();
}

/// Around the race HUD's layout and draw calls: its side pieces anchor to
/// the screen edges.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_begin() {
    let hud = hud();
    // SAFETY: a plain Win32 call.
    hud.hud_thread = unsafe { GetCurrentThreadId() };
    hud.sections.hud_begin();
}

/// After a HUD section.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_end() {
    hud().sections.hud_end();
}

/// Around code that needs the real screen (display and 3D set-up, clears,
/// the 3D projection and scene draw).
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_real_begin() {
    enter();
    hud().sections.real_begin(&mut Game);
}

/// After a real section.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_real_end() {
    hud().sections.real_end(&mut Game);
    leave();
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
    let Some(canvas) = canvas() else {
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
type GetSurfaceDesc = unsafe extern "system" fn(*mut usize, *mut SurfaceDesc) -> i32;
const GET_SURFACE_DESC: usize = 0x58 / 4;
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
    unsafe { lock_rect(surface, core::ptr::null(), flags) }
}

/// `lock` of a part of the surface (`rect`: left, top, right, bottom); the
/// returned pointer is the part's top left.
unsafe fn lock_rect(surface: *mut usize, rect: *const [i32; 4], flags: u32) -> Option<SurfaceDesc> {
    let mut desc: SurfaceDesc = unsafe { core::mem::zeroed() };
    desc.size = core::mem::size_of::<SurfaceDesc>() as u32;
    let f: Lock = unsafe { method(surface, LOCK) };
    let hr = unsafe {
        f(
            surface,
            rect.cast(),
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
        let Some(screen) = desc_of(primary) else {
            return 0;
        };
        if screen.width <= 640 || !(screen.bits == 16 || screen.bits == 32) {
            return 0;
        }
        let Some(src) = lock(back, DDLOCK_READONLY) else {
            return 0;
        };
        if src.bits != 16 || src.pitch <= 0 {
            unlock(back);
            return 0;
        }
        // The black bars only need drawing now and then (a new screen, and
        // once a second in case something drew over them); the rest of the
        // time only the picture is locked, uploaded and drawn.
        let full = &mut *addr_of_mut!(FULL);
        full.frames = if full.primary == primary {
            full.frames + 1
        } else {
            0
        };
        full.primary = primary;
        let (x, y, w, h) = crate::present::target(screen.width, screen.height);
        let whole = full.frames.is_multiple_of(60);
        let rect = [x as i32, y as i32, (x + w) as i32, (y + h) as i32];
        let dst = if whole {
            lock(primary, DDLOCK_WRITEONLY)
        } else {
            lock_rect(primary, &rect, DDLOCK_WRITEONLY)
        };
        let done = match dst {
            Some(dst) if dst.pitch > 0 => {
                let (src_px, pitch, green6) =
                    (src.surface, src.pitch as usize, src.green_mask == 0x7e0);
                let (out, out_pitch) = (dst.surface, dst.pitch as usize);
                if whole {
                    crate::present::blit(
                        src_px,
                        pitch,
                        green6,
                        out,
                        out_pitch,
                        screen.bits,
                        screen.width,
                        screen.height,
                    );
                } else {
                    crate::present::blit_picture(
                        src_px,
                        pitch,
                        green6,
                        out,
                        out_pitch,
                        screen.bits,
                        w,
                        h,
                    );
                }
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

/// When the front end's bars were last drawn (`cmr_fe_present`).
struct FullFrames {
    primary: *mut usize,
    frames: u32,
}

static mut FULL: FullFrames = FullFrames {
    primary: core::ptr::null_mut(),
    frames: 0,
};

fn desc_of(surface: *mut usize) -> Option<SurfaceDesc> {
    // SAFETY: a live DirectDraw surface; the descriptor is sized for it.
    unsafe {
        let mut desc: SurfaceDesc = core::mem::zeroed();
        desc.size = core::mem::size_of::<SurfaceDesc>() as u32;
        let get: GetSurfaceDesc = method(surface, GET_SURFACE_DESC);
        (get(surface, &mut desc) == 0).then_some(desc)
    }
}

/// The frame-rate cap and vsync choice, filled by the launcher (F17).
#[repr(C)]
pub struct PresentSettings {
    /// Frames per second, 0 for no cap.
    fps_limit: u32,
    /// `Flip` flags (`DDFLIP_WAIT`, `DDFLIP_NOVSYNC`), 0 for the game's own.
    flip_flags: u32,
}

type Flip = unsafe extern "system" fn(*mut usize, *mut usize, u32) -> i32;
const FLIP: usize = 0x2c / 4;

static mut PACER: Pacer = Pacer::new();

fn now() -> i64 {
    let mut t = 0;
    // SAFETY: a plain Win32 call.
    unsafe { QueryPerformanceCounter(&mut t) };
    t
}

/// Waits until the cap lets the next frame be shown: sleeps most of the
/// time, then spins the last millisecond or two (Sleep is coarse).
fn pace(limit: u32) {
    let mut freq = 0;
    // SAFETY: a plain Win32 call.
    unsafe { QueryPerformanceFrequency(&mut freq) };
    // SAFETY: only the main thread presents.
    let pacer = unsafe { &mut *addr_of_mut!(PACER) };
    let start = now();
    let deadline = start + pacer.wait(start, freq, limit);
    loop {
        let left = deadline - now();
        if left <= 0 {
            break;
        }
        let ms = left * 1000 / freq;
        if ms > 2 {
            // SAFETY: a plain Win32 call.
            unsafe { Sleep((ms - 2) as u32) };
        } else {
            core::hint::spin_loop();
        }
    }
}

/// Instead of each frame's `Flip` (`0x498705`, F17): waits for the frame
/// cap, enlarges the front end's picture when it needs it, else flips with
/// the chosen vsync. Returns 1 when the frame was shown, 0 to let the game
/// flip as it always did.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_present(display: *mut u8, settings: *const PresentSettings) -> u32 {
    if display.is_null() || settings.is_null() {
        return 0;
    }
    // SAFETY: the launcher's settings block in the fix memory.
    let settings = unsafe { &*settings };
    pace(settings.fps_limit);
    if cmr_fe_present(display) != 0 {
        return 1;
    }
    if settings.flip_flags == 0 {
        return 0;
    }
    // SAFETY: the game's display struct, +0x10 the primary surface.
    unsafe {
        let primary = *(display.add(0x10) as *const *mut usize);
        if primary.is_null() {
            return 0;
        }
        let flip: Flip = method(primary, FLIP);
        (flip(primary, core::ptr::null_mut(), settings.flip_flags) == 0) as u32
    }
}

/// At the entry of a call that takes one destination rectangle among its
/// arguments (the direct blit `0x49e3e0`, argument 4; a panel `0x49e200`,
/// argument 2): `args` points at argument 1, `index` is the rectangle's
/// 0-based position. Replaced by a scaled copy while the HUD canvas is on.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hud_rect_at(args: *mut *mut Rect, index: u32) {
    let hud = hud();
    let Some(canvas) = canvas() else {
        return;
    };
    if index >= 8 {
        return;
    }
    // SAFETY: the stub passes the caller's argument area.
    unsafe { scale_arg(args.add(index as usize), &canvas, &mut hud.one) };
}
