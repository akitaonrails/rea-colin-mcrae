//! The functions the launcher's stubs call (stdcall, 32-bit). They run on
//! the game's main thread.

use core::ptr::addr_of_mut;

use crate::canvas::{Canvas, Rect};
use crate::easing::Easer;
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
        (4..=0xb).contains(&self.state())
    }
}

impl Game {
    /// The game's top-level state (`0x5590a8`).
    fn state(&self) -> i32 {
        // SAFETY: a fixed global of the one executable these fixes are keyed to.
        unsafe { *STATE }
    }
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThreadId() -> u32;
    fn GetCurrentThread() -> *mut u8;
    fn GetCurrentProcess() -> *mut u8;
    fn GetProcessAffinityMask(process: *mut u8, mask: *mut usize, system: *mut usize) -> i32;
    fn SetThreadAffinityMask(thread: *mut u8, mask: usize) -> usize;
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
    fn Sleep(ms: u32);
    fn LoadLibraryA(name: *const u8) -> *mut u8;
    fn GetEnvironmentVariableA(name: *const u8, buf: *mut u8, size: u32) -> u32;
    fn CreateFileA(
        name: *const u8,
        access: u32,
        share: u32,
        sec: *mut u8,
        disp: u32,
        flags: u32,
        tmpl: *mut u8,
    ) -> *mut u8;
    fn SetFilePointer(file: *mut u8, lo: i32, hi: *mut i32, method: u32) -> u32;
    fn WriteFile(file: *mut u8, buf: *const u8, len: u32, written: *mut u32, ov: *mut u8) -> i32;
    fn GetProcAddress(module: *mut u8, name: *const u8) -> *mut u8;
    fn SetUnhandledExceptionFilter(filter: Option<ExceptionFilter>) -> Option<ExceptionFilter>;
    fn GetModuleHandleExA(flags: u32, name: *const u8, module: *mut *mut u8) -> i32;
    fn GetModuleFileNameA(module: *mut u8, buf: *mut u8, size: u32) -> u32;
    fn CloseHandle(handle: *mut u8) -> i32;
    fn ReadFile(file: *mut u8, buf: *mut u8, len: u32, read: *mut u32, ov: *mut u8) -> i32;
    fn GetFileSize(file: *mut u8, high: *mut u32) -> u32;
    fn GetProcessHeap() -> *mut u8;
    fn HeapAlloc(heap: *mut u8, flags: u32, bytes: usize) -> *mut u8;
    fn HeapFree(heap: *mut u8, flags: u32, mem: *mut u8) -> i32;
    fn VirtualProtect(addr: *mut u8, size: usize, protect: u32, old: *mut u32) -> i32;
    fn CreateThread(
        attrs: *mut u8,
        stack: usize,
        start: unsafe extern "system" fn(*mut u8) -> u32,
        param: *mut u8,
        flags: u32,
        id: *mut u32,
    ) -> *mut u8;
    fn WaitForMultipleObjects(n: u32, handles: *const *mut u8, all: i32, ms: u32) -> u32;
}

/// One band of rows of the front end's enlarged picture, for a helper thread.
struct Band<'a> {
    sample: &'a (dyn Fn(f32, f32) -> u16 + Sync),
    green6: bool,
    dst: *mut u8,
    pitch: usize,
    bits: u32,
    w: u32,
    h: u32,
    from: u32,
    to: u32,
}

unsafe extern "system" fn draw_band(param: *mut u8) -> u32 {
    // SAFETY: `blit_sampled_parallel` keeps the band alive until this
    // thread has ended; bands write disjoint rows.
    unsafe {
        let b = &*(param as *const Band);
        crate::present::blit_sampled_rows(
            b.sample, b.green6, b.dst, b.pitch, b.bits, b.w, b.h, b.from, b.to,
        );
    }
    0
}

/// `present::blit_sampled` spread over helper threads. Composing the HD
/// front end samples every screen pixel (6 million at 4K), and the main
/// thread is pinned to one core (`share_core`: on hybrid CPUs often an
/// efficiency core), which held the menus near 20 fps. The helpers are not
/// pinned, so Windows runs them on the fast cores; the main thread waits.
/// Falls back to drawing everything here when a thread can't start.
unsafe fn blit_sampled_parallel(
    sample: &(dyn Fn(f32, f32) -> u16 + Sync),
    green6: bool,
    dst: *mut u8,
    pitch: usize,
    bits: u32,
    w: u32,
    h: u32,
) {
    const BANDS: usize = 16;
    let step = h.div_ceil(BANDS as u32);
    let bands: [Band; BANDS] = core::array::from_fn(|i| Band {
        sample,
        green6,
        dst,
        pitch,
        bits,
        w,
        h,
        from: (i as u32 * step).min(h),
        to: ((i as u32 + 1) * step).min(h),
    });
    let mut threads = [core::ptr::null_mut::<u8>(); BANDS];
    let mut started = 0;
    // SAFETY: plain Win32 calls; every started thread is waited for before
    // `bands` goes out of scope.
    unsafe {
        for (i, band) in bands.iter().enumerate().skip(1) {
            let t = CreateThread(
                core::ptr::null_mut(),
                64 * 1024,
                draw_band,
                band as *const Band as *mut u8,
                0,
                core::ptr::null_mut(),
            );
            if t.is_null() {
                // This band and the rest are drawn here.
                for b in &bands[i..] {
                    draw_band(b as *const Band as *mut u8);
                }
                break;
            }
            threads[started] = t;
            started += 1;
        }
        draw_band(&bands[0] as *const Band as *mut u8);
        if started > 0 {
            WaitForMultipleObjects(started as u32, threads.as_ptr(), 1, 0xffff_ffff);
            for &t in &threads[..started] {
                CloseHandle(t);
            }
        }
    }
}

type ExceptionFilter = unsafe extern "system" fn(*mut u8) -> i32;

/// The scaling state and the scaled copies of the rectangles handed to the
/// 2D primitives.
struct Hud {
    sections: Sections,
    /// The thread drawing the race HUD (the main thread).
    hud_thread: u32,
    dst: Rect,
    clip: Rect,
    one: Rect,
}

static mut HUD: Hud = Hud {
    sections: Sections::new(),
    hud_thread: 0,
    dst: Rect::ZERO,
    clip: Rect::ZERO,
    one: Rect::ZERO,
};

fn hud() -> &'static mut Hud {
    // SAFETY: `sections` changes on the main thread, or on the update thread
    // only while a race starts or ends; readers see whole values.
    unsafe { &mut *addr_of_mut!(HUD) }
}

/// The 3D view block in the fix memory (F13, `F13_VIEW`): read by the
/// launcher's stubs at the 3D code's screen-size reads, which take
/// `shadow` instead of the game's display state when the calling thread is
/// `main` and `depth` is not 0.
#[repr(C)]
pub struct View {
    depth: u32,
    main: u32,
    shadow: *const u8,
}

/// The game's display state is `0x2c70` bytes; the 3D code sees this copy
/// with the real screen size while the game's own holds 640x480.
#[repr(C, align(16))]
struct Shadow([u8; SHADOW_LEN]);
const SHADOW_LEN: usize = 0x2c80;
static mut SHADOW: Shadow = Shadow([0; SHADOW_LEN]);
/// The view block, as last handed to `cmr_3d_begin`.
static mut VIEW: *mut View = core::ptr::null_mut();

/// The 2D mapping for the calling thread, while scaling is on (paused for
/// the main thread while it draws 3D).
fn canvas() -> Option<Canvas> {
    let hud = hud();
    // SAFETY: a plain Win32 call; the view block is the fix memory's.
    let me = unsafe { GetCurrentThreadId() };
    let view = unsafe { *addr_of_mut!(VIEW) };
    if !view.is_null() && unsafe { (*view).depth > 0 && (*view).main == me } {
        return None;
    }
    hud.sections.canvas(me == hud.hud_thread)
}

/// Before each game update (`0x443db0`, on the game's timer thread or the
/// main thread): keeps the race's virtual screen in place.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_update_begin() {
    share_core(&raw mut UPDATE_THREAD);
    install_crash_filter();
    diag_update();
    hud().sections.update(&mut Game);
}

/// Before 3D work on the main thread (the projection, the clears, the scene
/// draw): in a race the game's screen-size reads there see the real screen
/// (a copy of the display state, refreshed here), and 2D scaling pauses for
/// this thread. Nothing changes for the update thread, and nothing waits.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_3d_begin(view: *mut View) {
    if view.is_null() {
        return;
    }
    // SAFETY: the view block in the fix memory; the shadow is ours and only
    // written here, on the main thread.
    unsafe {
        *addr_of_mut!(VIEW) = view;
        let v = &mut *view;
        v.depth += 1;
        v.main = GetCurrentThreadId();
        if v.depth > 1 {
            return;
        }
        let shadow = addr_of_mut!(SHADOW) as *mut u8;
        match (hud().sections.real_size(), *DISPLAY) {
            (Some((w, h)), d) if !d.is_null() => {
                core::ptr::copy_nonoverlapping(d as *const u8, shadow, SHADOW_LEN);
                *(shadow as *mut i32) = w;
                *(shadow.add(4) as *mut i32) = h;
                v.shadow = shadow;
            }
            // Not in a race at a scaled size: the game's own state.
            _ => v.depth = 0,
        }
    }
}

/// After 3D work.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_3d_end(view: *mut View) {
    // SAFETY: the view block in the fix memory.
    if let Some(v) = unsafe { view.as_mut() } {
        v.depth = v.depth.saturating_sub(1);
    }
}

/// Timing diagnostics (`CMRHOOKS_LOG=<file>`): every 5 s, one line with
/// the game updates, the longest gap between updates and the longest frame.
/// Off unless the variable is set.
struct Diag {
    /// None until checked; Some(null) when off.
    file: Option<*mut u8>,
    window: i64,
    updates: u32,
    last_update: i64,
    gap_max: i64,
    last_frame: i64,
    frame_max: i64,
    frames: u32,
    /// Rumble: impacts seen, the strongest (Q16), the fastest motor speed
    /// sent and the sends XInput refused.
    impacts: u32,
    severity_max: i32,
    motor_max: u16,
    send_errors: u32,
    /// HD art: texture binds that used an HD twin, and those refused
    /// because a record had changed.
    hd_binds: u32,
    hd_stale: u32,
    /// HD art in the front end: blits into the menu picture, and twins
    /// drawn over it.
    fe_draws: u32,
    fe_shown: u32,
}

static mut DIAG: Diag = Diag::ZERO;

