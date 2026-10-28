# Thin compatibility app: plan

Status: proposed, 2026-11-02. The source reconstruction is parked at tag
`recomp-parked-2026-11-02` (see `HANDOFF.md`).

## Goal

Run the user's own copy of the PC `Rally.exe`, unmodified on disk, on modern
machines. Instead of the 1998 installer and DirectX/QSound runtime, it gets a
thin launcher and our own replacement libraries for the few old libraries it
needs. First goal: the game runs exactly as it shipped. Later goals use the
same layer to improve it: 4K and other resolutions, widescreen, 60 fps, a
modern window and modern controllers.

We never ship or commit the game's files. The launcher points at the user's
copy and builds a runtime folder from it.

Two ground rules from the owner:

- **Self-contained.** The user installs our package and points it at the
  game. No third-party patches (Patrxgt, kFYatek, ChemicalFlood), no
  dgVoodoo or other wrappers, no codec packs, no compatibility modes, no
  manual registry edits. Every fix the game needs is ours, done in memory at
  run time.
- **A Windows program first, easy on Linux.** The package is Windows binaries
  (launcher + DLLs + bundled `SDL3.dll`). On Linux it runs under Proton (for
  example as a non-Steam game, or through `umu-run`) or plain Wine, with no
  extra setup: the launcher configures everything inside its prefix itself.

## What the game needs (from the decomp)

`Rally.exe` (official EU v1.5, SHA-256 `0eb6ea96…`) imports 10 libraries and
168 functions. Five of them are what breaks on modern systems:

| Library | Used for | Our replacement |
| --- | --- | --- |
| `DDRAW.dll` | DirectDraw 2D, plus Direct3D 5 Immediate Mode reached through it (`IDirect3DDevice2`, `DrawIndexedPrimitive` with `D3DLVERTEX`, 16-bit Z-buffer) | `ddraw.dll` on SDL3 + OpenGL 3.3 |
| `QMIXER.dll` | QSound mixer, 16 `QSWaveMix*` calls | `qmixer.dll` on SDL3 audio (`cmr-core::audio_mixer` already holds the semantics) |
| `WINMM.dll` | joystick (`joyGetPosEx`), timers, MCI AVI playback | `winmm.dll` proxy: joystick from SDL3 gamepads, the rest forwarded to the system `winmm` |
| `DINPUT.dll` | `DirectInputCreateA` (force feedback, to confirm) | `dinput.dll`: SDL3 haptics, or a stub |
| `DPLAYX.dll` | DirectPlay multiplayer (ordinals 1, 2) | `dplayx.dll` stub that fails cleanly (multiplayer later, if wanted) |

The rest (`KERNEL32`, `USER32`, `GDI32`, `ADVAPI32`, `ole32`) is ordinary
Win32 that Windows and Wine both provide. Three environment issues remain:

- **Registry:** paths under `HKLM\SOFTWARE\Codemasters\Rally\1.00` (`CDPath`,
  `HDPath`, `AVIPath`, `WAVPath`, `Language`, plus game-managed `Hardware`,
  `Initialised`). On Wine the launcher writes them into its own prefix. On
  Windows, UAC registry virtualization lets a 32-bit, non-manifested process
  read them from `HKCU\Software\Classes\VirtualStore\MACHINE\SOFTWARE\WOW6432Node\…`,
  so no admin rights are needed.
- **CD check** (`GetDriveTypeA`, `GetLogicalDrives`) and the **Windows version
  check** (the game crashes loading a stage on Win7+ unless it believes it is
  on XP). Both get fixed in memory, never on disk: when our `ddraw.dll` loads,
  it patches `Rally.exe`'s import table entries for `GetDriveTypeA`,
  `GetVersionExA` and `GetVersion`.
- **Movies** are MCI `avivideo`. The intro uses Indeo 5 (IV50), which modern
  Windows no longer ships (Wine/Proton has a built-in decoder). The school
  videos are MS Video 1. Being self-contained means we can't ask for a codec
  pack, so the `winmm` proxy takes over AVI playback and decodes the movies
  itself, drawing through our window. MS Video 1 is already written (in the
  recomp tree); Indeo 5 is new work.
- **Patch level.** The game may be v1.0 from the CD or an updated v1.5.
  Shipping Codemasters' official patch would mean redistributing their
  binary, so instead the in-memory fixes are kept per edition, keyed by the
  exe's hash. The official EU v1.5 build is the first target, then the
  editions users actually have.

