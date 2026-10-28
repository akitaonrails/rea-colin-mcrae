# Compatibility package: plan, inventory and status

This is the living status page. The **Status** section at the top says where
the work is; the **Inventory** below lists everything the goal depends on, how
each piece is covered and how sure we are. Update both whenever something
changes.

## Status (2026-11-02)

**Current milestone:** M2 (runs as-is via our launcher). `CMRLauncher.exe`
exists and brings the official exe to the main menu under Wine. The repository has been restructured: the source reconstruction is parked
at tag `recomp-parked-2026-11-02` and copied in full (with its local notes) to
`/mnt/data/Projects/rea-colin-mcrae-recomp`.

| Milestone | What it proves | State |
| --- | --- | --- |
| M0 Reference preserved, repo restructured | nothing lost; clean start | **done** (`0783334`) |
| M1 Inventory + feasibility probes | every requirement has a covered, licence-checked path | **done** for M2's needs; remaining probes listed below |
| M2 Runs as-is on Linux (Proton) via our launcher | the user's folder → playable game, no manual steps | **in progress**: menu reached from a fresh Wine prefix; a stage, input and the Proton runner still to check |
| M3 Runs as-is on Windows 10/11 | same package natively | blocked: needs a Windows machine to test |
| M4 Gamepad, audio, movies complete | full experience, analog pad | not started |
| M5 Enhancements (opt-in): resolution, widescreen | 4K / 16:9 | not started; feasibility open |
| M6 Enhancement: 60 fps | smooth motion | not started; feasibility open |

**Last findings:**
- `CMRLauncher.exe` (Rust, 32-bit Windows, `crates/cmr-launcher`) runs the
  **official, unpatched** EU v1.5 exe from a brand-new Wine 11.20 prefix with
  no manual setup: it copies the game to a private runtime folder, writes the
  registry, starts `Rally.exe` suspended, applies F1-F5 in memory (each site
  checked against the file first) and resumes it. The game reached the
  Codemasters screen and the main menu.
- Audio plays (QSound under Wine): recorded from a private null sink at the
  menu.
- Second launch copies nothing and leaves the game's own registry values
  alone.

**Next:**
1. Drive the menus into a stage on the headless display (needs scripted key
   input), check probe S1 (stage-load crash on Win7+ reported by the
   community pack).
2. Settle the Linux runner (L1) and run the launcher under Proton.
3. Probe C1 (gamepad).

## Goal and ground rules

Run the user's own copy of the PC `Rally.exe`, unmodified on disk, on modern
Windows and on Linux (Proton/Wine). The user installs our package and points
it at the game; nothing else.

- **Self-contained:** no third-party patches, wrappers or codec packs to
  fetch, no compatibility modes, no manual registry edits.
- **Windows first, easy on Linux:** Windows binaries that run under Proton
  with no extra setup.
- **Reuse open source:** bundle maintained open-source components where they
  exist; write only the glue and the game-specific fixes.
- **No game files** in the repository or the package; fixes are applied in
  memory, keyed to the executable's hash.

## Inventory

Legend for **State**:
- **verified**: seen working;
- **evidence**: known from static analysis or prior local use, not yet
  checked in our setup;
- **probe**: open question with a defined experiment;
- **risk**: no known solution yet.

### What the game needs

Sources: the decomp ledger (`docs/recomp/EVIDENCE.md`), the API inventory
(`.work/api-surface-notes.md`), the local Wine setup in
`~/Projects/distrobox-gaming`, and the probes below.

