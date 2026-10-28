# Third-party components

Everything bundled with or compiled into the package, with its licence and
source. Add a row before bundling anything new.

## Compiled into CMRLauncher.exe

| Component | Licence | Source |
| --- | --- | --- |
| `sha2` and its RustCrypto dependencies | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| `windows-sys` | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| Rust standard library | MIT OR Apache-2.0 | https://github.com/rust-lang/rust |
| MinGW-w64 runtime (static parts linked by the GNU target) | ZPL-2.1 / public domain | https://www.mingw-w64.org |

## Bundled DLLs

None yet. Candidates and their state are in `docs/THIN_APP_PLAN.md`.

## Used by the game but not shipped by us

Provided by the user's game folder (`qmixer.dll`) or by the operating system
or Wine/Proton (DirectDraw, Direct3D, DirectInput, DirectPlay, WinMM).
