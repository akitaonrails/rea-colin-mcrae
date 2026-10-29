# Compatibility package: plan, inventory and status

This is the living status page. The **Status** section at the top says where
the work is; the **Inventory** below lists everything the goal depends on, how
each piece is covered and how sure we are. Update both whenever something
changes.

## Status (2026-11-02)

**Current milestone:** M2 (runs as-is via our launcher). Under Wine the
official exe is **playable** through `CMRLauncher.exe`, with plain Wine and
with Proton in Steam's runtime container: menus, stage load and driving with
the keyboard. Left for M2: the gamepad (C1). The repository has been restructured: the source reconstruction is parked
at tag `recomp-parked-2026-11-02` and copied in full (with its local notes) to
a separate local tree outside this repository.

| Milestone | What it proves | State |
| --- | --- | --- |
| M0 Reference preserved, repo restructured | nothing lost; clean start | **done** (`0783334`) |
| M1 Inventory + feasibility probes | every requirement has a covered, licence-checked path | **done** for M2's needs; remaining probes listed below |
| M2 Runs as-is on Linux (Proton) via our launcher | the user's folder → playable game, no manual steps | **in progress**: playable under Wine 11.20 and GE-Proton 11 (fresh prefixes, keyboard); gamepad (C1) open |
| M3 Runs as-is on Windows 10/11 | same package natively | blocked: needs a Windows machine to test |
| M4 Gamepad, audio, movies complete | full experience, analog pad | not started |
| M5 Enhancements (opt-in): resolution, widescreen | 4K / 16:9 | not started; feasibility open |
| M6 Enhancement: 60 fps | smooth motion | not started; feasibility open |

**Last findings:**
- **Playable under Wine 11.20**: `CMRLauncher.exe` takes the official,
  unpatched EU v1.5 exe from a brand-new prefix to the menus, a championship
  stage in New Zealand (3D, HUD, timer) and driving: throttle and steering on
  the default keyboard keys move the car (21 mph in first gear, turning).
- Stage loading works with Wine's default Windows version (no XP mode), so
  the community pack's "Win7+ stage crash" (S1) doesn't show here.
- Sound plays at the menu and in the race (private null sink recordings).
- A save copied from the user's earlier setup kept the car still: its control
  settings point player 1 at another device (most likely a gamepad Wine now
  sees). A fresh save uses the keyboard. Gamepad handling is probe C1.

- **Proton:** GE-Proton 11-1 inside Steam's sniper runtime container (the way
  Steam runs a non-Steam game) runs the launcher and the game: menus,
  New Zealand stage 1, driving at 42 mph. On this machine it needed
  `PROTON_USE_WOW64=1`, because the gaming container's 32-bit
  `libnvidia-glsi` is a bind mount of the host's 64-bit file, so Steam's
  runtime finds no 32-bit OpenGL driver; without it wined3d gets no GL
  context, DirectDraw has no 3D and the game crashes at `0x4a69fc`. A host
  with proper 32-bit drivers shouldn't need it.
- Under Proton the Codemasters/legal screen shows black (it rendered under
  plain Wine); the menus and races render normally.

- **Portrait main screen crashes the game** (seen on the developer's desktop:
  DP-2 rotated, listed first by XWayland). Wine then enumerates every mode
  rotated (480x640, ...); the game keeps only 320x240..800x600 modes, finds
  no 16-bit one, reads an empty table slot (`SetDisplayMode(0x80000000, 0)`)
  and crashes in `AddAttachedSurface` on a null surface. A Wine virtual
  desktop inherits the rotation and doesn't help. Running inside gamescope
  (its own landscape 640x480 screen, scaled to fit) works. Real Windows with
  a portrait primary probably fails the same way. Launcher fix: open (R15).
- Closing the window now quits the game (F6).
- Trees were once seen drawn over hills in a race (user screenshot); not
  reproduced since. The game toggles `ZENABLE` per draw; to investigate when
  it shows again (R16).

