# Runtime notes

Observations from running the user's own game. Each entry: date, setup, what
was seen. Static facts live in the parked reconstruction's documents (kept outside this repository) and local notes.

## 2026-11-02: first Proton runs (GE-Proton 11-1, `proton run` outside Steam)

Setup: a hard-linked copy of the user's installed game with `Rally.exe`
replaced by the official EU v1.5 executable (`0eb6ea96…`); fresh prefix;
registry values from `THIN_APP_PLAN.md` R2 imported with `regedit /S`.

- Run with the default Windows version: the game loads `QMIXER.dll` from its
  folder, then shows its own "Wrong operating system" message box (the Win9x
  platform gate, `FUN_0049dcb0`) and exits.
- With `HKCU\Software\Wine\AppDefaults\Rally.exe\Version = win98`: no message
  box. Wine loads `wined3d`; a window titled "Colin McRae Rally" (1261x1390,
  not fullscreen) appeared on the real display. The 32-bit Vulkan loader was
  missing (wined3d falls back to OpenGL). Without the NVIDIA PRIME variables,
  OpenGL context queries failed with `GL_OUT_OF_MEMORY`; with them they
  didn't.
- On a private Xvfb display every captured frame was black. With extra
  tracing, Wine reported it could not find FreeType. Running Proton by hand
  outside Steam's runtime container is not a usable test harness. Next: run
  through the runtime container (`umu-launcher`, or Steam's
  `SteamLinuxRuntime` entry point).
- A CD-type drive letter (`dosdevices\r:` → a folder with `setup\game` →
  the game's `Game`, `HKLM\Software\Wine\Drives "r:"="cdrom"`) was added for
  the CD probe; not yet confirmed because of the black frames.

## 2026-11-02: test harness and the fix list

- Xvfb is unusable: Wine's `ddraw.dll` page-faults (`ddraw+0x2d6a0`, read of
  `0x102`) on its software OpenGL, with both the official and the patched
  exe. Steam's runtime container (`_v2-entry-point`) on Xvfb showed nothing at
  all, not even `winecfg`.
- Headless gamescope (`gamescope --backend headless`, `gaming` distrobox,
  NVIDIA PRIME variables) runs Wine 11.20 on the real GPU without showing
  anything on the user's desktop; `gamescopectl screenshot <path>` captures
  the composited frame. (X11 root grabs are black there.)
- The ChemicalFlood exe (`46500c8e…`) reached the Codemasters screen and then
  the service screen from the user's save, at 640x480.
- The official EU v1.5 exe with `win98` and a CD-type drive crashed at
  `0x4a69ff`: `IDirect3D2::CreateDevice` for the front end's 8-bit Ramp device
  failed and the game used the null device.
- `cmp` of the two exes: exactly 87 bytes in 5 places, the fixes F1-F5 in
  `THIN_APP_PLAN.md`.

## 2026-11-02: first launcher runs

- `CMRLauncher.exe --game <official EU v1.5 folder>` in a fresh Wine 11.20
  prefix (no registry, no drive or version settings): copied 5167 files,
  wrote HKLM, applied F1-F5 (cave allocated at `0x00290000`), the game showed
  the Codemasters screen at ~20 s and the main menu at ~40 s.
- The second launch copied 0 files and reached the menu again.
- Sound was recorded from a private PipeWire null sink (`PULSE_SINK`): music
  at the menu, peak 8608. An earlier run went to the user's speakers; the
  harness now always uses the null sink.

## 2026-11-02: driving

- Key input on the private display: XTest via `.work/rt/xkeys.py` (live
  command file). Wine's `+key` trace shows the presses arriving as the right
  virtual keys (`VK_UP` 0x26, `VK_LEFT` 0x25, Escape).
- The game polls keys by level (`GetAsyncKeyState`, in race `FUN_0049cfb0`
  with key codes from the control settings), so a held key carries into the
  next screen and very short or mid-animation presses get lost; 60 ms
  presses with a pause after each work best.