fn diag() -> Option<&'static mut Diag> {
    // SAFETY: updates are serialised by the lock and frames run on the main
    // thread; a torn counter in a diagnostic line is harmless.
    let d = unsafe { &mut *addr_of_mut!(DIAG) };
    if d.file.is_none() {
        let mut path = [0u8; 260];
        // SAFETY: plain Win32 calls with buffers of the stated sizes.
        let file = unsafe {
            let n =
                GetEnvironmentVariableA(c"CMRHOOKS_LOG".as_ptr().cast(), path.as_mut_ptr(), 259);
            if n == 0 || n >= 259 {
                core::ptr::null_mut()
            } else {
                // GENERIC_WRITE, share read, OPEN_ALWAYS, normal file
                let f = CreateFileA(
                    path.as_ptr(),
                    0x4000_0000,
                    1,
                    core::ptr::null_mut(),
                    4,
                    0x80,
                    core::ptr::null_mut(),
                );
                if f as isize == -1 {
                    core::ptr::null_mut()
                } else {
                    f
                }
            }
        };
        d.file = Some(file);
    }
    (!d.file?.is_null()).then_some(d)
}

fn diag_update() {
    let Some(d) = diag() else { return };
    let got = now();
    d.updates += 1;
    if d.last_update != 0 {
        d.gap_max = d.gap_max.max(got - d.last_update);
    }
    d.last_update = got;
}

fn diag_frame() {
    let Some(d) = diag() else { return };
    let t = now();
    let mut freq = 0;
    // SAFETY: a plain Win32 call.
    unsafe { QueryPerformanceFrequency(&mut freq) };
    if d.last_frame != 0 {
        d.frame_max = d.frame_max.max(t - d.last_frame);
    }
    d.frames += 1;
    d.last_frame = t;
    if d.window == 0 {
        d.window = t;
    }
    if t - d.window < 5 * freq || freq <= 0 {
        return;
    }
    let ms = |ticks: i64| (ticks * 1000 / freq) as u32;
    let mut line = Line::default();
    line.text(b"updates ")
        .num(d.updates)
        .text(b", update gap max ")
        .num(ms(d.gap_max))
        .text(b" ms, fps ")
        .num((d.frames as i64 * freq / (t - d.window).max(1)) as u32)
        .text(b", frame max ")
        .num(ms(d.frame_max))
        .text(b" ms, impacts ")
        .num(d.impacts)
        .text(b" max ")
        .num((d.severity_max.max(0) as u32 * 100) >> 16)
        .text(b"%, motor max ")
        .num(u32::from(d.motor_max) * 100 / 65535)
        .text(b"%, send errors ")
        .num(d.send_errors)
        .text(b", hd pairs ")
        // SAFETY: a count the main thread writes; a diagnostic.
        .num(unsafe { (*addr_of_mut!(HD_PAIRS)).1 } as u32)
        .text(b" binds ")
        .num(d.hd_binds)
        .text(b" stale ")
        .num(d.hd_stale)
        .text(if unsafe { (*addr_of_mut!(SET_RENDER_STATE)).is_some() } {
            b" (hooked)"
        } else {
            b" (not hooked)"
        })
        .text(b", fe twins ")
        // SAFETY: as above.
        .num(unsafe { (*addr_of_mut!(FE_TWINS)).1 } as u32)
        .text(b" draws ")
        .num(d.fe_draws)
        .text(b" shown ")
        .num(d.fe_shown)
        .text(b", state ")
        .num(Game.state() as u32)
        .text(b"\r\n");
    if let Some(f) = d.file {
        let mut written = 0;
        // SAFETY: an open file handle and a buffer of the stated length.
        unsafe {
            SetFilePointer(f, 0, core::ptr::null_mut(), 2);
            WriteFile(
                f,
                line.buf.as_ptr(),
                line.len as u32,
                &mut written,
                core::ptr::null_mut(),
            );
        }
    }
    *d = Diag {
        file: d.file,
        window: t,
        last_update: d.last_update,
        last_frame: t,
        ..Diag::ZERO
    };
}

impl Diag {
    const ZERO: Diag = Diag {
        file: None,
        window: 0,
        updates: 0,
        last_update: 0,
        gap_max: 0,
        last_frame: 0,
        frame_max: 0,
        frames: 0,
        impacts: 0,
        severity_max: 0,
        motor_max: 0,
        send_errors: 0,
        hd_binds: 0,
        hd_stale: 0,
        fe_draws: 0,
        fe_shown: 0,
    };
}

/// A small text buffer for the diagnostics (no `std` formatting here).
struct Line {
    buf: [u8; 400],
    len: usize,
}

impl Default for Line {
    fn default() -> Line {
        Line {
            buf: [0; 400],
            len: 0,
        }
    }
}

impl Line {
    fn text(&mut self, t: &[u8]) -> &mut Line {
        for &b in t {
            if self.len < self.buf.len() {
                self.buf[self.len] = b;
                self.len += 1;
            }
        }
        self
    }

    fn hex(&mut self, n: u32) -> &mut Line {
        let mut digits = *b"0x00000000";
        for (i, d) in digits[2..].iter_mut().enumerate() {
            let v = (n >> (28 - 4 * i)) & 0xf;
            *d = if v < 10 {
                b'0' + v as u8
            } else {
                b'a' + v as u8 - 10
            };
        }
        self.text(&digits)
    }

    fn int(&mut self, n: i32) -> &mut Line {
        if n < 0 {
            self.text(b"-");
        }
        self.num(n.unsigned_abs())
    }

    fn num(&mut self, mut n: u32) -> &mut Line {
        let mut digits = [0u8; 10];
        let mut i = digits.len();
        loop {
            i -= 1;
            digits[i] = b'0' + (n % 10) as u8;
            n /= 10;
            if n == 0 {
                break;
            }
        }
        self.text(&digits[i..])
    }
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

/// Around the display and 3D device set-up (a stage loading, the menus
/// coming back): the game's display state briefly holds the real size.
/// These run while the game has no update thread, so nothing else reads it.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_real_begin() {
    hud().sections.real_begin(&mut Game);
}

/// After a set-up section.
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
    // SAFETY: the queue's own arguments (`0x49b420(src, dst, texture,
    // ...)`, rectangles of four shorts), read before any scaling.
    unsafe { trace_queue(args as *const usize) };
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
type Release = unsafe extern "system" fn(*mut usize) -> u32;
const RELEASE: usize = 2;
const GET_SURFACE_DESC: usize = 0x58 / 4;
type Unlock = unsafe extern "system" fn(*mut usize, *mut u8) -> i32;
const LOCK: usize = 0x64 / 4;
const UNLOCK: usize = 0x80 / 4;
const DDLOCK_WAIT: u32 = 1;
const DDLOCK_READONLY: u32 = 0x10;
const DDLOCK_WRITEONLY: u32 = 0x20;

/// DDSURFACEDESC (108 bytes) as far as it's read here.
#[repr(C)]
pub struct SurfaceDesc {
    size: u32,
    flags: u32,
    height: u32,
    width: u32,
    pitch: i32,
    /// `+0x14..0x24`; `[1]` (`+0x18`) is the mipmap count.
    rest1: [u32; 4],
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
    alpha_mask: u32,
    caps: u32,
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

/// Instead of the game's depth test `0x49fa70(x, y)` (F26): 1 when nothing
/// is drawn in front at screen point (x, y), as the game's own answer, but
/// reading only that pixel of the depth buffer (the game locked all of it).
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_depth_visible(x: i32, y: i32) -> u32 {
    // SAFETY: the game's globals: 0x50283c its 3D settings (+0x28 the
    // effect's switch), 0x502518 the display struct (+0x840 the depth
    // buffer surface).
    unsafe {
        let settings = *(0x0050_283c as *const *const u8);
        if settings.is_null() || *(settings.add(0x28) as *const u32) == 0 {
            return 0;
        }
        let display = *(0x0050_2518 as *const *const u8);
        let depth = *(display.add(0x840) as *const *mut usize);
        if depth.is_null() {
            return 0;
        }
        // A point off the screen (the game passes -1, -1 when there is
        // none) has nothing to see: the game's own read went outside the
        // buffer there and almost never found the far value.
        let Some(size) = desc_of(depth) else {
            return 0;
        };
        if x < 0 || y < 0 || x as u32 >= size.width || y as u32 >= size.height {
            return 0;
        }
        let one = [x, y, x + 1, y + 1];
        // The game: a failed lock counts as visible.
        let Some(px) = lock_rect(depth, &one, DDLOCK_READONLY) else {
            return 1;
        };
        // +0x18: the depth bits (dwZBufferBitDepth).
        let bits = px.rest1[1];
        let value = if bits == 16 {
            u32::from(*(px.surface as *const u16))
        } else {
            *(px.surface as *const u32)
        };
        unlock(depth);
        crate::depth::is_far(bits, value).map_or(0, u32::from)
    }
}

/// Before the front end's `Flip` (`0x49870f`): when the game draws at
/// 640x480 on a larger screen, enlarge its picture across the back buffer
/// (bars included) and let the game flip as it always did. Returns 0 (flip).
///
/// The picture used to be written straight into the visible screen with the
/// flip skipped, but under Wine with a GPU (GLX) inside gamescope, writes
/// to the visible buffer are only shown now and then; a flip always is.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_fe_present(display: *mut u8) -> u32 {
    // A race frame's virtual 640x480 is not the front end's picture.
    if display.is_null() || hud().sections.active() {
        forget_fe_draws();
        return 0;
    }
    // SAFETY: the game's display struct: +0 W, +4 H, +0x428 the back
    // buffer (an IDirectDrawSurface of the screen's size); the picture is
    // its top left 640x480, copied out before the back buffer is written.
    unsafe {
        let (w, h) = (*(display as *const i32), *(display.add(4) as *const i32));
        let back = *(display.add(0x428) as *const *mut usize);
        if (w, h) != (640, 480) || back.is_null() {
            forget_fe_draws();
            return 0;
        }
        let Some(desc) = desc_of(back) else {
            forget_fe_draws();
            return 0;
        };
        let (sw, sh) = (desc.width, desc.height);
        if sw <= 640 || sh < 480 {
            forget_fe_draws();
            return 0;
        }
        // The picture, read alone (a GPU copies back only what is locked).
        let corner = [0, 0, 640, 480];
        let Some(buf) = lock_rect(back, &corner, DDLOCK_READONLY) else {
            forget_fe_draws();
            return 0;
        };
        let ok = buf.bits == 16 && buf.pitch > 0;
        let green6 = buf.green_mask == 0x7e0;
        if ok {
            let scratch = &mut *addr_of_mut!(PICTURE);
            for y in 0..480 {
                core::ptr::copy_nonoverlapping(
                    buf.surface.add(y * buf.pitch as usize) as *const u16,
                    scratch.as_mut_ptr().add(y * 640),
                    640,
                );
            }
        }
        unlock(back);
        if !ok {
            forget_fe_draws();
            return 0;
        }
        let composed = compose_fe(back);
        fe_dump(back, composed);
        if let Some(buf) = lock(back, DDLOCK_WRITEONLY) {
            let pitch = buf.pitch as usize;
            if composed {
                let (layers, n) = &*addr_of_mut!(FE_LAYERS);
                let (top, under) = (&*addr_of_mut!(FE_TOP), &*addr_of_mut!(FE_UNDER));
                let picture = &*addr_of_mut!(PICTURE);
                blit_sampled_parallel(
                    &|fx, fy| crate::fe_compose::sample(&layers[..*n], top, under, picture, fx, fy),
                    green6,
                    buf.surface,
                    pitch,
                    buf.bits,
                    sw,
                    sh,
                );
            } else {
                crate::present::blit(
                    (*addr_of_mut!(PICTURE)).as_ptr() as *const u8,
                    640 * 2,
                    green6,
                    buf.surface,
                    pitch,
                    buf.bits,
                    sw,
                    sh,
                );
            }
            unlock(back);
        }
        forget_fe_draws();
        diag_frame();
        0
    }
}

