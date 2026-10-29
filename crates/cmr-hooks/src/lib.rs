#![cfg_attr(all(not(test), windows), no_std)]
//! `cmrhooks.dll`, loaded into the game by CMRLauncher before it runs.
//!
//! The launcher's byte-level fixes (see `cmr-launcher::fixes`, F13) call
//! these exports from small stubs; each stub checks that its table entry was
//! filled, so without this library the game behaves as the original.
//!
//! **HUD canvas (F13).** With a race resolution the game never had (F10),
//! the in-race HUD mixes three coordinate spaces: rectangles in 640x480
//! space scaled by W/640, fractions of W/H, and absolute pixel tables picked
//! by exact width (the rev dial), plus text drawn at the font's native size.
//! While the HUD lays itself out and draws, the game is shown a virtual
//! 640x480 screen, so all of it resolves in one space; every sprite it
//! queues is then mapped onto a 4:3 canvas centred on the real screen.

pub mod canvas;

#[cfg(all(not(test), windows))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// The DLL entry point. The library needs no runtime start-up (it is
/// linked without the C start files), so attaching just succeeds.
#[cfg(all(not(test), windows))]
#[unsafe(no_mangle)]
pub extern "system" fn DllMainCRTStartup(_: *mut u8, _: u32, _: *mut u8) -> i32 {
    1
}

#[cfg(all(not(test), windows, target_arch = "x86"))]
mod exports;
