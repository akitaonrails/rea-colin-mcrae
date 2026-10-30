# Third-party components

Everything bundled with or compiled into the package, with its licence and
source. Add a row before bundling anything new.

## Compiled into CMRLauncher.exe

| Component | Licence | Source |
| --- | --- | --- |
| `sha2` and its RustCrypto dependencies | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| `windows-sys` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| `eframe`, `egui` and their dependencies (`winit`, `glutin`, `glow`, `ab_glyph`, …): the settings window | MIT OR Apache-2.0 (dependencies: MIT, Apache-2.0, Zlib, BSD; see `cargo tree`) | https://github.com/emilk/egui |
| `jpeg-decoder` | MIT OR Apache-2.0 | https://github.com/image-rs/jpeg-decoder |
| Rust standard library | MIT OR Apache-2.0 | https://github.com/rust-lang/rust |
| MinGW-w64 runtime (static parts linked by the GNU target) | ZPL-2.1 / public domain | https://www.mingw-w64.org |

## Bundled DLLs

None yet. Candidates and their state are in `docs/THIN_APP_PLAN.md`.

## Bundled in the Linux AppImage (`packaging/appimage`)

| Component | Licence | Source |
| --- | --- | --- |
| Wine 11.20 (amd64, WoW64), Kron4ek's build of the unmodified WineHQ sources; licence and source notes inside the AppImage under `usr/share/doc/wine` | LGPL-2.1-or-later | https://dl.winehq.org/wine/source/11.x/wine-11.20.tar.xz, build scripts https://github.com/Kron4ek/Wine-Builds |
| AppImage runtime (added by `appimagetool` 1.9.1) | MIT | https://github.com/AppImage/type2-runtime |
| The AppImage icon `packaging/appimage/cmr-launcher.png` | ours (MIT OR Apache-2.0), set in Barlow (OFL) | this repository |

## Settings window artwork and fonts (compiled into CMRLauncher.exe)

| File | Licence | Source |
| --- | --- | --- |
| `crates/cmr-launcher/assets/portrait.jpg` — Colin McRae, photo by Carolyn Williams (resized) | CC BY 2.0 (credited in the window) | https://commons.wikimedia.org/wiki/File:Colin_Mcrae_crop.jpg |
| `crates/cmr-launcher/assets/fonts/Barlow-*.ttf`, `BarlowCondensed-*.ttf` | SIL OFL 1.1 (`assets/fonts/OFL.txt`) | https://github.com/jpt/barlow |

## Compiled into cmrhooks.dll

Our own code only (`crates/cmr-hooks`, `no_std`).

## Used by the game but not shipped by us

Provided by the user's game folder (`qmixer.dll`) or by the operating system
or Wine/Proton (DirectDraw, Direct3D, DirectInput, DirectPlay, WinMM).