/// A diagnostic (`CMR_FE_DUMP=<folder>`): every 3 seconds the front end's
/// picture (`pic.raw`, 640x480 16-bit) and the frame's blits into it
/// (`layers.txt`), overwritten. Off unless the variable is set.
static mut FE_DUMP: (Option<[u8; 260]>, usize, i64) = (None, 0, 0);
static mut FE_DUMP_SEQ: u32 = 0;

unsafe fn fe_dump(back: *mut usize, composed: bool) {
    // SAFETY: plain Win32 calls; the main thread's own tables.
    unsafe {
        let d = &mut *addr_of_mut!(FE_DUMP);
        if d.0.is_none() {
            let mut dir = [0u8; 260];
            let n = GetEnvironmentVariableA(c"CMR_FE_DUMP".as_ptr().cast(), dir.as_mut_ptr(), 230);
            d.1 = if n > 0 && n < 230 { n as usize } else { 0 };
            d.0 = Some(dir);
        }
        if d.1 == 0 {
            return;
        }
        let (mut t, mut freq) = (0i64, 0i64);
        QueryPerformanceCounter(&mut t);
        QueryPerformanceFrequency(&mut freq);
        if t - d.2 < 3 * freq {
            return;
        }
        d.2 = t;
        let dir = d.0.unwrap_or([0; 260]);
        let seq = &mut *addr_of_mut!(FE_DUMP_SEQ);
        *seq += 1;
        let tag = [
            b'0' + (*seq / 100 % 10) as u8,
            b'0' + (*seq / 10 % 10) as u8,
            b'0' + (*seq % 10) as u8,
        ];
        let open = |name: &[u8]| {
            let mut path = [0u8; 260];
            path[..d.1].copy_from_slice(&dir[..d.1]);
            path[d.1] = b'\\';
            path[d.1 + 1..d.1 + 4].copy_from_slice(&tag);
            path[d.1 + 4] = b'_';
            path[d.1 + 5..d.1 + 5 + name.len()].copy_from_slice(name);
            // GENERIC_WRITE, share read, CREATE_ALWAYS
            CreateFileA(
                path.as_ptr(),
                0x4000_0000,
                1,
                core::ptr::null_mut(),
                2,
                0x80,
                core::ptr::null_mut(),
            )
        };
        let f = open(b"pic.raw\0");
        if f as isize != -1 {
            let mut w = 0;
            WriteFile(
                f,
                (*addr_of_mut!(PICTURE)).as_ptr().cast(),
                640 * 480 * 2,
                &mut w,
                core::ptr::null_mut(),
            );
            CloseHandle(f);
        }
        let f = open(b"layers.txt\0");
        if f as isize == -1 {
            return;
        }
        let put = |line: &Line| {
            let mut w = 0;
            WriteFile(
                f,
                line.buf.as_ptr(),
                line.len as u32,
                &mut w,
                core::ptr::null_mut(),
            );
        };
        let draws = &*addr_of_mut!(FE_DRAWS);
        let mut head = Line::default();
        head.text(b"composed ")
            .num(u32::from(composed))
            .text(b" draws ")
            .num(draws.1 as u32)
            .text(b" full ")
            .num(u32::from(draws.2))
            .text(b"\r\n");
        put(&head);
        for dr in &draws.0[..draws.1] {
            let mut l = Line::default();
            let r = |l: &mut Line, x: [i32; 4]| {
                for v in x {
                    l.num(v.max(0) as u32).text(b" ");
                }
            };
            l.text(if dr.dst_surface == back as usize {
                b"back "
            } else {
                b"other "
            });
            l.text(b"dst ");
            r(&mut l, dr.draw.dst);
            l.text(b"src ");
            r(&mut l, dr.src);
            l.text(b"surf ").num(dr.surface as u32);
            l.text(b" twin ")
                .num(u32::from(dr.draw.twin))
                .text(b" key ")
                .num(dr.key.map_or(99999, |k| k[0]));
            if let Some(p) = cached_pixels(dr.surface, false) {
                l.text(b" img ").num(p.w as u32).text(b"x").num(p.h as u32);
            } else {
                l.text(b" img none");
            }
            l.text(b"\r\n");
            put(&l);
        }
        CloseHandle(f);
    }
}

/// This frame's layers for `fe_compose`, and its owner maps.
static mut FE_LAYERS: ([crate::fe_compose::Layer<'static>; 1024], usize) =
    ([crate::fe_compose::Layer::EMPTY; 1024], 0);
static mut FE_TOP: [u16; 640 * 480] = [0; 640 * 480];
static mut FE_UNDER: [u16; 640 * 480] = [0; 640 * 480];

/// Builds the frame's layers from its noted blits into `back` and their
/// owners (`fe_compose`); true when some picture pixel comes from a twin.
unsafe fn compose_fe(back: *mut usize) -> bool {
    // SAFETY: the main thread's own tables; the game's live surfaces.
    unsafe {
        let draws = &*addr_of_mut!(FE_DRAWS);
        if draws.2 {
            return false;
        }
        // Without a twin in the frame nothing is composed: don't lock the
        // images at all. Reading every source surface each frame crashed
        // inside dgVoodoo2's DirectDraw on native Windows (HD art off, any
        // race resolution), and the copies would go unused anyway.
        if !draws.0[..draws.1]
            .iter()
            .any(|d| d.dst_surface == back as usize && d.draw.twin)
        {
            return false;
        }
        let (layers, n) = &mut *addr_of_mut!(FE_LAYERS);
        *n = 0;
        let mut twins = false;
        for d in draws.0[..draws.1]
            .iter()
            .filter(|d| d.dst_surface == back as usize)
        {
            // Copies made when the blit was noted only: by now the game may
            // have released the surface (a stale one crashed here on native
            // Windows with dgVoodoo2).
            let Some(image) = cached_pixels(d.surface, false) else {
                // An image that can't be read: its pixels stay unexplained.
                continue;
            };
            let twin = if d.draw.twin {
                cached_pixels(d.twin_surface, false).map(|p| (p, d.twin_src))
            } else {
                None
            };
            twins |= twin.is_some();
            layers[*n] = crate::fe_compose::Layer::new(
                d.draw.dst,
                d.src,
                image,
                twin,
                d.key.map(|k| k[0] as u16),
            );
            *n += 1;
        }
        if !twins {
            return false;
        }
        let (top, under) = (&mut *addr_of_mut!(FE_TOP), &mut *addr_of_mut!(FE_UNDER));
        crate::fe_compose::owners(&layers[..*n], &*addr_of_mut!(PICTURE), top, under);
        let any = crate::fe_compose::any_twin(&layers[..*n], top);
        if let Some(dg) = diag() {
            dg.fe_draws += *n as u32;
            dg.fe_shown += u32::from(any);
        }
        any
    }
}

/// A copy of a 16-bit surface's pixels, kept until images load again.
#[derive(Clone, Copy)]
struct Cached {
    surface: usize,
    px: *mut u16,
    w: i32,
    h: i32,
}

static mut PIXELS: ([Cached; 1024], usize) = (
    [Cached {
        surface: 0,
        px: core::ptr::null_mut(),
        w: 0,
        h: 0,
    }; 1024],
    0,
);

/// The pixels of a game surface (16-bit), read once; with `may_lock` false
/// only an earlier copy (the surface may be gone).
unsafe fn cached_pixels(
    surface: usize,
    may_lock: bool,
) -> Option<crate::fe_compose::Pixels<'static>> {
    // SAFETY: the main thread's own table; a live surface locked for
    // reading and unlocked again; heap blocks the table owns.
    unsafe {
        let cache = &mut *addr_of_mut!(PIXELS);
        let view = |c: &Cached| crate::fe_compose::Pixels {
            px: core::slice::from_raw_parts(c.px, (c.w * c.h) as usize),
            w: c.w,
            h: c.h,
            stride: c.w as usize,
        };
        if let Some(c) = cache.0[..cache.1].iter().find(|c| c.surface == surface) {
            return Some(view(c));
        }
        if !may_lock || surface == 0 || cache.1 == cache.0.len() {
            return None;
        }
        let s = surface as *mut usize;
        let desc = lock(s, DDLOCK_READONLY)?;
        let (w, h) = (desc.width as i32, desc.height as i32);
        let px = if desc.bits == 16 && w > 0 && h > 0 {
            HeapAlloc(GetProcessHeap(), 0, (w * h * 2) as usize) as *mut u16
        } else {
            core::ptr::null_mut()
        };
        if !px.is_null() {
            for y in 0..h as usize {
                core::ptr::copy_nonoverlapping(
                    desc.surface.add(y * desc.pitch as usize) as *const u16,
                    px.add(y * w as usize),
                    w as usize,
                );
            }
        }
        unlock(s);
        if px.is_null() {
            return None;
        }
        let c = Cached { surface, px, w, h };
        cache.0[cache.1] = c;
        cache.1 += 1;
        Some(view(&c))
    }
}

/// Forgets the cached pixels (images are being loaded: surfaces change).
fn forget_pixels() {
    // SAFETY: the main thread's own table and its heap blocks.
    unsafe {
        let cache = &mut *addr_of_mut!(PIXELS);
        for c in &cache.0[..cache.1] {
            HeapFree(GetProcessHeap(), 0, c.px.cast());
        }
        cache.1 = 0;
    }
}

/// Clears the frame's noted blits.
fn forget_fe_draws() {
    // SAFETY: the main thread's own table.
    unsafe {
        let draws = &mut *addr_of_mut!(FE_DRAWS);
        draws.1 = 0;
        draws.2 = false;
    }
}

/// The front end's 640x480 picture while it is enlarged (`cmr_fe_present`).
static mut PICTURE: [u16; 640 * 480] = [0; 640 * 480];

fn desc_of(surface: *mut usize) -> Option<SurfaceDesc> {
    // SAFETY: a live DirectDraw surface; the descriptor is sized for it.
    unsafe {
        let mut desc: SurfaceDesc = core::mem::zeroed();
        desc.size = core::mem::size_of::<SurfaceDesc>() as u32;
        let get: GetSurfaceDesc = method(surface, GET_SURFACE_DESC);
        (get(surface, &mut desc) == 0).then_some(desc)
    }
}

static mut EASER: Easer = Easer::new();

/// Where the race projection reads the far plane (`0x435ffe`/`0x436018`,
/// F20): the far plane to use, eased towards the game's `target` (Q16).
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_ease_far(target: i32) -> i32 {
    let mut freq = 0;
    // SAFETY: a plain Win32 call; only the main thread renders.
    unsafe { QueryPerformanceFrequency(&mut freq) };
    let easer = unsafe { &mut *addr_of_mut!(EASER) };
    easer.step(target, now(), freq)
}

/// The frame-rate cap, vsync choice and rumble strength, filled by the
/// launcher (F17).
#[repr(C)]
pub struct PresentSettings {
    /// Frames per second, 0 for no cap.
    fps_limit: u32,
    /// `Flip` flags (`DDFLIP_WAIT`, `DDFLIP_NOVSYNC`), 0 for the game's own.
    flip_flags: u32,
    /// Rumble strength in percent, 0 for none (F22).
    rumble: u32,
}

/// XINPUT_STATE: packet number, then XINPUT_GAMEPAD (buttons, the two
/// triggers, four stick axes).
#[repr(C)]
#[derive(Default)]
struct PadState {
    packet: u32,
    buttons: u16,
    left_trigger: u8,
    right_trigger: u8,
    sticks: [i16; 4],
}

type XInputGetState = unsafe extern "system" fn(u32, *mut PadState) -> u32;
type XInputSetState = unsafe extern "system" fn(u32, *mut [u16; 2]) -> u32;

/// XInput, loaded on first use: `None` until tried, then the two calls (or
/// none when the system has no XInput).
static mut XINPUT: Option<Option<(XInputGetState, XInputSetState)>> = None;

fn xinput() -> Option<(XInputGetState, XInputSetState)> {
    // SAFETY: plain Win32 calls; the game's update and render threads only
    // ever race to load the same library.
    unsafe {
        let slot = &mut *addr_of_mut!(XINPUT);
        if slot.is_none() {
            let mut found = None;
            for name in [c"xinput1_4.dll", c"xinput1_3.dll", c"xinput9_1_0.dll"] {
                let module = LoadLibraryA(name.as_ptr().cast());
                if module.is_null() {
                    continue;
                }
                let get = GetProcAddress(module, c"XInputGetState".as_ptr().cast());
                let set = GetProcAddress(module, c"XInputSetState".as_ptr().cast());
                if !get.is_null() && !set.is_null() {
                    found = Some((
                        core::mem::transmute::<*mut u8, XInputGetState>(get),
                        core::mem::transmute::<*mut u8, XInputSetState>(set),
                    ));
                    break;
                }
            }
            *slot = Some(found);
        }
        (*slot).flatten()
    }
}

/// Where the game reads a pad's buttons (`0x49cef6`, F23): returns the
/// buttons remapped by the player's choice (`map`, 12 bytes, see
/// `pad::remap`) and adds the d-pad's directions to the record's flags.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_pad_buttons(info: *const u32, record: *mut u32, map: *const u8) -> u32 {
    // SAFETY: the game's JOYINFOEX (buttons at +0x20, POV at +0x28), its
    // device record (flags at +4) and the launcher's 12-byte map.
    unsafe {
        let buttons = *info.add(8);
        if record.is_null() || map.is_null() {
            return buttons;
        }
        *record.add(1) |= crate::pad::pov_directions(*info.add(10));
        crate::pad::remap(buttons, &*(map as *const [u8; crate::pad::GAME_BUTTONS]))
    }
}

