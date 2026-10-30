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
}

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
}

static mut DIAG: Diag = Diag {
    file: None,
    window: 0,
    updates: 0,
    last_update: 0,
    gap_max: 0,
    last_frame: 0,
    frame_max: 0,
};

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
        .text(b" ms, frame max ")
        .num(ms(d.frame_max))
        .text(b" ms, state ")
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
    };
}

/// A small text buffer for the diagnostics (no `std` formatting here).
struct Line {
    buf: [u8; 200],
    len: usize,
}

impl Default for Line {
    fn default() -> Line {
        Line {
            buf: [0; 200],
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
        if strength > 0 {
            (*addr_of_mut!(RUMBLE)).impact(severity, strength, now());
        }
    }
}

/// Each frame: sends the fading rumble to player 1's pad; off outside races.
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
        if !Game.in_race() {
            rumble.stop();
        }
        let motors = crate::xinput::motors(rumble.level(now(), freq));
        let sent = &mut *addr_of_mut!(MOTORS);
        if motors != *sent {
            let mut v = [motors.0, motors.1];
            set(0, &mut v);
            *sent = motors;
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
        return;
    };
    if index >= 8 {
        return;
    }
    // SAFETY: the stub passes the caller's argument area.
    unsafe { scale_arg(args.add(index as usize), &canvas, &mut hud.one) };
}