- **Gamepads (C1), findings.** The game enumerates pads with DirectInput and
  reads them with WinMM `joyGetPosEx`: X, Y and buttons only. A modern pad
  arrives through Wine's WinMM as X/Y = left stick, Z = both triggers on one
  axis (R2 low, L2 high), R/U = right stick, POV = d-pad, buttons 1..10
  (Xbox order; a Nintendo-layout 8BitDo reports A/B as 2/1). Three game-side
  problems, none specific to one machine:
  1. Only the left stick drives (Y up = throttle, down = brake); triggers
     and the right stick are never read. Fixed by F7 (optional,
     `gamepad_axes`).
  2. Control Setup re-probes the pads every frame and clamps CONTROL METHOD
     to the pads that answered; Wine briefly reports `JOYERR_UNPLUGGED`
     (about 2% of reads in a trace, likely its periodic device rescan or a
     dozing wireless pad), so the choice falls back to KEYBOARD or below,
     and OK then saves device 0 with the joystick's type. Fixed by F8.
  3. With that save the game reads the keyboard record as an analog
     joystick (key codes as axes): full lock and full brake, the car
     circles in reverse, the keyboard does nothing. Fixed by F9, which also
     repairs such saves at start-up.
  Verified headless with a virtual Xbox-style pad (uinput, harness only):
  the broken save no longer circles (F9); the pad steers and L2 brakes.
  Still open (C2-C6 in the probes table): R2/right-stick throttle not yet
  confirmed on a real pad; Control Setup -> Analogue -> Button Setup can
  loop back to Control Setup with no way out by keyboard (looked like a
  freeze); Button Setup's capture recorded the wrong control twice; RUN
  JOYSTICK PANEL deactivates the game and opens `joy.cpl`, which cannot
  show inside gamescope (the game then waits); the d-pad is never read.
- **Resolution (E1), research.** Each race re-creates DirectDraw at the
  RESOLUTION setting (`cfg[0xb]`, 4 slots up to 800x600; call `0x421ea9`),
  while the front end always runs at 640x480. A race above 800x600 is a
  small in-memory patch (return W x H for the 640x480 slot at
  `0x497d43`), but the in-race HUD uses absolute 640/800 tables and fonts,
  and the projection assumes 4:3 (`FUN_00435d10`), so 16:9 needs a second
  patch. Forcing the whole game to 1440x1080 broke the front end (menu
  drawn in the corner, then a hang). Plan in the Enhancements table.

- **Gamepad verified (2026-11-02):** with F7-F9 the user played a full
  stage on the 8BitDo with analog steering, R2/L2 and the right stick, and
  drove the menus with the pad; no glitches. RUN JOYSTICK PANEL still hangs
  the game when clicked (C6).

