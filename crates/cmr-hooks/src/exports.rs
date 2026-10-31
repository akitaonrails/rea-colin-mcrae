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
        return 0;
    }
    // SAFETY: the game's display struct: +0 W, +4 H, +0x428 the back
    // buffer (an IDirectDrawSurface of the screen's size); the picture is
    // its top left 640x480, copied out before the back buffer is written.
    unsafe {
        let (w, h) = (*(display as *const i32), *(display.add(4) as *const i32));
        let back = *(display.add(0x428) as *const *mut usize);
        if (w, h) != (640, 480) || back.is_null() {
            return 0;
        }
        let Some(buf) = lock(back, 0) else {
            return 0;
        };
        if buf.width > 640 && buf.height >= 480 && buf.bits == 16 && buf.pitch > 0 {
            let scratch = &mut *addr_of_mut!(PICTURE);
            let pitch = buf.pitch as usize;
            for y in 0..480 {
                core::ptr::copy_nonoverlapping(
                    buf.surface.add(y * pitch) as *const u16,
                    scratch.as_mut_ptr().add(y * 640),
                    640,
                );
            }
            crate::present::blit(
                scratch.as_ptr() as *const u8,
                640 * 2,
                buf.green_mask == 0x7e0,
                buf.surface,
                pitch,
                16,
                buf.width,
                buf.height,
            );
        }
        let size = (buf.width, buf.height);
        unlock(back);
        if size.0 > 640 && size.1 >= 480 {
            draw_fe_twins(back, size.0, size.1);
        }
        diag_frame();
        0
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
    let Some((get, _)) = xinput() else { return };
    let mut pad = PadState::default();
    // SAFETY: XInput fills the state it is given.
    if unsafe { get(player, &mut pad) } != 0 {
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
        let strength = (*settings).rumble;
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
        // GENERIC_READ, FILE_SHARE_READ, OPEN_EXISTING
        let f = CreateFileA(
            path.as_ptr(),
            0x8000_0000,
            1,
            core::ptr::null_mut(),
            3,
            0x80,
            core::ptr::null_mut(),
        );
        if f as isize == -1 {
            return core::ptr::null_mut();
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
        out
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
        if game.is_null() || bmp.is_null() || flags & 1 == 0 {
            return game;
        }
        let hd = hd_bitmap(bmp);
        if hd.is_null() {
            return game;
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
        let game = load(record, bmp, flags);
        if game.is_null() || bmp.is_null() {
            return game;
        }
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
        draws.0[draws.1] = entry;
        draws.1 += 1;
    }
}

/// `IDirectDrawSurface::Blt` and `SetColorKey`.
type Blt = unsafe extern "system" fn(
    *mut usize,
    *const [i32; 4],
    *mut usize,
    *const [i32; 4],
    u32,
    *const u8,
) -> i32;
const BLT: usize = 0x14 / 4;
type SetColorKey = unsafe extern "system" fn(*mut usize, u32, *const [u32; 2]) -> i32;
const SET_COLOR_KEY: usize = 0x74 / 4;
const DDBLT_WAIT: u32 = 0x0100_0000;
const DDBLT_KEYSRC: u32 = 0x8000;

/// Whether the finished picture (`PICTURE`) still shows the image of blit
/// `d` where it was drawn (`fe_hd::intact`).
unsafe fn image_intact(d: &FeDraw, w: i32, h: i32) -> bool {
    let Some((dst, src)) = crate::fe_hd::clip(d.draw.dst, d.src, w, h) else {
        return false;
    };
    // SAFETY: the image's live surface, read and unlocked again; the
    // picture copied out by `cmr_fe_present` this frame.
    unsafe {
        let surface = d.surface as *mut usize;
        let Some(img) = lock(surface, DDLOCK_READONLY) else {
            return false;
        };
        let ok = img.bits == 16
            && crate::fe_hd::intact(
                &*addr_of_mut!(PICTURE),
                dst,
                src,
                |x, y| {
                    *(img.surface.add(y as usize * img.pitch as usize) as *const u16)
                        .add(x as usize)
                },
                d.key.map(|k| k[0] as u16),
            );
        unlock(surface);
        ok
    }
}

/// After the menu picture is enlarged onto `back` (`w` x `h`): each twin
/// noted this frame that nothing later covers, drawn over its spot at the
/// screen's size. Then the frame's notes are cleared.
unsafe fn draw_fe_twins(back: *mut usize, w: u32, h: u32) {
    // SAFETY: the game's live surfaces; the main thread's own table.
    unsafe {
        let draws = &mut *addr_of_mut!(FE_DRAWS);
        let noted = &draws.0[..draws.1];
        // The blits into the picture, in order.
        let mut picture = [crate::fe_hd::Draw {
            dst: [0; 4],
            twin: false,
        }; 1024];
        let mut which = [0usize; 1024];
        let mut n = 0;
        for (i, d) in noted.iter().enumerate() {
            if d.dst_surface == back as usize {
                picture[n] = d.draw;
                which[n] = i;
                n += 1;
            }
        }
        let mut shown = 0;
        if !draws.2 {
            let target = crate::present::target(w, h);
            let blt: Blt = method(back, BLT);
            for (k, show) in crate::fe_hd::shown(&picture[..n]).enumerate() {
                let d = &noted[which[k]];
                let (w, h) = (crate::present::SRC_W as i32, crate::present::SRC_H as i32);
                let clipped = crate::fe_hd::clip(d.draw.dst, d.twin_src, w, h);
                let (true, Some((dst, src))) = (show, clipped) else {
                    continue;
                };
                if !image_intact(d, w, h) {
                    continue;
                }
                let twin = d.twin_surface as *mut usize;
                let mut flags = DDBLT_WAIT;
                if let Some(key) = d.key {
                    let set: SetColorKey = method(twin, SET_COLOR_KEY);
                    set(twin, DDCKEY_SRCBLT, &key);
                    flags |= DDBLT_KEYSRC;
                }
                let screen = crate::fe_hd::to_screen(dst, target);
                let src = [src[0], src[1], src[0] + src[2], src[1] + src[3]];
                if blt(back, &screen, twin, &src, flags, core::ptr::null()) == 0 {
                    shown += 1;
                }
            }
        }
        if let Some(dg) = diag() {
            dg.fe_draws += n as u32;
            dg.fe_shown += shown;
        }
        draws.1 = 0;
        draws.2 = false;
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

/// Each frame: sends the fading rumble to player 1's pad; off outside
/// driving (menus, loading, the pause menu).
fn update_rumble(settings: &PresentSettings) {
    if settings.rumble == 0 {
        return;
    }
    let Some((_, set)) = xinput() else { return };
    let mut freq = 0;
    // SAFETY: plain Win32 calls and the main thread's own statics.
    unsafe {
        QueryPerformanceFrequency(&mut freq);
        let rumble = &mut *addr_of_mut!(RUMBLE);
        // Driving states only: 5..=9 (state 0xa is the pause menu).
        if !(5..=9).contains(&Game.state()) {
            rumble.stop();
        }
        let motors = crate::xinput::motors(rumble.level(now(), freq));
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
        let mut cur = surface;
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