/// After the game's control map (`0x449580`, F21): the triggers of the
/// player's XInput pad press throttle (right) and brake (left) on their own,
/// so both can be held; other inputs are kept when they press harder.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_pedals(block: *mut u8, player: u32) {
    if block.is_null() || player > 3 {
        return;
    }
    if player == 0 {
        let mut freq = 0;
        // SAFETY: the game's live car block; a plain Win32 call. The feel
        // is written here (update thread) and read each frame (main
        // thread): a torn reading of a few floats is harmless.
        unsafe {
            QueryPerformanceFrequency(&mut freq);
            (*addr_of_mut!(FEEL)).update(crate::feel::Car::read(block), now(), freq);
        }
    }
    let Some((get, _)) = xinput() else { return };
    let mut pad = PadState::default();
    // SAFETY: XInput fills the state it is given.
    let read = unsafe { get(player, &mut pad) } == 0;
    if player == 0 {
        // SAFETY: the game's live car block (see `car_dump`).
        unsafe { car_dump(block, pad.left_trigger, pad.right_trigger) };
    }
    if !read {
        return;
    }
    // SAFETY: the player's car control block: throttle +0x304 and brake
    // +0x308, their analog flags +0x414 / +0x418.
    unsafe {
        for (trigger, value, flag) in [
            (pad.right_trigger, 0x304, 0x414),
            (pad.left_trigger, 0x308, 0x418),
        ] {
            let v = crate::xinput::pedal(trigger);
            let cur = block.add(value) as *mut u32;
            if v > *cur {
                *cur = v;
                *(block.add(flag) as *mut u32) = 1;
            }
        }
    }
}

/// A research aid (`CMR_CAR_DUMP=<file>`): twenty times a second, 16 KiB of
/// the game's memory around player 1's car block (from 8 KiB before it;
/// the cars are in the executable's own data) appended to the file after
/// a header of the milliseconds since the first record, the block's
/// address and the pad's two triggers. Off unless the variable is set.
static mut CAR_DUMP: (Option<*mut u8>, i64) = (None, 0);

unsafe fn car_dump(block: *mut u8, left: u8, right: u8) {
    // SAFETY: plain Win32 calls; the block is the game's (4 KiB of car state).
    unsafe {
        let d = &mut *addr_of_mut!(CAR_DUMP);
        if d.0.is_none() {
            let mut path = [0u8; 260];
            let n =
                GetEnvironmentVariableA(c"CMR_CAR_DUMP".as_ptr().cast(), path.as_mut_ptr(), 259);
            let f = if n == 0 || n >= 259 {
                core::ptr::null_mut()
            } else {
                // GENERIC_WRITE, share read, CREATE_ALWAYS
                let f = CreateFileA(
                    path.as_ptr(),
                    0x4000_0000,
                    1,
                    core::ptr::null_mut(),
                    2,
                    0x80,
                    core::ptr::null_mut(),
                );
                if f as isize == -1 {
                    core::ptr::null_mut()
                } else {
                    f
                }
            };
            d.0 = Some(f);
        }
        let Some(f) = d.0.filter(|f| !f.is_null()) else {
            return;
        };
        let (mut t, mut freq) = (0i64, 0i64);
        QueryPerformanceCounter(&mut t);
        QueryPerformanceFrequency(&mut freq);
        if d.1 == 0 {
            d.1 = t - freq;
        }
        static mut LAST: i64 = 0;
        if t - LAST < freq / 20 {
            return;
        }
        LAST = t;
        // Only inside the executable's image (0x400000..0xd95000).
        let from = block as usize - 0x2000;
        if from < 0x0040_0000 || from + 0x4000 > 0x00d9_5000 {
            return;
        }
        let head = [
            ((t - d.1) * 1000 / freq.max(1)) as u32,
            block as u32,
            u32::from(left) | (u32::from(right) << 8),
        ];
        let mut w = 0;
        WriteFile(f, head.as_ptr().cast(), 12, &mut w, core::ptr::null_mut());
        WriteFile(f, from as *const u8, 0x4000, &mut w, core::ptr::null_mut());
    }
}

static mut RUMBLE: crate::xinput::Rumble = crate::xinput::Rumble::new();
/// The motor speeds last sent, so XInput is only called on a change.
static mut MOTORS: (u16, u16) = (0, 0);

/// At an impact sound (`0x469e80`, F22): the pad rumbles with the impact's
/// `severity` (Q16, 0..=1.0).
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_rumble(severity: i32, settings: *const PresentSettings) {
    if settings.is_null() {
        return;
    }
    // SAFETY: the launcher's settings block; the rumble state is the main
    // and update threads' only shared write, a few floats.
    unsafe {
        let strength = (*settings).rumble & 0xff;
        if let Some(d) = diag() {
            d.impacts += 1;
            d.severity_max = d.severity_max.max(severity);
        }
        if strength > 0 {
            (*addr_of_mut!(RUMBLE)).impact(severity, strength, now());
        }
    }
}

/// The HD art folder (`CMR_HD_ART`, set by the launcher to the 2x or 4x set
/// for the resolution), read once; length 0 when there is none.
static mut HD_DIR: ([u8; 300], usize, bool) = ([0; 300], 0, false);

/// Pairs of D3D texture handles: a game texture and its HD twin. Emptied
/// outside races (`cmr_present`), so a reused handle never maps to a freed
/// texture.
static mut HD_PAIRS: ([HdPair; 512], usize) = ([HdPair::NONE; 512], 0);

/// A game texture record and its HD twin's (each with the D3D handle at
/// +0x10 it had when paired; checked at each bind, so a record the game
/// freed and reused never swaps in the wrong picture).
#[derive(Clone, Copy)]
struct HdPair {
    game: usize,
    twin: usize,
    game_handle: u32,
    twin_handle: u32,
}

impl HdPair {
    const NONE: HdPair = HdPair {
        game: 0,
        twin: 0,
        game_handle: 0,
        twin_handle: 0,
    };

    /// Both records still hold the handles they were paired with.
    unsafe fn live(&self) -> bool {
        // SAFETY: the game's texture records (+0x10 the handle).
        unsafe {
            *((self.game + 0x10) as *const u32) == self.game_handle
                && *((self.twin + 0x10) as *const u32) == self.twin_handle
        }
    }
}

/// The game's bitmap loader (`0x499ec0(bmp, flags)`), reached through
/// F27's trampoline.
type LoadBitmap = unsafe extern "stdcall" fn(*const u8, u32) -> *mut u8;

