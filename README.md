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
- a small hook library with the game-specific fixes (CD check, Windows
  version check), applied in memory at run time.

This repository contains no game files and never will. See
[docs/THIN_APP_PLAN.md](docs/THIN_APP_PLAN.md) for the design and status.

## History

The project started as a static source reconstruction. That work is parked
at tag `recomp-parked-2026-11-02`; its documents are kept under
[docs/recomp/](docs/recomp/) because they describe exactly how the game uses
DirectDraw, Direct3D, audio, input and its files. The full parked tree
(including local notes) is kept outside this repository as a reference.
