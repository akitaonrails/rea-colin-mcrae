# Compatibility package: plan, inventory and status

This is the living status page. The **Status** section at the top says where
the work is; the **Inventory** below lists everything the goal depends on, how
each piece is covered and how sure we are. Update both whenever something
changes.

## Status (2026-11-02)

**Current milestone:** M1 (feasibility inventory and probes). No product code
yet. The repository has been restructured: the source reconstruction is parked
at tag `recomp-parked-2026-11-02` and copied in full (with its local notes) to
`/mnt/data/Projects/rea-colin-mcrae-recomp`.

| Milestone | What it proves | State |
| --- | --- | --- |
| M0 Reference preserved, repo restructured | nothing lost; clean start | **done** (`0783334`) |
| M1 Inventory + feasibility probes | every requirement has a covered, licence-checked path | **in progress** (items marked *probe* below) |
| M2 Runs as-is on Linux (Proton) via our launcher | the user's folder → playable game, no manual steps | not started |
| M3 Runs as-is on Windows 10/11 | same package natively | blocked: needs a Windows machine to test |
| M4 Gamepad, audio, movies complete | full experience, analog pad | not started |
| M5 Enhancements (opt-in): resolution, widescreen | 4K / 16:9 | not started; feasibility open |
| M6 Enhancement: 60 fps | smooth motion | not started; feasibility open |

**Last findings:**
- The official EU v1.5 `Rally.exe` starts under GE-Proton from a copied folder
  once its registry paths are set, and stops at its own "Wrong operating
  system" box. It accepts only the Windows 9x platform.
- With Wine reporting Windows 98, it passed the version gate and opened its
  window.
- The CD check and graphics are not yet confirmed: the virtual-display runs
  were black, and running Proton outside Steam's runtime container broke font
  and OpenGL init.

**Next:**
1. Settle the Linux runner (`umu-launcher` or Steam, inventory L1).
2. Finish probe G1 (does Proton's built-in renderer reach the menu and a
   stage).
3. Probes P1/P2: the in-memory version and CD fixes.

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
| R1 | Game files in place without the 1998 installer | Launcher builds a runtime folder (links/copies) from the user's folder | ours | **verified**: the game starts from a copied folder |
| R2 | Registry: `CDPath`, `HDPath`, `AVIPath`, `WAVPath`, `Language`, `Hardware`, `Initialised` under `Codemasters\Rally\1.00`; missing `Language`/`Hardware` read uninitialized memory, so always write them | Launcher writes them (Wine prefix HKLM; Windows VirtualStore under HKCU, no admin) | ours | **verified** on Proton (read by the game); Windows path: evidence (UAC docs) |
| R3 | Paths under ~127 characters (`OpenFile`) | Keep the runtime folder short (its own drive letter or a short root) | ours | evidence |
| R4 | OS gate: only the Win9x platform passes (`FUN_0049dcb0`) | In-memory patch for known hashes (P1); on Proton alone a per-app `win98` version also works | ours | **verified** with `win98`; the patch itself is *probe P1* |
| R5 | CD check: a CD-ROM drive containing `\setup\game\objects\cars\subday.c3d`, probed on almost every load (`FUN_00496ca0`); the files themselves come from `CDPath` | In-memory patch to "found" (P2); fallback: a CD-type drive letter (Wine), which has no Windows equivalent without admin | ours | *probe P2* |
| R6 | DirectDraw + Direct3D 5 (`DirectDrawCreate`; 8-bit 640x480 front end, 16-bit race, Ramp/HAL device, `DrawIndexedPrimitive`, back-buffer and Z-buffer reads each frame, `WaitForVerticalBlank` must succeed) | Linux: Proton's built-in ddraw/wined3d. Windows: one of G2 | wined3d (LGPL-2.1, part of Wine/Proton) | evidence (it runs a patched exe in the distrobox setup); *probe G1* in our setup |
| R7 | Audio: QSound `qmixer.dll` | It ships with the user's game and uses DirectSound, which Windows and Wine both provide; a replacement is only needed if it fails | the game's own file | evidence (distrobox setup plays with sound); *probe A1* |
| R8 | Joysticks via WinMM, with joystick ids taken from DirectInput 7 enumeration (no DirectInput means no pads) | Xidi: `winmm.dll` + `dinput.dll` presenting XInput pads; or the platform's own WinMM | Xidi (BSD-3-Clause) | *probe C1* (Xidi failed to load in the old distrobox setup; cause unknown) |
| R9 | Force feedback (DirectInput 7 effects) | Optional; the platform's DirectInput or Xidi | (platform) | optional |
| R10 | DirectPlay `dplayx.dll`, statically imported (the exe won't load without it) | Proton: built-in. Windows 10/11: the system DLL prompts to install the DirectPlay feature, so bundle Wine's `dplayx.dll` built for Windows or a small stub | Wine dplayx (LGPL-2.1) or ours | Proton: evidence; Windows: *probe D1* (needs Windows) |
| R11 | In-game movies (rally school): MCI `avivideo`, MS Video 1 | System codec: `msvidc32` on Windows and in Wine | (platform) | evidence |
| R12 | Intro movie: played by `Game.exe` (DirectShow, Indeo 5), not by `Rally.exe` | The launcher starts `Rally.exe` directly, as `Game.exe` does after the intro. The intro is optional: play it with an open-source decoder (Wine's `ir50_32` or FFmpeg), or skip it | Wine ir50_32 / FFmpeg (LGPL-2.1) | optional; *probe V1* |
| R13 | Display mode switching to 640x480/800x600 and scaling to modern screens | Proton scales low fullscreen modes to the display; on Windows the GPU driver does | (platform) | *probe G1* |
| R14 | Process quirks: realtime priority, `rdtsc` calibration, a 25/50 Hz timer thread, Debug CRT | Nothing to do; noted in case of trouble | n/a | evidence |

### Editions

| Executable (SHA-256) | What it is | What we need to do | State |
| --- | --- | --- | --- |
| `0eb6ea96…` | Official EU v1.5 (the decomp's reference) | R4 + R5 in-memory fixes | P1/P2 |
| `46500c8e…` | ChemicalFlood pack exe (the user's installed copy) | Already patched by the pack for R4/R5; run as-is | evidence |
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
| G1 | Does Proton's built-in renderer run the official exe through the menus and a stage? | Run via L1's runner with `win98`, then with P1/P2 | started: window opens; menu not yet seen |
| P1 | In-memory OS-gate fix works | Launcher: start suspended, patch `0x49dcb0` to "accept", resume | — |
| P2 | In-memory CD-check fix works | Patch `0x496ca0` to return 1 | — |
| A1 | QMixer audio plays under Proton | Listen / log during G1 | — |
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
- Runtime checks: run the game on a private virtual display (Xvfb), so test
  runs never take over the user's desktop. Capture frames from that display,
  and read the game's own debug output (it's a Debug build) through Wine's
  `debugstr` trace.
- Every probe result is recorded here (Probes table) and in
  `docs/RUNTIME_NOTES.md`.