/// The HD version of a bitmap from the HD folder (see `hd`), in a heap
/// block the caller frees, or null.
unsafe fn hd_bitmap(bmp: *const u8) -> *mut u8 {
    // SAFETY: the game's BMP in memory (its header says how long it is);
    // plain Win32 file and heap calls; loading runs on the main thread.
    unsafe {
        let dir = &mut *addr_of_mut!(HD_DIR);
        if !dir.2 {
            dir.2 = true;
            let n = GetEnvironmentVariableA(c"CMR_HD_ART".as_ptr().cast(), dir.0.as_mut_ptr(), 260);
            dir.1 = if n > 0 && n < 260 { n as usize } else { 0 };
        }
        if dir.1 == 0 {
            return core::ptr::null_mut();
        }
        let Some(len) = crate::hd::bmp_len(&*(bmp as *const [u8; 6])) else {
            return core::ptr::null_mut();
        };
        let name = crate::hd::name(core::slice::from_raw_parts(bmp, len));
        let mut path = [0u8; 300];
        let n = dir.1;
        path[..n].copy_from_slice(&dir.0[..n]);
        path[n] = b'\\';
        path[n + 1..n + 17].copy_from_slice(&name);
        path[n + 17..n + 22].copy_from_slice(b".bmp\0");
        let heap = GetProcessHeap();
        let (mut out, mut size) = read_bmp_file(path.as_ptr());
        if out.is_null() {
            return out;
        }
        // Larger than a texture may be (`hd::too_large`): the same picture
        // from the x2 set, when this is the x4 one.
        if crate::hd::too_large(core::slice::from_raw_parts(out, size as usize))
            && n >= 2
            && dir.0[n - 2..n].eq_ignore_ascii_case(b"x4")
        {
            path[n - 1] = b'2';
            let (smaller, len) = read_bmp_file(path.as_ptr());
            if !smaller.is_null() {
                HeapFree(heap, 0, out);
                (out, size) = (smaller, len);
            }
        }
        // Rows the game would read sheared and past the end: lay them out
        // its way (`hd::game_layout_len`).
        let file = core::slice::from_raw_parts(out as *const u8, size as usize);
        if let Some(len) = crate::hd::game_layout_len(file) {
            let fixed = HeapAlloc(heap, 0, len);
            let ok = !fixed.is_null()
                && crate::hd::to_game_layout(file, core::slice::from_raw_parts_mut(fixed, len));
            HeapFree(heap, 0, out);
            if !ok {
                if !fixed.is_null() {
                    HeapFree(heap, 0, fixed);
                }
                return core::ptr::null_mut();
            }
            out = fixed;
            size = len as u32;
        }
        // A defined palette size for RGB images (`hd::define_palette_count`).
        crate::hd::define_palette_count(core::slice::from_raw_parts_mut(out, size as usize));
        out
    }
}

/// Scratch space for `hd::to_8bit` (loading runs on the main thread).
static mut QUANT_HIST: [u32; 32768] = [0; 32768];
static mut QUANT_LUT: [u8; 32768] = [0; 32768];

/// A whole BMP file (`path` NUL-terminated) on the process heap, and its
/// size; null when it can't be read or isn't a BMP of a sane size.
unsafe fn read_bmp_file(path: *const u8) -> (*mut u8, u32) {
    // SAFETY: plain Win32 file and heap calls.
    unsafe {
        // GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING
        let f = CreateFileA(
            path,
            0x8000_0000,
            1,
            core::ptr::null_mut(),
            3,
            0x80,
            core::ptr::null_mut(),
        );
        if f as isize == -1 {
            return (core::ptr::null_mut(), 0);
        }
        let heap = GetProcessHeap();
        let size = GetFileSize(f, core::ptr::null_mut());
        let mut out = core::ptr::null_mut();
        if (54..=64 << 20).contains(&size) {
            let buf = HeapAlloc(heap, 0, size as usize);
            let mut read = 0;
            if !buf.is_null()
                && ReadFile(f, buf, size, &mut read, core::ptr::null_mut()) != 0
                && read == size
                && crate::hd::bmp_len(&*(buf as *const [u8; 6])) == Some(size as usize)
            {
                out = buf;
            } else if !buf.is_null() {
                HeapFree(heap, 0, buf);
            }
        }
        CloseHandle(f);
        (out, size)
    }
}

/// Where the game notes that the device has palettized textures:
/// `[[0x50283c]+0x78]+0x6c`, non-zero when it does (the texture loader
/// `0x499ef0` then makes 8-bit images palettized textures and copies their
/// palette, `0x49a153`). Null when the game hasn't set its device up.
unsafe fn palette_textures() -> *mut usize {
    // SAFETY: the game's 3D settings and its device description.
    unsafe {
        let settings = *(0x0050_283c as *const *const u8);
        if settings.is_null() {
            return core::ptr::null_mut();
        }
        let device = *(settings.add(0x78) as *const *mut u8);
        if device.is_null() {
            return core::ptr::null_mut();
        }
        device.add(0x6c) as *mut usize
    }
}

/// Instead of the game's bitmap loader `0x499ec0` (F27): the game's own
/// load, and for textures (flags bit 1; the race HUD among them) with an
/// HD version, a second load of that version, its twin. The game keeps its
/// own textures and sizes; only binding (`hd_set_render_state`) swaps in
/// the twin, whose coordinates are the same fractions of the picture.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hd_load(load: LoadBitmap, bmp: *const u8, flags: u32) -> *mut u8 {
    // SAFETY: the game's loader and its texture records (+0x10 the handle
    // of the texture it draws with); loading runs on the main thread.
    unsafe {
        let game = load(bmp, flags);
        if !game.is_null() && !bmp.is_null() {
            trace_name(game, bmp);
        }
        if game.is_null() || bmp.is_null() || flags & 1 == 0 {
            return game;
        }
        let hd = hd_bitmap(bmp);
        if hd.is_null() {
            return game;
        }
        // An RGB twin on a device with palettized textures (dgVoodoo2 on
        // native Windows; Wine has none): the game turns 8-bit images into
        // palettized textures and has no working path for an RGB one there.
        // It crashed copying an unset palette size, and with that defined
        // (`hd::define_palette_count`) or the device's palettes hidden for
        // the load the twin still drew black (the skies). Such a twin is
        // made an 8-bit image with its own palette (`hd::to_8bit`), the
        // kind the game makes working textures of there, like the HUD's.
        let bits = *(hd.add(28) as *const u16);
        let palettes = palette_textures();
        let mut hd = hd;
        if bits > 8 && !palettes.is_null() && *palettes != 0 {
            let heap = GetProcessHeap();
            let Some(file_len) = crate::hd::bmp_len(&*(hd as *const [u8; 6])) else {
                HeapFree(heap, 0, hd);
                return game;
            };
            let file = core::slice::from_raw_parts(hd as *const u8, file_len);
            let eight = crate::hd::to_8bit_len(file).map_or(core::ptr::null_mut(), |len| {
                let out = HeapAlloc(heap, 0, len);
                if !out.is_null()
                    && !crate::hd::to_8bit(
                        file,
                        core::slice::from_raw_parts_mut(out, len),
                        &mut *addr_of_mut!(QUANT_HIST),
                        &mut *addr_of_mut!(QUANT_LUT),
                    )
                {
                    HeapFree(heap, 0, out);
                    return core::ptr::null_mut();
                }
                out
            });
            HeapFree(heap, 0, hd);
            if eight.is_null() {
                return game;
            }
            hd = eight;
        }
        let twin = load(hd, flags);
        HeapFree(GetProcessHeap(), 0, hd);
        let pairs = &mut *addr_of_mut!(HD_PAIRS);
        if !twin.is_null() && pairs.1 < pairs.0.len() {
            let handle = |rec: *mut u8| *(rec.add(0x10) as *const u32);
            pairs.0[pairs.1] = HdPair {
                game: game as usize,
                twin: twin as usize,
                game_handle: handle(game),
                twin_handle: handle(twin),
            };
            pairs.1 += 1;
            hook_texture_binds();
        }
        game
    }
}

/// The device's `SetRenderState` (slot 23 of the `IDirect3DDevice2` the
/// game holds at `[[0x50283c]+0x10]`), before F27 took the slot.
type SetRenderState = unsafe extern "stdcall" fn(*mut usize, u32, u32) -> i32;
static mut SET_RENDER_STATE: Option<SetRenderState> = None;
const SET_RENDER_STATE_SLOT: usize = 23;
const TEXTUREHANDLE: u32 = 1;

/// The game binds textures from several places, all through the device's
/// `SetRenderState(TEXTUREHANDLE, handle)`; that one method of the device
/// is taken over once, so every bind of a texture with a twin uses the twin.
unsafe fn hook_texture_binds() {
    // SAFETY: the game's 3D settings and its live D3D device; the method
    // table is made writable for the one entry and restored.
    unsafe {
        if (*addr_of_mut!(SET_RENDER_STATE)).is_some() {
            return;
        }
        let settings = *(0x0050_283c as *const *const u8);
        let device = if settings.is_null() {
            core::ptr::null_mut()
        } else {
            *(settings.add(0x10) as *const *mut usize)
        };
        if device.is_null() {
            return;
        }
        let entry = (*device as *mut usize).add(SET_RENDER_STATE_SLOT);
        let mut old = 0;
        if VirtualProtect(entry.cast(), 4, 0x04, &mut old) == 0 {
            return;
        }
        *addr_of_mut!(SET_RENDER_STATE) =
            Some(core::mem::transmute::<usize, SetRenderState>(*entry));
        *entry = hd_set_render_state as *const () as usize;
        VirtualProtect(entry.cast(), 4, old, &mut old);
    }
}

/// `D3DRENDERSTATE_TEXTUREMAG` and `TEXTUREMIN`, and `D3DFILTER_LINEAR`.
const TEXTUREMAG: u32 = 17;
const TEXTUREMIN: u32 = 18;
const FILTER_LINEAR: u32 = 2;

/// Whether a twin is bound, and the filters the game last asked for
/// (`TEXTUREMAG`, `TEXTUREMIN`; 0 before it asked).
static mut HD_FILTER: (bool, [u32; 2]) = (false, [0; 2]);

/// Every bind of a texture with a twin binds the twin, sampled smoothly:
/// the game draws its 2D with sharp pixels, which at the twin's finer grid
/// would still step along diagonals; filtered, its edges come out as
/// straight lines. The game's own filters come back with its next texture.
unsafe extern "stdcall" fn hd_set_render_state(device: *mut usize, state: u32, value: u32) -> i32 {
    // SAFETY: the original method saved by `hook_texture_binds`; the main
    // thread's own tables.
    unsafe {
        let Some(original) = *addr_of_mut!(SET_RENDER_STATE) else {
            return 0;
        };
        let filter = &mut *addr_of_mut!(HD_FILTER);
        match state {
            TEXTUREHANDLE => {
                let pairs = &*addr_of_mut!(HD_PAIRS);
                let mut value = value;
                let mut twin = false;
                if let Some(p) = pairs.0[..pairs.1].iter().find(|p| p.game_handle == value) {
                    if p.live() {
                        value = p.twin_handle;
                        twin = true;
                        if let Some(d) = diag() {
                            d.hd_binds += 1;
                        }
                    } else if let Some(d) = diag() {
                        d.hd_stale += 1;
                    }
                }
                if twin != filter.0 {
                    filter.0 = twin;
                    for (state, game) in [(TEXTUREMAG, filter.1[0]), (TEXTUREMIN, filter.1[1])] {
                        let want = if twin { FILTER_LINEAR } else { game };
                        if want != 0 {
                            original(device, state, want);
                        }
                    }
                }
                original(device, state, value)
            }
            TEXTUREMAG | TEXTUREMIN => {
                filter.1[(state - TEXTUREMAG) as usize] = value;
                original(device, state, if filter.0 { FILTER_LINEAR } else { value })
            }
            _ => original(device, state, value),
        }
    }
}