## What the local setup taught us (`~/Projects/distrobox-gaming`)

CMR1 already runs on this machine under Wine 11.20 (wined3d → OpenGL), with
these workarounds:

- registry paths written by hand;
- `winver=winxp` for `Rally.exe`;
- the NVIDIA GPU forced for OpenGL (PRIME offload);
- a dedicated fullscreen Xwayland server pinned to 800x600, the only way it
  found to scale the image;
- joysticks dead (neither Wine's WinMM/DirectInput nor Xidi), so an `evsieve`
  bridge turns the pad into keyboard keys.

Dead ends:

- every gamescope variant left frame trails;
- dgVoodoo2 page-faulted after the mode set;
- Wine's Vulkan renderer gave a black screen;
- the GDI renderer crashed.

Our own `ddraw.dll` owns the window and the scaling, and the `winmm.dll` proxy
gives real analog joystick input, so these workarounds go away.

It used the ChemicalFlood pack's patched `Rally.exe` (SHA-256 `46500c8e…`), not
the official EU v1.5 binary we reverse-engineered. The launcher will identify
the edition by hash, and the official v1.5 exe is the reference target.

## Architecture

```text
CMRLauncher.exe (Windows, also the thing Proton/Wine runs on Linux)
  ├─ inputs: path to the user's game (installed folder, portable folder,
  │          original CD or its ISO)
  ├─ identifies the edition by hash; refuses unknown executables with a clear message
  ├─ builds a runtime folder: copies/links the game's files, drops our DLLs and
  │  SDL3.dll next to Rally.exe (the original folder is never modified)
  ├─ writes the game's registry paths: the user's VirtualStore on Windows (no
  │  admin), or the prefix's HKLM under Proton/Wine (same Win32 call, no extra setup)
  └─ starts Rally.exe as a child process; under Wine it sets the DLL load order
     for its own child, so no WINEDLLOVERRIDES or winecfg is needed

replacement DLLs (Rust, i686-pc-windows-gnu, one shared support crate)
  ddraw.dll   DirectDraw + Direct3D 5 on SDL3 + OpenGL 3.3; also patches the
              import table for the CD/version checks at load
  qmixer.dll  QSWaveMix* on SDL3 audio
  winmm.dll   joy* from SDL3 gamepads; everything else forwarded
  dinput.dll  force feedback through SDL3 haptics, or a stub
  dplayx.dll  stub
```

Why these choices:

- **Drop-in DLLs over a patched exe.** Nothing of the game is modified or
  redistributed. The same DLLs work natively on Windows and under Wine on
  Linux.
- **One package for both systems.** On Linux, Proton/Wine provides KERNEL32,
  USER32 and so on; we replace only the parts that break, with the same DLLs
  Windows uses. A Wine-free native Linux loader (in the style of `wibo`) is a
  possible later phase and would reuse those DLLs.
- **OpenGL 3.3 through SDL3** is the most predictable backend under both
  Windows and Wine. It's easy to write GLSL for D3D5's fixed-function states:
  colour key, alpha blend, fog, modulate. SDL_GPU/Vulkan can come later behind
  the same interface.
- **Rust**, as the project already prefers, with `unsafe` COM vtables kept in
  one narrow module per interface.

## Phases

Each phase ends with something runnable. Every step records runtime
observations as evidence.

### Phase 0: preserve the reference and set up

1. Copy the whole repository, including the ignored `.work/` and
   `runtime-data/`, to `/mnt/data/Projects/rea-colin-mcrae-recomp`. Verify the
   copy (file counts, `git log`, a build). Nothing is deleted.
2. In this repository, on a new branch: remove the recomp crates from the
   workspace (they live on in the copy and at the tag), move the recomp docs
   to `docs/recomp/`, and create the new crates. Update `AGENTS.md`,
   `README.md` and the progress docs for the new goal.
3. Toolchain: the `i686-pc-windows-gnu` Rust target, built inside the
   `gaming` distrobox (it has i686 MinGW, Wine 11.20 and 32-bit SDL3); the
   SDL3 MinGW i686 `SDL3.dll`; a dedicated, disposable Wine prefix for tests.
4. Baseline: run the user's game in a clean prefix with the system DLLs and
   confirm the known state (menus, a stage, audio). This is the reference to
   compare against.

### Phase 1: launcher and observation proxies

1. `CMRLauncher.exe`: game path in, edition check, runtime folder, registry,
   launch. A basic window with a folder picker and a Play button (SDL3), plus a
   command line for scripting. Tested under Proton/Wine here; Windows-native
   testing when a Windows machine is available.
2. Logging proxy DLLs: forward to the system `ddraw`, `qmixer`, `winmm`,
   `dinput` and `dplayx`, and log every call and argument. This is the runtime
   check of the API inventory (`.work/api-surface-notes.md`) and the spec for
   phase 2+.
3. The in-memory import-table fix for the CD and version checks, so neither
   XP mode nor a CD path is needed.

**Exit:** the game launches from the user's folder through the launcher
alone: no manual registry, no XP mode, no third-party patch.

### Phase 2: the easy replacements

- `dplayx.dll` stub.
- `winmm.dll` proxy with SDL3 gamepads: analog steering and triggers, which
  fixes the dead joystick path.
- `qmixer.dll` on SDL3 audio, checked against recorded proxy logs and by ear.
- `dinput.dll`: force feedback via SDL3 haptics where the device supports it.

**Exit:** the full game plays with a real gamepad, our audio, and the
system's DirectDraw/Direct3D still in place.

### Phase 3: `ddraw.dll` on SDL3 + OpenGL

1. DirectDraw: display mode, cooperative level, surfaces kept in memory
   (Lock/Unlock, Blt/BltFast with colour key, Flip), palettes, and an SDL
   window that the game's own HWND is bound to. The front end is mostly 2D
   blits, so it comes first.
2. Direct3D 5: device and viewport, textures loaded from surfaces, render
   states, transforms, `DrawPrimitive`/`DrawIndexedPrimitive` for LVERTEX and
   TLVERTEX, the Z-buffer, fog.
3. Present at the game's own 640x480/800x600, scaled to the window with
   correct 4:3 aspect (letterboxed), and vsync.

4. Movies: the `winmm` proxy takes over MCI AVI playback with our own
   decoders (MS Video 1, then Indeo 5).

**Exit:** menus, every stage, the school and the movies render correctly,
matching the wined3d baseline in side-by-side captures, with nothing installed
besides our package.

### Phase 4: improvements (opt-in settings, the faithful mode stays default)

- **Internal resolution up to 4K.** Render the 3D pass at N× (projection
  unchanged), scale TLVERTEX/2D coordinates, and filter the HUD.
- **Widescreen (Hor+).** Widen the projection in the wrapper. The game culls
  terrain cells and objects with its own view angle (recovered in the decomp:
  `c3d_terrain_cull`, `c3d_camera`), so its cull constants also need an
  in-memory patch. The HUD keeps a 4:3 layout or gets re-anchored.
- **60 fps.** Physics runs at a fixed 25 Hz (or 50 Hz with the double-rate
  option). Smooth motion needs either the game's existing 50 Hz mode or
  render-side interpolation; both need study (the decomp has the frame
  driver, `FUN_00443e90`).
- Texture filtering, MSAA, borderless or windowed modes, frame pacing.

### Phase 5 (optional): no Wine on Linux

A `wibo`-style native loader: maps `Rally.exe`, provides the 168 imports
itself (backed by SDL3), and loads our DLLs. One native Linux binary, no
prefix. Only worth doing once phases 1-4 are solid.

## Verification

- Shim logic (render-state translation, surface blits, colour keys, mixer
  maths) is unit-tested on the host, independent of Wine.
- Phase 1's proxy logs are recorded call traces. Later phases replay the same
  scenarios and compare call results and frames.
- Every phase compares screenshots with the wined3d baseline at the native
  resolution.
- Runtime evidence goes into the ledger as `run.*` rows, kept separate from
  static `rea.*` rows.

## Risks

- **GDI drawing mixed with DirectDraw.** If the game draws some of its 2D
  through GDI, the wrapper has to capture or emulate it; the inventory will
  tell.
- **Direct3D 5 corner cases** (execute buffers, ramp devices) are only a risk
  if the game falls back to them. The decomp says it uses the DrawPrimitive
  path.
- **Indeo 5 movies** need our own decoder to stay self-contained; it's the
  largest single piece of new code outside `ddraw.dll`.
- **Original CD layout.** Extracting from the 1998 CD or ISO needs its
  installer's archive format studied; we only have an installed/portable copy
  here. Installed folders are supported first.
- **No Windows machine here.** Windows-native testing waits on a Windows box;
  Wine is the test platform.