- **Graphics (2026-11-02):** opt-in race resolution (F10) with a HUD that
  scales (F13, first use of our hook library `cmrhooks.dll`), Hor+ 16:9
  (F11) and longer draw distance (F12), verified headless up to
  3840x2160. Frame rate needs no patch (already interpolated). Open: a
  display path that scales the 640x480 front end while offering the race
  mode (Proton looks right; Wine's Wayland driver scales but showed black).

**Next:**
1. Graphics: scale the 640x480 front end (splash, menus) to the screen with
   our own stretched present (GRAPHICS.md); results pages at high
   resolution; confirm on a real screen.
2. Controllers: keep RUN JOYSTICK PANEL from hanging (C6), the Control
   Setup loop and binding capture (C3, C4), d-pad and A/B swap (C5).
2. R15: handle a portrait main screen in the launcher (give the game a
   landscape mode list or a clear message).
3. E1: race resolution as an opt-in `resolution` setting (front end stays
   640x480), then the HUD scaling and the 16:9 projection.
4. Write the Linux how-to (Steam "Add a non-Steam game" + Proton; plain
   Wine) and package a release zip.
5. Windows 10/11 (M3) needs a Windows machine: G2 renderer and D1 dplayx.

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

Sources: the decomp ledger (the parked reconstruction's `EVIDENCE.md`, kept outside this repository), the API inventory
(`.work/api-surface-notes.md`), the local Wine setup in
`~/Projects/distrobox-gaming`, and the probes below.

| # | Requirement | How it's covered | Component (licence) | State |
| --- | --- | --- | --- | --- |
| R1 | Game files in place without the 1998 installer | Launcher copies the folder to `%LOCALAPPDATA%\CMRLauncher\Rally` (copies, not links: the game writes `save\`); later launches copy only files changed in the source; save files are never overwritten; creates `save\` when the source has none (the game can't create it) | ours | **verified** (launcher) |
| R2 | Registry: `CDPath`, `HDPath`, `AVIPath`, `WAVPath`, `Language`, `Hardware`, `Initialised` under `Codemasters\Rally\1.00`; missing `Language`/`Hardware` read uninitialized memory, so always write them | Launcher writes the paths every launch and seeds the game-owned values once (HKLM 32-bit view; when access is denied, the HKCU VirtualStore copy that UAC virtualization reads) | ours | **verified** under Wine; the Windows VirtualStore path: evidence (UAC docs) |
| R3 | Paths under ~127 characters (`OpenFile`) | Keep the runtime folder short (its own drive letter or a short root) | ours | evidence |
| R4 | OS gate: only the Win9x platform passes (`FUN_0049dcb0`) | In-memory fix F4 | ours | **verified** (community exe with the same fix runs) |
| R5 | CD check: a CD-ROM drive containing `\setup\game\objects\cars\subday.c3d`, probed on almost every load (`FUN_00496ca0`); the files themselves come from `CDPath` | In-memory fix F1 | ours | **verified** (same) |
| R6 | DirectDraw + Direct3D 5 (`DirectDrawCreate`; 8-bit 640x480 front end, 16-bit race, Ramp/HAL device, `DrawIndexedPrimitive`, back-buffer and Z-buffer reads each frame, `WaitForVerticalBlank` must succeed) | Linux: Wine/Proton's built-in ddraw/wined3d (with F2/F3). Windows: one of G2 | wined3d (LGPL-2.1, part of Wine/Proton) | **verified** under Wine (front end and race, 640x480 and 800x600); Windows: G2 |
| R7 | Audio: QSound `qmixer.dll` | It ships with the user's game and uses DirectSound, which Windows and Wine both provide; a replacement is only needed if it fails | the game's own file | **verified** under Wine (menu and race) |
| R8 | Joysticks via WinMM, with joystick ids taken from DirectInput 7 enumeration (no DirectInput means no pads) | The platform's own WinMM and DirectInput (Wine's map XInput-style pads to X/Y/Z/R/U + POV + buttons) plus fixes F7-F9; Xidi only if Windows needs it | (platform); Xidi (BSD-3-Clause) as fallback | Wine: enumeration and reads work (traced); see C1 |
| R9 | Force feedback (DirectInput 7 effects) | Optional; the platform's DirectInput or Xidi | (platform) | optional |
| R10 | DirectPlay `dplayx.dll`, statically imported (the exe won't load without it) | Proton: built-in. Windows 10/11: the system DLL prompts to install the DirectPlay feature, so bundle Wine's `dplayx.dll` built for Windows or a small stub | Wine dplayx (LGPL-2.1) or ours | Proton: evidence; Windows: *probe D1* (needs Windows) |
| R11 | In-game movies (rally school): MCI `avivideo`, MS Video 1 | System codec: `msvidc32` on Windows and in Wine | (platform) | evidence |
| R12 | Intro movie: played by `Game.exe` (DirectShow, Indeo 5), not by `Rally.exe` | The launcher starts `Rally.exe` directly, as `Game.exe` does after the intro. The intro is optional: play it with an open-source decoder (Wine's `ir50_32` or FFmpeg), or skip it | Wine ir50_32 / FFmpeg (LGPL-2.1) | optional; *probe V1* |
| R13 | Display mode switching to 640x480/800x600 and scaling to modern screens | Proton scales low fullscreen modes to the display; on Windows the GPU driver does | (platform) | *probe G1* |
| R14 | Process quirks: realtime priority, `rdtsc` calibration, a 25/50 Hz timer thread, Debug CRT | Nothing to do; noted in case of trouble | n/a | evidence |
| R15 | A portrait main screen: Wine (and likely Windows) lists every mode rotated, the game finds no usable mode and crashes | Launcher: give the game a landscape mode list, or stop with a clear message; workaround: gamescope | ours | **seen**; open |
| R16 | Depth order: trees once drawn over hills in a race | Find whether it is the game's own draw order or the Z-buffer under wined3d | n/a | seen once; open |

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
| F6 | `0x49de78` (WndProc jump-table entry for `WM_CLOSE`) | the window procedure swallows `WM_CLOSE`, so Alt+F4 or the window manager's close leaves the game running full screen; under gamescope the window looks frozen | on `WM_CLOSE` set the main loop's quit flag `0x96efe8`; the game shuts down through its own teardown (also applied to the community exe, which lacks it) | **verified** (Wine, gamescope on Hyprland: closes in ~1 s; during a load it quits when the load ends) |
| F7 | `0x49ce1b`, `0x49ce6f` (analog joystick read `FUN_0049cdf0`) | the game reads X/Y/buttons only, so on a modern pad only the left stick drives | also request Z and R; feed the game's Y with whichever of left-stick Y, Z (triggers) or R (right stick) is furthest from centre, for axes the launcher found present and centred at start (pedals are left out); optional (`gamepad_axes`) | **verified** (user, 8BitDo Ultimate 2 under Wine + gamescope: a full stage driven with analog steering, triggers and right stick; menus by pad) |
| F8 | `0x49d60e` (joystick probe `FUN_0049d5c0`) | Control Setup clamps CONTROL METHOD to the pads that answered this frame; a transient `JOYERR_UNPLUGGED` under Wine drops it to KEYBOARD or below | a pad that answered once stays present for the session (sticky mask in the fix memory) | **verified** (JOYSTICK 1 setting kept; menus and race on the pad) |
| F10 | `0x497d43` (race's 16-bit mode-slot load) | races offer at most 800x600 | slot 2 (the menu's 640x480) returns the configured `resolution`; front end unchanged; opt-in | **verified** at 1440x1080 and 1920x1080 (Wine, gamescope) |
| F13 | race states keep a virtual 640x480 in the display state; real sections: display set-up `0x497be0`, 3D set-up `0x4a6950`, clears `0x49ffc0`/`0x49ff80`, projection `0x435d10`, scene draw `0x49fe30`; updates `0x443db0` (timer thread) and real sections share one lock; 2D primitives: queues `0x49b420`/`0x49b1d0`, blit `0x49e3e0`, panel `0x49e200`; race HUD calls marked for corner anchoring | above the game's own sizes the in-race 2D mixes coordinate spaces: the rev dial stays at its 640x480 pixel position, text stays at font size, messages, results and the loading screen draw 640x480-sized; the results table, laid out on the update thread, fell off screen | the whole race sees the 640x480 it was made for, every 2D rectangle is scaled onto a centred 4:3 area (full-screen backdrops span the screen); only display set-up and 3D see the real size; logic in `cmrhooks.dll` (`sections`, `canvas`); applied with F10 | **verified** headless at 1920x1080 and 3840x2160: menus, loading, HUD in the corners, pause, quit to menu, stage results (finish forced in a test); STAGE COMPLETE seen by the user |
| F11 | `0x435d44` (projection `xs` aspect term), `0x435d87` (`ys` factor) | the game projects in software to `x·xs·W/2`, `y·ys·H/(2A)` with `xs` scaled by A = W/H: a 16:9 race comes out 1.78x wider and 0.75x shorter than 4:3 | Hor+: `xs` aspect term (4/3)²/A, `ys` × A/(4/3), so the pixel scale is a 4:3 screen's of the same height; identical at 4:3; opt-in `widescreen` | **verified** at 1920x1080: the centre matches the 1440x1080 picture exactly, the sides show more |
| F12 | `0x4809a7` (after the stage's `.ars` load), `0x403ea7` (section-trigger table read), cap immediates `0x435e91/98`, `0x435f6a/71` | the view distance is set by stage data (usually 150 m) | multiply the `.ars` object/far/LOD values, each trigger value and the 200 m cap by `draw_distance`; the game's own menu factors still apply; opt-in | **verified** (x2.0 race: far plane and object wedge 300 m in memory) |
| F14 | `0x449714` (race pause check, `GetAsyncKeyState(VK_ESCAPE)`) | a pad has no pause | Escape also reads as held while the player's pad holds button 8 (Start); the game's latch and pause menu do the rest; with `gamepad_axes` | **verified** headless (virtual pad: Start opens GAME PAUSED, clock stops); resuming is CONTINUE, Start again does not close it |
| F15 | `0x4ad240` (CPU clock check) | at real-time priority it busy-waits 1 s of QPC counting `rdtsc` in 32 bits and retries on wrap; above ~2 GHz it wraps, so start-up (and exit) wait several seconds; called 3 times | return 1 GHz at once (only compared with 198 MHz for default detail, and printed by the exit profiler) | **verified**: main menu at ~6 s after start |
| F16 | `0x497db9` (front-end mode set), `0x498705` (front-end `Flip`), `0x4c1e5f` (legal screen) | the intro and menus are drawn at 640x480 and sat small in a corner at high resolutions | the front end runs in the race resolution's screen mode while the game keeps drawing 640x480; `cmr_fe_present` copies each frame to the screen enlarged (nearest neighbour, centred 4:3, 16- to 32-bit as needed); with F13 | **verified** headless 1920x1080 and 3840x2160: legal screen, menus, service screen, race and back. Menus ~90 fps at 1080p and ~27 fps at 4K (Wine's software renderer uploads the whole screen per frame); next: scale on the GPU with point sampling |
| F9 | `0x49d67d` (device-type setter `FUN_0049d670`) | a joystick type written to keyboard record 0 (from F8's case, or a save made before F8) makes the game read key codes as joystick axes: the car circles in reverse | records 0 and 1 always keep type 1 (keyboard); repairs such saves at start-up | **verified** headless (broken save: car waits at the start instead of circling) |

### Editions

| Executable (SHA-256) | What it is | What we need to do | State |
| --- | --- | --- | --- |
| `0eb6ea96…` | Official EU v1.5 (the decomp's reference) | F1-F9 in memory | **verified**: menu via the launcher |
| `46500c8e…` | ChemicalFlood pack exe (the user's installed copy) = EU v1.5 + F1-F5 on disk | F6-F9 in memory | **verified** under Wine 11.20 |
| `b445fd0d…` | `Game.exe` (1998 launcher/intro player) | Not needed to play | evidence |
| others (US v1.5, v1.0 CD) | Unknown until users bring them | Per-hash patch tables | open |

Installing straight from the original CD or its ISO needs that installer's
archive format studied; there is no CD image here, so installed or portable
folders come first.

### Linux runner

| # | Need | Option | State |
| --- | --- | --- | --- |
| L1 | Run our Windows launcher under Proton without hand-built environments | The user adds `CMRLauncher.exe` to Steam as a non-Steam game with Proton (Steam runs it in the sniper container); plain `wine` also works; `umu-launcher` (GPL-3.0) is the no-Steam route | **verified**: GE-Proton 11-1 in the sniper container plays (this machine needed `PROTON_USE_WOW64=1`, see Status); plain Wine 11.20 plays |
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
| E1 | Higher race resolution (1080p, aiming at 4K) — **done** with F10 + F13 (HUD) and F11 (16:9); 4K run still to check | The race re-creates DirectDraw at the RESOLUTION slot (`0x421ea9`; front end stays 640x480). (a) In-memory: hook the 16-bit slot load at `0x497d43` to return the configured W x H for slot 2 (640x480 fonts and HUD tables match), bypassing the 800x600 enumeration filter; then scale the absolute HUD tables (`0x45d0fd`, fonts by width) and add a Hor+ 16:9 projection (`0x435d44`, viewport `0x4a6f15`/`0x4a6f6f`). (b) Alternative: keep 640x480 and supersample in the renderer, if a bundled renderer offers it. Risks: `SetDisplayMode` failures are ignored (the mode must exist: gamescope/Proton provide it), culling tuned for 4:3, CPU locks at 4K | researched; to implement as opt-in `resolution` |
| E4 | Draw distance | Scale the stage's far plane, object and LOD wedges and the section-trigger table by an opt-in factor (GRAPHICS.md) | **done** (F12); visual check on a long straight and the frame-rate cost still to judge |
| E2 | Widescreen | Widen the projection (renderer side) **and** patch the game's own view-culling constants in memory (`c3d_terrain_cull`/`c3d_camera` in the decomp); re-anchor or keep the HUD | evidence for where to patch; **risk** until E1's renderer is chosen |
| E3 | 60 fps — **not needed**: races already render decoupled from the 25 Hz tick and interpolate (190 fps measured headless at 1080p); vsync via Flip | Physics is a fixed 25 Hz tick (50 Hz option), driven by a WinMM timer thread in races; smooth 60 fps needs render-side interpolation inside the game loop | **risk**: needs a design study of the frame driver (`FUN_00443e90`) |
| E4 | Texture filtering, MSAA, borderless window | Renderer settings, if the chosen renderer exposes them | open |

### Probes

| Id | Question | Method | Result |
| --- | --- | --- | --- |
| G1 | Does Wine's built-in renderer run the game? | Headless gamescope + Wine 11.20 | **yes** with F1-F5 (title, service screen); official exe without F2 crashes at the Ramp device |
| P1 | Launcher applies F1-F5 in memory (start suspended, write, resume) | `CMRLauncher.exe` under Wine 11.20 | **yes**: official exe reaches the main menu |
| A1 | QMixer audio plays | Record a private null sink during a run | **yes** at the menu and in the race (Wine 11.20) |
| S1 | Stage loading under NT-family Windows versions (community pack: crashes on Win7+ without XP compatibility mode) | Load a stage with the default Wine version; if it crashes, find the cause | **no crash** under Wine 11.20 (two stages: New Zealand, Greece); real Windows still to see (M3) |
| C1 | Analog pad works via WinMM under Wine/Proton | `CMRLauncher --controllers` (lists pads, logs every axis/button), then drive | **verified** under Wine with F7-F9 (real pad, full stage); Proton and Windows still to check |
| C2 | R2/L2 and right stick drive with F7 on a real pad | user test with an Xbox-layout and a Nintendo-layout pad | **verified** (Nintendo-layout 8BitDo); Xbox-layout pad to check |
| C3 | Control Setup -> Analogue -> Button Setup loops back with no exit | trace the menu parent chain (`0xd445e4`) | open |
| C4 | Button Setup capture records a direction/the wrong button | trace capture (`state::CAPTURE`) edges vs F7's flags | open |
| C5 | d-pad (POV) not read; A/B order differs per pad | map POV to the digital flags in F7; optional A/B swap setting | open |
| C6 | RUN JOYSTICK PANEL hangs under gamescope | the game deactivates itself and opens `joy.cpl`; skip it (in memory) when no desktop can show it | **seen again** by the user (accidental click hangs the game); open |
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