/// Outside races the textures are gone: forget the pairs.
fn forget_hd_pairs() {
    // SAFETY: the main thread's own table.
    unsafe {
        let pairs = &mut *addr_of_mut!(HD_PAIRS);
        if pairs.1 != 0 && !(4..=0xb).contains(&Game.state()) {
            pairs.1 = 0;
        }
        // The device may not exist yet when the textures load.
        if pairs.1 != 0 {
            hook_texture_binds();
        }
    }
}

/// Front-end images with an HD twin (F27): the game's 2D record and its
/// twin's, each with the surface it held when paired (checked at each use,
/// so a record the game reused never draws the wrong picture).
#[derive(Clone, Copy)]
struct FeTwin {
    game: usize,
    twin: usize,
    game_surface: usize,
    twin_surface: usize,
}

impl FeTwin {
    const NONE: FeTwin = FeTwin {
        game: 0,
        twin: 0,
        game_surface: 0,
        twin_surface: 0,
    };

    /// The twin of the record `game`, if both still hold their surfaces.
    unsafe fn of(game: usize) -> Option<FeTwin> {
        // SAFETY: the main thread's own table; records start with their
        // surface.
        unsafe {
            let twins = &*addr_of_mut!(FE_TWINS);
            twins.0[..twins.1].iter().copied().find(|t| {
                t.game == game
                    && *(t.game as *const usize) == t.game_surface
                    && *(t.twin as *const usize) == t.twin_surface
            })
        }
    }
}

static mut FE_TWINS: ([FeTwin; 512], usize) = ([FeTwin::NONE; 512], 0);

/// The game's 2D surface loader (`0x499320(record, bmp, flags)`), reached
/// through F27's trampoline.
type LoadSurface = unsafe extern "stdcall" fn(*mut u8, *const u8, u32) -> *mut u8;

/// Instead of the 2D surface loader `0x499320` (F27): the game's own load,
/// and for an image with an HD version a second load of that version, its
/// twin. Flags bit 1 makes a new surface (the front end's images); without
/// it the pixels go into an existing record (textures, reloads), which has
/// a twin only if it had one before, reloaded too.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_hd_load_2d(
    load: LoadSurface,
    record: *mut u8,
    bmp: *const u8,
    flags: u32,
) -> *mut u8 {
    // SAFETY: the game's loader and its records (+0 the surface); loading
    // runs on the main thread.
    unsafe {
        forget_pixels();
        let game = load(record, bmp, flags);
        if game.is_null() || bmp.is_null() {
            return game;
        }
        trace_name(game, bmp);
        let twins = &mut *addr_of_mut!(FE_TWINS);
        let known = twins.0[..twins.1]
            .iter()
            .position(|t| t.game == game as usize);
        if flags & 1 == 0 && known.is_none() {
            return game;
        }
        let hd = hd_bitmap(bmp);
        let twin = if hd.is_null() {
            core::ptr::null_mut()
        } else {
            let into = match known {
                Some(i) if flags & 1 == 0 => twins.0[i].twin as *mut u8,
                _ => core::ptr::null_mut(),
            };
            let twin = load(into, hd, flags | u32::from(into.is_null()));
            HeapFree(GetProcessHeap(), 0, hd);
            twin
        };
        let entry = FeTwin {
            game: game as usize,
            twin: twin as usize,
            game_surface: *(game as *const usize),
            twin_surface: if twin.is_null() {
                0
            } else {
                *(twin as *const usize)
            },
        };
        match known {
            Some(i) if twin.is_null() => {
                twins.1 -= 1;
                twins.0[i] = twins.0[twins.1];
            }
            Some(i) => twins.0[i] = entry,
            None if !twin.is_null() && twins.1 < twins.0.len() => {
                twins.0[twins.1] = entry;
                twins.1 += 1;
            }
            None => {}
        }
        game
    }
}

/// A blit of the frame (`cmr_hud_rect_at`): its destination surface and
/// rectangle, and for an image with a twin what to draw from the twin.
#[derive(Clone, Copy)]
struct FeDraw {
    dst_surface: usize,
    draw: crate::fe_hd::Draw,
    /// The source in the image's pixels, and the image's surface.
    src: crate::fe_hd::Rect,
    surface: usize,
    /// The source in the twin's pixels, and the twin's surface.
    twin_src: crate::fe_hd::Rect,
    twin_surface: usize,
    key: Option<[u32; 2]>,
}

/// This frame's blits, oldest first; `full` when some were not noted (then
/// no twin is drawn, as one might cover a later blit).
static mut FE_DRAWS: ([FeDraw; 1024], usize, bool) = (
    [FeDraw {
        dst_surface: 0,
        draw: crate::fe_hd::Draw {
            dst: [0; 4],
            twin: false,
        },
        src: [0; 4],
        surface: 0,
        twin_src: [0; 4],
        twin_surface: 0,
        key: None,
    }; 1024],
    0,
    false,
);

/// Notes a front-end blit (`0x49e3e0(src, src_rect, dst, dst_rect,
/// key_low, key_high, keyed)`; records start with their surface and hold
/// their width and height at +8; rectangles are four shorts).
unsafe fn note_fe_blit(args: *const usize) {
    // SAFETY: the blit's own arguments, at its entry.
    unsafe {
        let draws = &mut *addr_of_mut!(FE_DRAWS);
        if draws.1 == draws.0.len() {
            draws.2 = true;
            return;
        }
        let (src_rec, src_rect, dst_rec, dst_rect) = (
            *args as *const usize,
            *args.add(1) as *const i16,
            *args.add(2) as *const usize,
            *args.add(3) as *const i16,
        );
        if src_rec.is_null() || dst_rec.is_null() || src_rect.is_null() || dst_rect.is_null() {
            return;
        }
        let rect = |r: *const i16| core::array::from_fn(|i| i32::from(*r.add(i)));
        let dst = rect(dst_rect);
        trace_draw(b"fe", src_rec as usize, rect(src_rect), dst);
        let mut entry = FeDraw {
            dst_surface: *dst_rec,
            draw: crate::fe_hd::Draw { dst, twin: false },
            src: rect(src_rect),
            surface: *src_rec,
            twin_src: [0; 4],
            twin_surface: 0,
            key: (*args.add(6) != 0).then(|| [*args.add(4) as u32, *args.add(5) as u32]),
        };
        if let Some(t) = FeTwin::of(src_rec as usize) {
            let size = |rec: usize| {
                let r = rec as *const i16;
                (i32::from(*r.add(4)), i32::from(*r.add(5)))
            };
            entry.twin_src = crate::fe_hd::scale(entry.src, size(t.game), size(t.twin));
            entry.twin_surface = t.twin_surface;
            entry.draw.twin = true;
        }
        // With HD twins about, copy the images' pixels now, while the game
        // is drawing from them and they surely exist (`compose_fe` reads
        // only these copies at the end of the frame).
        if (*addr_of_mut!(FE_TWINS)).1 != 0 {
            let _ = cached_pixels(entry.surface, true);
            if entry.draw.twin {
                let _ = cached_pixels(entry.twin_surface, true);
            }
        }
        draws.0[draws.1] = entry;
        draws.1 += 1;
    }
}

/// The draw trace (`CMR_HD_TRACE=<file>`, a tool for making HD art,
/// docs/UPSCALING.md): each distinct HUD sprite and front-end blit, once,
/// as `hud|fe <runtime name> src x y w h dst x y w h`, so the screens can
/// be rebuilt from their pieces. Off unless the variable is set.
struct Trace {
    /// None until checked; Some(null) when off.
    file: Option<*mut u8>,
    /// Images by record: the record and its runtime name.
    names: [(usize, [u8; 16]); 2048],
    count: usize,
    /// Hashes of the lines written (open addressing; 0 is empty).
    seen: [u64; 8192],
}

static mut TRACE: Trace = Trace {
    file: None,
    names: [(0, [0; 16]); 2048],
    count: 0,
    seen: [0; 8192],
};

fn trace() -> Option<&'static mut Trace> {
    // SAFETY: the main thread's own state (loading and drawing).
    let t = unsafe { &mut *addr_of_mut!(TRACE) };
    if t.file.is_none() {
        let mut path = [0u8; 260];
        // SAFETY: plain Win32 calls with buffers of the stated sizes.
        let file = unsafe {
            let n =
                GetEnvironmentVariableA(c"CMR_HD_TRACE".as_ptr().cast(), path.as_mut_ptr(), 259);
            if n == 0 || n >= 259 {
                core::ptr::null_mut()
            } else {
                // GENERIC_WRITE, share read, CREATE_ALWAYS, normal file
                let f = CreateFileA(
                    path.as_ptr(),
                    0x4000_0000,
                    1,
                    core::ptr::null_mut(),
                    2,
                    0x80,
                    core::ptr::null_mut(),
                );
                if f as isize == -1 {
                    core::ptr::null_mut()
                } else {
                    f
                }
            }
        };
        t.file = Some(file);
    }
    (!t.file?.is_null()).then_some(t)
}

/// Notes the runtime name of a loaded image's record.
unsafe fn trace_name(record: *mut u8, bmp: *const u8) {
    let Some(t) = trace() else { return };
    // SAFETY: the game's BMP in memory; its header gives its length.
    unsafe {
        let Some(len) = crate::hd::bmp_len(&*(bmp as *const [u8; 6])) else {
            return;
        };
        let name = crate::hd::name(core::slice::from_raw_parts(bmp, len));
        let at = t.names[..t.count]
            .iter()
            .position(|n| n.0 == record as usize);
        match at {
            Some(i) => t.names[i].1 = name,
            None if t.count < t.names.len() => {
                t.names[t.count] = (record as usize, name);
                t.count += 1;
            }
            None => {}
        }
    }
}

/// Traces a sprite queue entry (`0x49b420(src, dst, texture, ...)`).
unsafe fn trace_queue(args: *const usize) {
    if trace().is_none() {
        return;
    }
    // SAFETY: as the caller says.
    unsafe {
        let (src, dst, texture) = (
            *args as *const i16,
            *args.add(1) as *const i16,
            *args.add(2),
        );
        if src.is_null() || dst.is_null() || texture == 0 {
            return;
        }
        let rect = |r: *const i16| core::array::from_fn(|i| i32::from(*r.add(i)));
        trace_draw(b"hud", texture, rect(src), rect(dst));
    }
}

