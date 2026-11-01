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
| `image`, `image-webp`, `png` and their dependencies: installing the HD art pack | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| Rust standard library | MIT OR Apache-2.0 | https://github.com/rust-lang/rust |
| MinGW-w64 runtime (static parts linked by the GNU target) | ZPL-2.1 / public domain | https://www.mingw-w64.org |

## Bundled data

| Component | Licence | Source |
| --- | --- | --- |
| The HD art pack (`hd/`): the game's own 2D art redrawn with `cmr-hd` and `tools/hd-redraw` (owner decision, see `AGENTS.md`) | derived from Colin McRae Rally (Codemasters); not covered by this project's licence | `packaging/hd-pack.txt` (release asset `hd-pack-3`) |

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

## The cmr-hd study tool (built, not shipped)

`crates/cmr-hd` (docs/UPSCALING.md) is a developer and player tool that is
not part of the package. It links:

| Component | Licence | Source |
| --- | --- | --- |
| `flate2` (with `miniz_oxide`) | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| `image` (BMP and PNG codecs) and its dependencies | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| `crates/cmr-hd/src/depixel.rs`: ported code (Rust port of depixel-js by Jimb Esser, itself a port of Felix Kreuzer's "Depixelizing Pixel Art on GPUs", after Kopf and Lischinski's paper "Depixelizing Pixel Art") | MIT (both) | https://github.com/Jimbly/depixel-js , https://github.com/falichs/Depixelizing-Pixel-Art-on-GPUs , https://johanneskopf.de/publications/pixelart/ |
| `libwebp` (through the `webp` crate) in `cmr-hd`, for making the pack (not shipped) | BSD-3-Clause (`webp`: MIT OR Apache-2.0) | https://chromium.googlesource.com/webm/libwebp , https://github.com/jaredforth/webp |

It can run Real-ESRGAN (`realesrgan-ncnn-vulkan`, BSD-3-Clause,
https://github.com/xinntao/Real-ESRGAN) as a separate program the user
downloads; nothing of it is bundled.

## The tools/hd-redraw scripts (run, not shipped)

`tools/hd-redraw` (docs/UPSCALING.md) makes part of the HD art pack. It
runs on the developer's machine and is not part of the package. It uses:

| Component | Licence | Source |
| --- | --- | --- |
| NumPy | BSD-3-Clause | https://numpy.org |
| SciPy | BSD-3-Clause | https://scipy.org |
| Pillow | MIT-CMU (HPND) | https://python-pillow.org |
| Google's Gemini API, model `gemini-3.1-flash-image`: a web service the scripts call with the developer's own key; nothing of it is bundled | Gemini API terms of service | https://ai.google.dev/gemini-api/terms |

## Compiled into cmrhooks.dll

Our own code only (`crates/cmr-hooks`, `no_std`).

## Used by the game but not shipped by us

Provided by the user's game folder (`qmixer.dll`) or by the operating system
or Wine/Proton (DirectDraw, Direct3D, DirectInput, DirectPlay, WinMM).
