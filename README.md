# Colin McRae Rally compatibility package

Run your own copy of the 1998 PC **Colin McRae Rally** on modern machines:
Windows 10/11 natively, and Linux through Proton or Wine. You point the
launcher at your game; it prepares everything else itself. No third-party
patches, wrappers, codec packs, compatibility modes or registry edits.

The game's own `Rally.exe` runs unmodified on disk. The package adds the
pieces the 1998 game expected from Windows and no longer finds:

- a launcher that sets the game up from your files (replacing the installer);
- open-source compatibility libraries for graphics, controllers and movies,
  bundled and preconfigured;
- the game-specific fixes (CD check, Windows version check, the 8-bit front
  end, VGA port access), applied in memory by the launcher at run time.

This repository contains no game files and never will. See
[docs/THIN_APP_PLAN.md](docs/THIN_APP_PLAN.md) for the design and status.

## Using it (early)

```text
CMRLauncher.exe --game <folder with your Rally.exe>
```

The folder is remembered, so later runs need no arguments; a launcher copied
into the game folder finds it by itself. On Linux, run the same exe with
Wine (`wine CMRLauncher.exe --game Z:/path/to/game`); the Proton/Steam route
is still being settled. The launcher keeps its copy of the game, saves and
`launcher.log` under `%LOCALAPPDATA%\CMRLauncher`.

Settings live in `launcher.cfg` in the same folder, written with comments on
the first run (a `launcher.cfg` next to `CMRLauncher.exe` takes its place for
portable installs). Besides the folders and language it holds `close_quits`
(closing the window quits the game, or `false` for the original behaviour)
and `gamepad_axes` (a pad's triggers and right stick work as throttle and
brake), and the opt-in graphics settings `resolution`, `widescreen` and
`draw_distance` (see the comments in the file and
[docs/GRAPHICS.md](docs/GRAPHICS.md)). Keep `cmrhooks.dll` next to
`CMRLauncher.exe`: it holds the HUD scaling for higher resolutions.
`CMRLauncher --controllers` lists the joysticks the game can see and
logs what each stick, trigger and button sends.

## Building

```text
cargo test                                         # host logic
cargo build --release --target i686-pc-windows-gnu  # needs i686-w64-mingw32-gcc;
                                                   # builds CMRLauncher.exe and cmrhooks.dll
```

## History

The project started as a static source reconstruction. That work is parked
at tag `recomp-parked-2026-11-02`; its documents are kept under
[docs/recomp/](docs/recomp/) because they describe exactly how the game uses
DirectDraw, Direct3D, audio, input and its files. The full parked tree
(including local notes) is kept outside this repository as a reference.