/// Writes one trace line for a draw of `record`, once per distinct line.
fn trace_draw(kind: &[u8], record: usize, src: [i32; 4], dst: [i32; 4]) {
    let Some(t) = trace() else { return };
    let Some(&(_, name)) = t.names[..t.count].iter().find(|n| n.0 == record) else {
        return;
    };
    let mut line = Line::default();
    line.text(kind).text(b" ").text(&name).text(b" src");
    for v in src {
        line.text(b" ").int(v);
    }
    line.text(b" dst");
    for v in dst {
        line.text(b" ").int(v);
    }
    line.text(b"\r\n");
    // FNV-1a of the line, to write it once.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in &line.buf[..line.len] {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    let h = h.max(1);
    let mut i = (h as usize) % t.seen.len();
    for _ in 0..t.seen.len() {
        match t.seen[i] {
            0 => break,
            v if v == h => return,
            _ => i = (i + 1) % t.seen.len(),
        }
    }
    if t.seen[i] != 0 {
        return; // full
    }
    t.seen[i] = h;
    let mut written = 0;
    // SAFETY: an open file handle and a buffer of the stated length.
    unsafe {
        WriteFile(
            t.file.unwrap_or(core::ptr::null_mut()),
            line.buf.as_ptr(),
            line.len as u32,
            &mut written,
            core::ptr::null_mut(),
        );
    }
}

/// The game's two threads, its race updates (a multimedia timer, 25 a
/// second) and its drawing (the main thread), were written for one-core
/// PCs, where they never ran at the same moment: an update interrupted
/// the drawing and finished before it went on. On several cores they run
/// side by side, and the drawing can read the car and camera halfway
/// through an update (a frame with the camera somewhere else). Both are
/// kept on one core, as on the PCs the game was made for: nothing waits on
/// a lock and an update still runs when it is due, taking over the core
/// for its millisecond. Wine's renderer threads stay on the other cores.
/// Each thread is pinned once (a new timer thread on its first update).
fn share_core(pinned: *mut u32) {
    // SAFETY: plain Win32 calls; each thread only writes its own slot.
    unsafe {
        let me = GetCurrentThreadId();
        if *pinned == me {
            return;
        }
        *pinned = me;
        let (mut process, mut system) = (0usize, 0usize);
        if GetProcessAffinityMask(GetCurrentProcess(), &mut process, &mut system) == 0
            || process.count_ones() < 2
        {
            return;
        }
        // The highest core the process may use (core 0 takes most interrupts).
        let core = 1usize << (usize::BITS - 1 - process.leading_zeros());
        SetThreadAffinityMask(GetCurrentThread(), core);
    }
}

static mut UPDATE_THREAD: u32 = 0;
static mut DRAW_THREAD: u32 = 0;

/// The crash handler that was there before ours (the game's, or none).
static mut PREVIOUS_FILTER: Option<ExceptionFilter> = None;
static mut FILTER_SET: bool = false;

/// Motors off now (on a pause, outside races, when the game ends or
/// crashes). The launcher also turns them off after the game exits.
pub fn stop_rumble() {
    // SAFETY: the main thread's pad, if one was found; its motors stopped.
    unsafe {
        (*addr_of_mut!(FEEL)).reset();
        let pad = (*addr_of_mut!(WGI)).pad;
        if !pad.is_null() && *addr_of_mut!(WGI_SENT) != [0; 4] {
            let put: PutVibration = method(pad, IGAMEPAD_PUT_VIBRATION);
            put(pad, 0.0, 0.0, 0.0, 0.0);
            *addr_of_mut!(WGI_SENT) = [0; 4];
        }
    }
    let Some((_, set)) = xinput() else { return };
    // SAFETY: the rumble statics; a plain XInput call.
    unsafe {
        (*addr_of_mut!(RUMBLE)).stop();
        let sent = &mut *addr_of_mut!(MOTORS);
        if *sent != (0, 0) {
            let mut v = [0u16, 0];
            set(0, &mut v);
            *sent = (0, 0);
        }
    }
}

/// Installs the crash handler once (the first game update or frame).
fn install_crash_filter() {
    // SAFETY: main/update thread statics, set once; a plain Win32 call.
    unsafe {
        if !*addr_of_mut!(FILTER_SET) {
            *addr_of_mut!(FILTER_SET) = true;
            *addr_of_mut!(PREVIOUS_FILTER) = SetUnhandledExceptionFilter(Some(crash_filter));
        }
    }
}

/// Writes `crash.log` in the game's folder: the exception code, where it
/// happened (module + offset) and the main registers. The launcher copies
/// it into its log when the game has exited.
unsafe fn write_crash_log(info: *mut u8) {
    unsafe {
        let record = *(info as *const *const u32);
        let context = *(info.add(4) as *const *const u8);
        if record.is_null() || context.is_null() {
            return;
        }
        let code = *record;
        let address = *record.add(3);
        let mut line = Line::default();
        line.text(b"crash ").hex(code).text(b" at ").hex(address);
        let mut module = core::ptr::null_mut();
        // FROM_ADDRESS | UNCHANGED_REFCOUNT
        if GetModuleHandleExA(6, address as *const u8, &mut module) != 0 {
            let mut path = [0u8; 260];
            let n = GetModuleFileNameA(module, path.as_mut_ptr(), 260) as usize;
            let name = path[..n].rsplit(|&b| b == b'\\').next().unwrap_or(&[]);
            line.text(b" (")
                .text(name)
                .text(b"+")
                .hex(address.wrapping_sub(module as u32))
                .text(b")");
        }
        let reg = |off: usize| *(context.add(off) as *const u32);
        for (name, off) in [
            (&b" eax="[..], 0xb0),
            (b" ebx=", 0xa4),
            (b" ecx=", 0xac),
            (b" edx=", 0xa8),
            (b" esi=", 0xa0),
            (b" edi=", 0x9c),
            (b" ebp=", 0xb4),
            (b" esp=", 0xc4),
        ] {
            line.text(name).hex(reg(off));
        }
        line.text(b"\r\n");
        // GENERIC_WRITE, CREATE_ALWAYS
        let f = CreateFileA(
            c"crash.log".as_ptr().cast(),
            0x4000_0000,
            0,
            core::ptr::null_mut(),
            2,
            0x80,
            core::ptr::null_mut(),
        );
        if f as isize != -1 {
            let mut written = 0;
            WriteFile(
                f,
                line.buf.as_ptr(),
                line.len as u32,
                &mut written,
                core::ptr::null_mut(),
            );
            CloseHandle(f);
        }
    }
}

/// On a crash: the motors stop, `crash.log` is written, then the previous
/// handler (if any) runs as before.
unsafe extern "system" fn crash_filter(info: *mut u8) -> i32 {
    stop_rumble();
    // SAFETY: the system's exception pointers.
    unsafe { write_crash_log(info) };
    // SAFETY: the handler that was installed before ours.
    match unsafe { *addr_of_mut!(PREVIOUS_FILTER) } {
        Some(previous) => unsafe { previous(info) },
        None => 0, // EXCEPTION_CONTINUE_SEARCH
    }
}

/// Each frame: sends the impacts' fading rumble and the driving feel
/// (`feel`) to player 1's pad; off outside driving (menus, loading, the
/// pause menu). Through Windows.Gaming.Input, which also reaches the
/// trigger motors, when it has the pad; else XInput's two motors.
fn update_rumble(settings: &PresentSettings) {
    // The strength in the low byte; bits 8 and 9 switch the trigger and
    // road feel off (`feel::Choices`).
    let strength = settings.rumble & 0xff;
    if strength == 0 {
        return;
    }
    let choices = crate::feel::Choices::of_word(settings.rumble);
    let mut freq = 0;
    // SAFETY: plain Win32 calls and the main thread's own statics.
    unsafe {
        QueryPerformanceFrequency(&mut freq);
        let t = now();
        let rumble = &mut *addr_of_mut!(RUMBLE);
        let feel = &mut *addr_of_mut!(FEEL);
        // Driving states only: 5..=9 (state 0xa is the pause menu).
        let driving = (5..=9).contains(&Game.state());
        if !driving {
            rumble.stop();
            feel.reset();
        }
        let impact = crate::xinput::motors(rumble.level(t, freq));
        let out = feel.output_with(
            t,
            freq,
            (impact.0 as f32 / 65535.0, impact.1 as f32 / 65535.0),
            strength.min(100) as f32 / 100.0,
            choices,
        );
        let pad = wgi_pad(t, freq);
        if !pad.is_null() {
            // A 1/64 step, so a steady feel isn't sent again every frame.
            let q = |v: f32| (v.clamp(0.0, 1.0) * 64.0) as u8;
            let key = [
                q(out.heavy),
                q(out.light),
                q(out.left_trigger),
                q(out.right_trigger),
            ];
            let sent = &mut *addr_of_mut!(WGI_SENT);
            if key != *sent {
                let put: PutVibration = method(pad, IGAMEPAD_PUT_VIBRATION);
                let hr = put(
                    pad,
                    f64::from(out.heavy),
                    f64::from(out.light),
                    f64::from(out.left_trigger),
                    f64::from(out.right_trigger),
                );
                *sent = key;
                if hr < 0 {
                    // The pad went away: look for it again.
                    wgi_forget_pad();
                }
                if let Some(d) = diag() {
                    d.motor_max = d.motor_max.max((out.heavy * 65535.0) as u16);
                    d.send_errors += u32::from(hr < 0);
                }
            }
            return;
        }
        let Some((_, set)) = xinput() else { return };
        let motors = ((out.heavy * 65535.0) as u16, (out.light * 65535.0) as u16);
        let sent = &mut *addr_of_mut!(MOTORS);
        if motors != *sent {
            let mut v = [motors.0, motors.1];
            let rc = set(0, &mut v);
            *sent = motors;
            if let Some(d) = diag() {
                d.motor_max = d.motor_max.max(motors.0);
                d.send_errors += u32::from(rc != 0);
            }
        }
    }
}

static mut FEEL: crate::feel::Feel = crate::feel::Feel::new();

/// Windows.Gaming.Input's gamepad statics and player 1's pad, found once
/// (the pad looked for again each second until there is one).
struct Wgi {
    tried: bool,
    statics: *mut usize,
    pad: *mut usize,
    next_look: i64,
}

static mut WGI: Wgi = Wgi {
    tried: false,
    statics: core::ptr::null_mut(),
    pad: core::ptr::null_mut(),
    next_look: 0,
};
/// The vibration last sent (1/64 steps: heavy, light, left, right).
static mut WGI_SENT: [u8; 4] = [0; 4];