| # | Requirement | How it's covered | Component (licence) | State |
| --- | --- | --- | --- | --- |
| R1 | Game files in place without the 1998 installer | Launcher copies the folder to `%LOCALAPPDATA%\CMRLauncher\Rally` (copies, not links: the game writes `save\`); later launches copy only files changed in the source; save files are never overwritten | ours | **verified** (launcher) |
| R2 | Registry: `CDPath`, `HDPath`, `AVIPath`, `WAVPath`, `Language`, `Hardware`, `Initialised` under `Codemasters\Rally\1.00`; missing `Language`/`Hardware` read uninitialized memory, so always write them | Launcher writes the paths every launch and seeds the game-owned values once (HKLM 32-bit view; when access is denied, the HKCU VirtualStore copy that UAC virtualization reads) | ours | **verified** under Wine; the Windows VirtualStore path: evidence (UAC docs) |
| R3 | Paths under ~127 characters (`OpenFile`) | Keep the runtime folder short (its own drive letter or a short root) | ours | evidence |
| R4 | OS gate: only the Win9x platform passes (`FUN_0049dcb0`) | In-memory fix F4 | ours | **verified** (community exe with the same fix runs) |
| R5 | CD check: a CD-ROM drive containing `\setup\game\objects\cars\subday.c3d`, probed on almost every load (`FUN_00496ca0`); the files themselves come from `CDPath` | In-memory fix F1 | ours | **verified** (same) |
| R6 | DirectDraw + Direct3D 5 (`DirectDrawCreate`; 8-bit 640x480 front end, 16-bit race, Ramp/HAL device, `DrawIndexedPrimitive`, back-buffer and Z-buffer reads each frame, `WaitForVerticalBlank` must succeed) | Linux: Proton's built-in ddraw/wined3d. Windows: one of G2 | wined3d (LGPL-2.1, part of Wine/Proton) | evidence (it runs a patched exe in the distrobox setup); *probe G1* in our setup |
| R7 | Audio: QSound `qmixer.dll` | It ships with the user's game and uses DirectSound, which Windows and Wine both provide; a replacement is only needed if it fails | the game's own file | **verified** at the menu under Wine |
| R8 | Joysticks via WinMM, with joystick ids taken from DirectInput 7 enumeration (no DirectInput means no pads) | Xidi: `winmm.dll` + `dinput.dll` presenting XInput pads; or the platform's own WinMM | Xidi (BSD-3-Clause) | *probe C1* (Xidi failed to load in the old distrobox setup; cause unknown) |
| R9 | Force feedback (DirectInput 7 effects) | Optional; the platform's DirectInput or Xidi | (platform) | optional |
| R10 | DirectPlay `dplayx.dll`, statically imported (the exe won't load without it) | Proton: built-in. Windows 10/11: the system DLL prompts to install the DirectPlay feature, so bundle Wine's `dplayx.dll` built for Windows or a small stub | Wine dplayx (LGPL-2.1) or ours | Proton: evidence; Windows: *probe D1* (needs Windows) |
| R11 | In-game movies (rally school): MCI `avivideo`, MS Video 1 | System codec: `msvidc32` on Windows and in Wine | (platform) | evidence |
| R12 | Intro movie: played by `Game.exe` (DirectShow, Indeo 5), not by `Rally.exe` | The launcher starts `Rally.exe` directly, as `Game.exe` does after the intro. The intro is optional: play it with an open-source decoder (Wine's `ir50_32` or FFmpeg), or skip it | Wine ir50_32 / FFmpeg (LGPL-2.1) | optional; *probe V1* |
| R13 | Display mode switching to 640x480/800x600 and scaling to modern screens | Proton scales low fullscreen modes to the display; on Windows the GPU driver does | (platform) | *probe G1* |
| R14 | Process quirks: realtime priority, `rdtsc` calibration, a 25/50 Hz timer thread, Debug CRT | Nothing to do; noted in case of trouble | n/a | evidence |

### Game-specific fixes (applied in memory by the launcher)

Derived from the decomp and confirmed by the community-patched exe, which is
the official EU v1.5 plus exactly these changes. We reimplement each fix
ourselves and apply it in memory; the file on disk is never changed.

| # | Address (EU v1.5) | Problem | Fix | State |
| --- | --- | --- | --- | --- |
| F1 | `0x495f09`, `0x495f59` (calls to `FUN_00496ca0`) | CD-ROM probe on every load | treat the probe as found | **verified** (launcher, Wine) |
| F2 | `0x497cee` (`mov edi,8`) | the front end sets an 8-bit palettized mode and creates a Ramp 3D device; Wine's renderer and modern Windows have neither, so `CreateDevice` fails and the game dereferences null (`0x4a69ff`, seen) | use a 16-bit mode | **verified** (launcher, Wine) |
| F3 | `0x497df5` (`jne`) | with F2, the GDI system-palette block (8-bit only) would run in 16-bit | run it only when a palette is supplied (extra test of the caller's palette argument) | **verified** (launcher, Wine) |
| F4 | `0x49dcb0` | Win9x-only OS gate | accept | **verified** (launcher, Wine) |
| F5 | `0x4ae590..0x4ae5ac` | reads the VGA palette with `in`/`out` port I/O, a privileged instruction on NT-family Windows | skip the port reads | **verified** (launcher, Wine) |

### Editions

| Executable (SHA-256) | What it is | What we need to do | State |
| --- | --- | --- | --- |
| `0eb6ea96…` | Official EU v1.5 (the decomp's reference) | F1-F5 in memory | **verified**: menu via the launcher |
| `46500c8e…` | ChemicalFlood pack exe (the user's installed copy) = EU v1.5 + F1-F5 on disk | run as-is | **verified** under Wine 11.20 |
| `b445fd0d…` | `Game.exe` (1998 launcher/intro player) | Not needed to play | evidence |
| others (US v1.5, v1.0 CD) | Unknown until users bring them | Per-hash patch tables | open |

Installing straight from the original CD or its ISO needs that installer's
archive format studied; there is no CD image here, so installed or portable
folders come first.

### Linux runner

| # | Need | Option | State |
| --- | --- | --- | --- |
| L1 | Run our Windows launcher under Proton without hand-built environments | `umu-launcher` (GPL-3.0) runs Proton inside Steam's runtime container outside Steam, or the user adds the launcher to Steam as a non-Steam game | *probe*: our manual `proton run` outside the container broke FreeType/OpenGL init |
| L2 | Dual-GPU laptops pick the right GPU | Steam/umu handle PRIME; document the override | evidence (needed in the distrobox setup) |

### Graphics on Windows (G2)

Probe in this order and pick the first that works. All are Windows DLLs
placed next to `Rally.exe`.

| Option | Licence | Notes | State |
| --- | --- | --- | --- |
| Wine's `ddraw.dll` + `wined3d.dll` built for Windows | LGPL-2.1 | The same renderer as Proton, so one behaviour on both platforms; Windows builds are known to exist | *probe* |
| dxwrapper (DirectDraw/D3D 1-7 → D3D9) | zlib | Windows-focused; D3D5 coverage to confirm | *probe* |
| D7VK (D3D 3/5/6/7 → Vulkan) | zlib | D3D5 since 1.3; built for Wine, Windows use unconfirmed | *probe* |
| Windows' own DirectDraw | n/a | Community reports problems on Win10/11 | fallback |

Excluded: dgVoodoo2 (closed source; crashes under Wine), cnc-ddraw
(DirectDraw 2D only).

### Enhancements (opt-in, after M2/M3)

| # | Goal | Approach | State |
| --- | --- | --- | --- |
| E1 | Higher internal resolution (up to 4K) | Needs a renderer that renders the 3D above the game's mode: check wined3d, dxwrapper and D7VK for a resolution override; otherwise a thin DirectDraw layer of our own on top of the chosen open-source renderer | **risk**: no confirmed open-source path yet |
| E2 | Widescreen | Widen the projection (renderer side) **and** patch the game's own view-culling constants in memory (`c3d_terrain_cull`/`c3d_camera` in the decomp); re-anchor or keep the HUD | evidence for where to patch; **risk** until E1's renderer is chosen |
| E3 | 60 fps | Physics is a fixed 25 Hz tick (50 Hz option), driven by a WinMM timer thread in races; smooth 60 fps needs render-side interpolation inside the game loop | **risk**: needs a design study of the frame driver (`FUN_00443e90`) |
| E4 | Texture filtering, MSAA, borderless window | Renderer settings, if the chosen renderer exposes them | open |

### Probes

| Id | Question | Method | Result |
| --- | --- | --- | --- |
| G1 | Does Wine's built-in renderer run the game? | Headless gamescope + Wine 11.20 | **yes** with F1-F5 (title, service screen); official exe without F2 crashes at the Ramp device |
| P1 | Launcher applies F1-F5 in memory (start suspended, write, resume) | `CMRLauncher.exe` under Wine 11.20 | **yes**: official exe reaches the main menu |
| A1 | QMixer audio plays | Record a private null sink during a run | **yes** at the menu (Wine 11.20); in-race audio still to hear |
| S1 | Stage loading under NT-family Windows versions (community pack: crashes on Win7+ without XP compatibility mode) | Load a stage with the default Wine version; if it crashes, find the cause | — |
| C1 | Analog pad works via Xidi or WinMM under Proton | Plug a pad during G1, check the controller menu | — |
| D1 | `dplayx` on Windows 10/11 | Windows machine | blocked |
| V1 | Intro playback with an open-source decoder | After M2 | — |

## Architecture (to be confirmed by M1)

```text
CMRLauncher.exe (ours, Windows; also what Proton/umu runs on Linux)
  ├─ input: the user's game folder; identifies the exe by hash
  ├─ builds a short-path runtime folder from it; adds the bundled DLLs
  ├─ writes the registry values (R2)
  ├─ starts Rally.exe suspended, applies the in-memory fixes for that hash
  │  (R4, R5), resumes it
  └─ on Windows: also sets up the bundled renderer (G2) and dplayx (R10)

bundled open-source components (each listed in THIRD_PARTY.md with licence)
  renderer (G2, Windows only; Proton already has wined3d)
  Xidi (controllers, if C1 shows it's needed)
  dplayx (Windows only)
```

Our own code is the launcher (Rust), the patch tables and the packaging.
Enhancements may later add a thin layer of our own where no open-source
component offers the feature (E1-E3).

## Verification

- Host unit tests for the launcher's logic: edition detection, runtime-folder
  layout, registry values, patch tables.
- Runtime checks: run on a private headless gamescope display (real GPU) with
  sound routed to a private null sink, so test runs never reach the user's
  screen or speakers. Every run has a timeout, ends with `wineserver -k`, and
  is followed by a check for leftover processes. Capture frames with
  `gamescopectl screenshot`, sound with `parec` on the sink's monitor, and
  read the launcher's `launcher.log`.
- Every probe result is recorded here (Probes table) and in
  `docs/RUNTIME_NOTES.md`.