- With the save copied from the earlier distrobox setup, Up/arrows did
  nothing in the race (gear showed R) while Escape opened the pause menu: its
  control settings select another device for player 1. A fresh save
  (no `save\` folder in the source) used the keyboard: the car drove (21 mph
  in first, steering left into a fence) in New Zealand stage 1.
- The game starts fine without a `save\` folder.
- Stage loads under Wine's default Windows version: New Zealand (800x600 from
  the old save, 640x480 fresh) and Greece.

## 2026-11-02: Proton in Steam's runtime container

- Harness: `.work/rt/run-gs-proton.sh` runs
  `SteamLinuxRuntime_sniper/_v2-entry-point --verb=waitforexitandrun --
  GE-Proton11-1/proton waitforexitandrun CMRLauncher.exe` inside headless
  gamescope in the `gaming` distrobox. It must start from `$HOME`: from a
  host path bwrap tries to enter `/run/host/...` and fails.
- First runs: the launcher worked (registry, copy, F1-F5) but the game died
  with c0000005 at `0x4a69fc`. Proton log: `wined3d_caps_gl_ctx_create Failed
  to find a suitable pixel format`, then `d3d_device_create ... the current
  DirectDrawRenderer does not support this`. `renderer=vulkan` gave the same
  refusal (no Direct3D 1-7 device on that backend here).
- Cause: inside the distrobox, `/usr/lib32/libnvidia-glsi.so.615.78.08` is a
  bind mount of the host's 64-bit `/usr/lib/libnvidia-glsi.so.615.78.08`;
  pressure-vessel's i386 capsule rejects it, so the 32-bit NVIDIA GLX/EGL
  libs are dropped. `PRESSURE_VESSEL_GRAPHICS_PROVIDER=/` didn't help.
- `PROTON_USE_WOW64=1` (32-bit code on 64-bit Unix libraries) fixed it:
  menus, stage, driving at 42 mph in first. Plain Wine 11.20 in the distrobox
  already runs this way.
- Under Proton the legal/Codemasters screen is black for its ~30 s; Enter
  skips it.

## 2026-11-02: save folder, menu input

- From a source folder without `save\`, the game ran but never wrote
  `save\cmRally.cfg`: it creates files with `OpenFile`, which doesn't create
  folders (the installer made it). The launcher now creates `save` in the
  runtime folder when no folder of that name exists; the next run wrote
  `cmRally.cfg` (7.7 KB).
- Idling 30 s on the main menu, the selected item never changed, so no
  connected device (an 8BitDo pad and a Moonlander keyboard's joystick
  interface are visible to Wine) drives the menu by itself. The menu's
  odd jumps come from the game polling key levels during its ring animation.

## 2026-11-02: first runs on the developer's desktop

- Wine 11.20 on Hyprland (XWayland), two 4K monitors, DP-2 rotated to
  portrait and listed first by XWayland, so Wine takes it as the main screen.
- The game crashed at start (`AddAttachedSurface`, read of `0x102`). Trace:
  Wine enumerated every mode as height x width (`480x640x16`, ...); the
  mode callback `0x4a66b0` keeps only widths 320..800 and heights 240..600,
  so no 16-bit mode survived, the selector read an empty slot and called
  `SetDisplayMode(0x80000000, 0, 16)`; the primary chain then failed and the
  game attached a null surface. The headless runs never saw it (gamescope
  offers landscape modes only).
- A Wine virtual desktop (`explorer /desktop=`) inherits the rotation and
  crashed the same way. gamescope (`-w 640 -h 480 -f -S fit`) works: the
  game fills the monitor at 4:3.
- The window procedure swallows `WM_CLOSE`; Super+W on the gamescope window
  left the game running and Hyprland flagged gamescope as not responding.
  Fix F6 sets the quit flag on `WM_CLOSE`: closing now ends the game in
  about a second (or when a load finishes) through its normal teardown.
- Played by the user: menus, a New Zealand stage, driving. One screenshot
  showed trees drawn over the hills; not seen again (R16).

## 2026-11-02: gamepads

- Pad: 8BitDo Ultimate 2 (USB). A desktop key mapper (AntiMicroX, from
  another project) was turning pad presses into keys; that is what made the
  pad "work" in KEYBOARD mode (stick as arrows, Start as Enter). With it
  stopped, the pad does nothing in KEYBOARD mode, as expected.
- `CMRLauncher --controllers` (WinMM, the game's API): one device "Wine
  joystick driver", 5 axes, 10 buttons, POV. Left stick X/Y; right stick U
  (across) and R (up = low); both triggers on Z (R2 low, L2 high); A/B/X/Y =
  buttons 2/1/4/3 on this pad (Nintendo order), L1 5, R1 6, Select 7,
  Start 8, L3 9, R3 10. A virtual Xbox 360 pad (uinput, `.work/rt/vpad.py`)
  maps the same way with A/B = 1/2.
- Control Setup has three screens: CONTROL METHOD / CONTROLLER TYPE, then
  ANALOGUE SETUP (steering, acceleration, braking: analogue or digital;
  force feedback), then BUTTON SETUP (handbrake 5, gear up 6, gear down 4,
  camera 3 by default). Settings reach `cmRally.cfg` only when leaving
  Options.
- The front end reads only the keyboard while player 1 is on the keyboard;
  once player 1 is on a pad, the pad drives the menus.
- Relay trace (`joyGetPosEx` only): the game polls every pad many times per
  frame; about 2% of reads returned `JOYERR_UNPLUGGED` (0xa7) with the pad
  connected. With JOYSTICK 2 chosen, the saved player block read device 0
  (keyboard) with type 2 (analogue): the clamp described under F8. A memory
  dump (winedbg) showed record 0 at type 2 with key codes in its axis
  slots; the car circled in reverse at ~21 mph with no input and ignored
  the keyboard. With F9 the same save starts normally.
- An F8 build that left the fix memory read-only crashed (0xC0000005) on
  the first probe; the fix memory now stays writable.
- Left-stick steering and L2 braking seen in a race with the virtual pad;
  R2 and right-stick throttle not yet confirmed (the scripted navigation
  into races is unreliable; a real-pad test is next).
- Real pad, user test with F7-F9: settings already on JOYSTICK 1 / ANALOGUE
  (all three analogue). A full stage driven normally with analog steering,
  triggers and right stick; menus follow the pad; no visual glitches.
  Clicking RUN JOYSTICK PANEL by accident still hangs the game (C6).


## Game updates run on a timer thread (2026-10-29)

In a race the game's update handlers (state machine column A, via
`0x443db0`) run on a multimedia-timer thread (`timeSetEvent` in `0x4a7790`,
callback `0x4a7920`) when the profile field `[0xd3ca80]+0x14` is 0, as on
the test setup; rendering stays on the main thread. Layout code such as the
results pages' `0x42e320` runs on that thread, so anything that changes what
the game sees (F13's screen size) has to hold for both threads. Read from
the running game with the harness scripts `.work/rt/peek.py`/`peekptr.py`
(`/proc/<pid>/mem`); `.work/rt/poke.py 0x514468 1` forces the stage end for
tests.

## Camera hiccups from a lock (2026-10-30)

The owner saw short car/camera hiccups in races and, twice, the camera
jumping to the car's roof and animating back. The lock F13 used to keep the
update thread away from 3D drawing made about 40% of updates wait 2-9 ms
(update gaps up to 49 ms instead of 40, measured with `CMRHOOKS_LOG`): the
game draws in-between frames by time, so a late update made the car and
camera run ahead and snap back. F13 now has no lock: the display state holds
640x480 for the whole race and the 3D code's eight screen-size reads (plus
F11's) call a stub that, for the main thread inside 3D work, returns a copy
of the display state with the real size (`F13_VIEW`, `cmr_3d_begin`). Update
gaps went back to 41 ms. `CMRHOOKS_LOG=<windows path>` writes the update and
frame timing every 5 s.

## Focus loss and the game's windowed mode (2026-10-30, E6)

Another window taking focus (Alt-Tab) is safe under Wine at 1024x768:
mid-race the game pauses itself and returns at its pause menu with the
picture intact; in the menus it carries on. On focus return the window
procedure (`WM_ACTIVATEAPP`) calls `0x498380`, which restores every lost
surface and reloads the textures.

Setting the fullscreen flag `0x5022f4` to 0 at start (the game's dormant
windowed mode) gives a blank screen: that path skips the mode switch but
still draws 16-bit pixels into a desktop-format back buffer, and its present
only waits for vertical blank without copying anything to the window.

A crash during a run is now reported in `launcher.log` ("the game crashed:
code, module+offset, registers"), from `crash.log` written by the hook
library's crash handler. A forced crash (display pointer `0x502518` zeroed
mid-race) logged `0xc0000005 at Rally.exe+0x9e40a`.