type RoInitialize = unsafe extern "system" fn(u32) -> i32;
type RoGetActivationFactory = unsafe extern "system" fn(*mut u8, *const u8, *mut *mut usize) -> i32;
type WindowsCreateString = unsafe extern "system" fn(*const u16, u32, *mut *mut u8) -> i32;
type WindowsDeleteString = unsafe extern "system" fn(*mut u8) -> i32;
type GetGamepads = unsafe extern "system" fn(*mut usize, *mut *mut usize) -> i32;
type VectorGetAt = unsafe extern "system" fn(*mut usize, u32, *mut *mut usize) -> i32;
type VectorSize = unsafe extern "system" fn(*mut usize, *mut u32) -> i32;
/// `IGamepad::put_Vibration(GamepadVibration)`: the struct's four doubles
/// (left motor, right motor, left trigger, right trigger) passed by value
/// are, on x86, the same stack as four double arguments.
type PutVibration = unsafe extern "system" fn(*mut usize, f64, f64, f64, f64) -> i32;
/// Method slots after IInspectable's six.
const STATICS_GET_GAMEPADS: usize = 10;
const VECTOR_GET_AT: usize = 6;
const VECTOR_SIZE: usize = 7;
const IGAMEPAD_PUT_VIBRATION: usize = 7;
/// IID_IGamepadStatics {8BBCE529-D49C-39E9-9560-E47DDE96B7C8}.
const IID_IGAMEPAD_STATICS: [u8; 16] = [
    0x29, 0xe5, 0xbc, 0x8b, 0x9c, 0xd4, 0xe9, 0x39, 0x95, 0x60, 0xe4, 0x7d, 0xde, 0x96, 0xb7, 0xc8,
];

/// Player 1's pad through Windows.Gaming.Input, or null (no such API, as
/// under Wine; no pad yet).
unsafe fn wgi_pad(now: i64, freq: i64) -> *mut usize {
    // SAFETY: the main thread's own state; combase's documented exports;
    // COM objects through their method tables, every reference released.
    unsafe {
        let w = &mut *addr_of_mut!(WGI);
        if !w.tried {
            w.tried = true;
            let combase = LoadLibraryA(c"combase.dll".as_ptr().cast());
            if combase.is_null() {
                return core::ptr::null_mut();
            }
            let f = |name: &core::ffi::CStr| GetProcAddress(combase, name.as_ptr().cast());
            let (init, factory, create, delete) = (
                f(c"RoInitialize"),
                f(c"RoGetActivationFactory"),
                f(c"WindowsCreateString"),
                f(c"WindowsDeleteString"),
            );
            if init.is_null() || factory.is_null() || create.is_null() || delete.is_null() {
                return core::ptr::null_mut();
            }
            let init: RoInitialize = core::mem::transmute(init);
            let factory: RoGetActivationFactory = core::mem::transmute(factory);
            let create: WindowsCreateString = core::mem::transmute(create);
            let delete: WindowsDeleteString = core::mem::transmute(delete);
            // Multithreaded; a thread already in a COM apartment keeps it.
            init(1);
            let name = b"Windows.Gaming.Input.Gamepad";
            let wide: [u16; 28] = core::array::from_fn(|i| u16::from(name[i]));
            let mut hs = core::ptr::null_mut();
            if create(wide.as_ptr(), wide.len() as u32, &mut hs) < 0 {
                return core::ptr::null_mut();
            }
            let mut statics = core::ptr::null_mut();
            let hr = factory(hs, IID_IGAMEPAD_STATICS.as_ptr(), &mut statics);
            delete(hs);
            if hr >= 0 {
                w.statics = statics;
            }
        }
        if w.statics.is_null() {
            return core::ptr::null_mut();
        }
        if w.pad.is_null() && now >= w.next_look {
            w.next_look = now + freq;
            let get: GetGamepads = method(w.statics, STATICS_GET_GAMEPADS);
            let mut view = core::ptr::null_mut();
            if get(w.statics, &mut view) >= 0 && !view.is_null() {
                let size: VectorSize = method(view, VECTOR_SIZE);
                let mut n = 0;
                if size(view, &mut n) >= 0 && n > 0 {
                    let at: VectorGetAt = method(view, VECTOR_GET_AT);
                    let mut pad = core::ptr::null_mut();
                    if at(view, 0, &mut pad) >= 0 {
                        w.pad = pad;
                    }
                }
                let release: Release = method(view, RELEASE);
                release(view);
            }
        }
        w.pad
    }
}

fn wgi_forget_pad() {
    // SAFETY: the main thread's own state; the pad's reference released.
    unsafe {
        let w = &mut *addr_of_mut!(WGI);
        if !w.pad.is_null() {
            let release: Release = method(w.pad, RELEASE);
            release(w.pad);
            w.pad = core::ptr::null_mut();
        }
        *addr_of_mut!(WGI_SENT) = [0; 4];
    }
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
    share_core(&raw mut DRAW_THREAD);
    forget_hd_pairs();
    // SAFETY: the launcher's settings block in the fix memory.
    let settings = unsafe { &*settings };
    diag_frame();
    update_rumble(settings);
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

const DDSCAPS_TEXTURE: u32 = 0x1000;
const DDSCAPS_VIDEOMEMORY: u32 = 0x4000;
const DDSCAPS_ALLOCONLOAD: u32 = 0x0400_0000;
const DDSCAPS_MIPMAP: u32 = 0x0040_0000;
const DDSCAPS_COMPLEX: u32 = 0x8;
const DDSD_MIPMAPCOUNT: u32 = 0x0002_0000;
const DDCKEY_SRCBLT: u32 = 0x8;
const DDPF_RGB: u32 = 0x40;
const DDPF_ALPHAPIXELS: u32 = 0x1;
const DDPF_PALETTEINDEXED8: u32 = 0x20;
const QUERY_INTERFACE: usize = 0;
const GET_ATTACHED_SURFACE: usize = 0x30 / 4;
const GET_COLOR_KEY: usize = 0x40 / 4;
type QueryInterface = unsafe extern "system" fn(*mut usize, *const u8, *mut *mut usize) -> i32;
type GetAttachedSurface = unsafe extern "system" fn(*mut usize, *const u32, *mut *mut usize) -> i32;
type GetColorKey = unsafe extern "system" fn(*mut usize, u32, *mut [u32; 2]) -> i32;
/// IID_IDirectDrawSurface {6C14DB81-A733-11CE-A521-0020AF0BE560}.
const IID_IDIRECTDRAWSURFACE: [u8; 16] = [
    0x81, 0xdb, 0x14, 0x6c, 0x33, 0xa7, 0xce, 0x11, 0xa5, 0x21, 0x00, 0x20, 0xaf, 0x0b, 0xe5, 0x60,
];

/// Before the game creates a surface (`0x49eb66`, F18): a race texture in
/// video memory gets a mipmap chain.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_texture_desc(desc: *mut SurfaceDesc) {
    // SAFETY: the game's surface description on its stack.
    let desc = unsafe { &mut *desc };
    if desc.caps != DDSCAPS_VIDEOMEMORY | DDSCAPS_TEXTURE | DDSCAPS_ALLOCONLOAD {
        return;
    }
    let levels = crate::mipmap::levels(desc.width, desc.height);
    if levels > 1 {
        desc.caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
        desc.flags |= DDSD_MIPMAPCOUNT;
        desc.rest1[1] = levels;
    }
}

/// After the game uploads a texture (`IDirect3DTexture2::Load`, F18): its
/// smaller levels are made from the uploaded one.
#[unsafe(no_mangle)]
pub extern "stdcall" fn cmr_texture_loaded(texture: *mut usize) {
    if texture.is_null() {
        return;
    }
    // SAFETY: COM objects of the game's live textures; every reference
    // taken here is released.
    unsafe {
        let qi: QueryInterface = method(texture, QUERY_INTERFACE);
        let mut surface = core::ptr::null_mut();
        if qi(texture, IID_IDIRECTDRAWSURFACE.as_ptr(), &mut surface) != 0 || surface.is_null() {
            return;
        }
        let key = {
            let get: GetColorKey = method(surface, GET_COLOR_KEY);
            let mut k = [0u32; 2];
            (get(surface, DDCKEY_SRCBLT, &mut k) == 0).then_some(k[0] as u16)
        };
        let fmt = desc_of(surface)
            .filter(|d| d.bits == 16 && d.pf_flags & DDPF_RGB != 0)
            .and_then(|d| {
                let alpha = if d.pf_flags & DDPF_ALPHAPIXELS != 0 {
                    d.alpha_mask
                } else {
                    0
                };
                crate::mipmap::Format::new(d.red_mask, d.green_mask, d.blue_mask, alpha)
            });
        // Palettized levels (dgVoodoo2 has such textures; Wine doesn't).
        let palettized = fmt.is_none()
            && desc_of(surface)
                .is_some_and(|d| d.bits == 8 && d.pf_flags & DDPF_PALETTEINDEXED8 != 0);
        let mut cur = surface;
        if palettized {
            let caps = DDSCAPS_MIPMAP | DDSCAPS_TEXTURE;
            loop {
                let attached: GetAttachedSurface = method(cur, GET_ATTACHED_SURFACE);
                let mut next = core::ptr::null_mut();
                if attached(cur, &caps, &mut next) != 0 || next.is_null() {
                    break;
                }
                if let Some(src) = lock(cur, DDLOCK_READONLY) {
                    if let Some(dst) = lock(next, DDLOCK_WRITEONLY) {
                        crate::mipmap::downsample8(
                            key.map(|k| k as u8),
                            src.surface as *const u8,
                            src.pitch as usize,
                            src.width,
                            src.height,
                            dst.surface,
                            dst.pitch as usize,
                        );
                        unlock(next);
                    }
                    unlock(cur);
                }
                let release: Release = method(cur, RELEASE);
                release(cur);
                cur = next;
            }
        }
        if let Some(fmt) = fmt {
            let caps = DDSCAPS_MIPMAP | DDSCAPS_TEXTURE;
            loop {
                let attached: GetAttachedSurface = method(cur, GET_ATTACHED_SURFACE);
                let mut next = core::ptr::null_mut();
                if attached(cur, &caps, &mut next) != 0 || next.is_null() {
                    break;
                }
                if let Some(src) = lock(cur, DDLOCK_READONLY) {
                    if let Some(dst) = lock(next, DDLOCK_WRITEONLY) {
                        crate::mipmap::downsample(
                            &fmt,
                            key,
                            src.surface as *const u16,
                            src.pitch as usize / 2,
                            src.width,
                            src.height,
                            dst.surface as *mut u16,
                            dst.pitch as usize / 2,
                        );
                        unlock(next);
                    }
                    unlock(cur);
                }
                let release: Release = method(cur, RELEASE);
                release(cur);
                cur = next;
            }
        }
        let release: Release = method(cur, RELEASE);
        release(cur);
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
        // The front end: its blits are noted for HD twins (F27).
        if index == 3 {
            // SAFETY: the stub passes the blit's argument area.
            unsafe { note_fe_blit(args as *const usize) };
        }
        return;
    };
    if index >= 8 {
        return;
    }
    // SAFETY: the stub passes the caller's argument area.
    unsafe { scale_arg(args.add(index as usize), &canvas, &mut hud.one) };
}
