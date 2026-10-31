#![cfg_attr(all(not(test), windows), no_std)]
//! `cmrhooks.dll`, loaded into the game by CMRLauncher before it runs.
//!
//! The launcher's byte-level fixes (see `cmr-launcher::fixes`, F13) call
//! these exports from small stubs; each stub checks that its table entry was
//! filled, so without this library the game behaves as the original.
//!
//! **In-race 2D (F13).** With a race resolution the game never had (F10),
//! its 2D (HUD, messages, results, loading screen) mixes rectangles in
//! 640x480 space scaled by W/640, fractions of W/H, and absolute pixel
//! tables picked by exact width, plus text drawn at the font's native size.
//! In a race the game is shown a virtual 640x480 screen (`sections`), so
//! all of it resolves in the one space it was made for; the 2D primitives
//! then map every rectangle onto a 4:3 canvas centred on the real screen
//! (`canvas`). Display set-up and the 3D draw see the real screen.

pub mod canvas;
pub mod depth;
pub mod easing;
pub mod fe_hd;
pub mod hd;
pub mod mipmap;
pub mod pacing;
pub mod pad;
pub mod present;
pub mod sections;
pub mod xinput;

#[cfg(all(not(test), windows))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Referenced by the unwind tables of the pre-built `core`; panics abort in
/// this library (release profile), so it is never called.
#[cfg(all(not(test), windows))]
#[unsafe(no_mangle)]
pub extern "C" fn rust_eh_personality() {}

/// The DLL entry point. The library needs no runtime start-up (it is
/// linked without the C start files), so attaching just succeeds.
#[cfg(all(not(test), windows))]
#[unsafe(no_mangle)]
pub extern "system" fn DllMainCRTStartup(_: *mut u8, reason: u32, _: *mut u8) -> i32 {
    const DLL_PROCESS_DETACH: u32 = 0;
    // The game is ending: no rumble may outlive it.
    #[cfg(target_arch = "x86")]
    if reason == DLL_PROCESS_DETACH {
        exports::stop_rumble();
    }
    let _ = reason;
    1
}

#[cfg(all(not(test), windows, target_arch = "x86"))]
mod exports;
