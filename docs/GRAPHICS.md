# Graphics: resolution, widescreen, draw distance, frame rate

How the official EU v1.5 `Rally.exe` sizes, projects, culls and paces its 3D,
and what our in-memory fixes change. Static analysis of the executable
(objdump, Ghidra) unless a line says it was seen at run time. [O] observed in
code or data, [I] inferred. Status and the fix list are in
[THIN_APP_PLAN.md](THIN_APP_PLAN.md) (E1-E4, F10+).

## Resolution (E1)

- The front end always initialises DirectDraw at 640x480 (`0x4c2dc5`,
  8-bit table branch, slot 2) [O].
- Each race re-creates DirectDraw from scratch (`FUN_00421da0`, call
  `0x421ea9` -> `FUN_00497be0(0, cfg[0xb], 0, flags 2, 1)`): mode switch,
  primary/back surfaces, D3D device, Z-buffer `0x4a6e30(W,H)`, viewport
  `0x4a6ed0(0,0,W,H)`; torn down on race exit (`0x4992e0`) [O].
- `cfg[0xb]` is the RESOLUTION option (save byte `0x13`): 0..3 =
  320x240, 512x384, 640x480, 800x600. The slots come from EnumDisplayModes
  filtered to 320..800 x 240..600 (`0x4a66b0`) and matched exactly
  (`0x4a6548..0x4a65d1`) [O].
- **F10** returns a configured W x H for slot 2 at the race's slot load
  (`0x497d43`). Seen at run time (Wine, gamescope 1440x1080): the race
  renders at 1440x1080; the frame lines, timer, rev bar and digits follow,
  but the speedometer bitmap stays at its 640x480 pixel position.
- HUD sizing [O]: absolute rectangles chosen by exact width at `0x45d0fd`
  (320, 512, 800, anything else = the 640 table); fonts and HUD bitmaps by
  width (`fonts\{320x240,512x384,640x480}\…`, `images\osdnew\revs\classic\<WxH>…`);
  race fonts by the resolution byte (`0x4f0e1c`); some pieces scale 640x480
  rectangles by W/640, H/480 (`0x461a60`) or use Q16 fractions of W/H.
- **F13** (in-race 2D): one general rule instead of per-screen patches.
  In a race state (4 loading .. 0xb replay), each game tick (`0x443e90`)
  sees a virtual 640x480 screen, the size all of the game's 2D was laid out
  for, so the HUD, every message, the pause menu, results, replay overlay
  and loading screen resolve in one space with no per-screen work. The 2D
  primitives map every rectangle onto a 4:3 area centred on the real
  screen: the sprite and text queues `0x49b420`/`0x49b1d0` (destination and
  clip), the direct blit `0x49e3e0` and the panel `0x49e200`. A rectangle
  covering the whole virtual width or height (backdrop, fade, dimming
  panel, letterbox) spans the real screen. Only code that needs the real
  screen runs in "real" sections, with the real size and scaling paused:
  display set-up `0x497be0`, 3D device and viewport set-up `0x4a6950`, the
  clears `0x49ffc0`/`0x49ff80`, the projection `0x435d10` and the scene
  draw `0x49fe30` (which projects some effects to screen coordinates
  itself). A display change inside a real section updates the real size (a
  return to the front end's 640x480 ends scaling). The race HUD's layout
  and draw calls are marked so its side pieces anchor to the screen edges.
  The state logic is `cmr-hooks::sections` (host tests); the front end is
  never touched (it has F16). Seen at run time (3840x2160): menus, service
  screen, loading screen, race HUD in the corners, pause, quit back to the
  enlarged menu. Still to see: STAGE COMPLETE, results and replay overlay
  (need a finished stage). Not covered: split-screen views, the rotated
  needle dial's pivot.
- Forcing the whole game (front end too) above 640x480 breaks the front
  end (menu drawn in the corner, then a hang), so the front end stays at
  640x480.
- The game ignores a failed `SetDisplayMode` (`0x497ddf`): the display must
  offer the mode. Proton's fullscreen scaling and gamescope do.

## Aspect ratio (E2)

- Projection `FUN_00435d10` -> `0x4a71b0`: `xs = fov·2.46·(W/H)·1.14`
  (`0x435d44 fild [W]; fidiv [H]; fmul 65536.0`), `ys = fov·2.46·2.73`;
  the viewport clip is `dvClipY = W/H`, `dvClipHeight = 2W/H`
  (`0x4a6f03..0x4a6f71`; per view `0x49fe7d..0x49fee8`) [O].
- Correct only for W/H ≈ 4:3 [I]; at 16:9 the picture would be stretched.
- The D3D viewport clip values do not change the picture: the game
  transforms in software, to pixels `x·xs·W/2` and `y·ys·H/(2A)` (seen at
  run time: changing only the clip values had no effect).
- **F11** (Hor+): the `xs` aspect term becomes (4/3)²/A (`fild [H]; fidiv
  [W]; fmul 65536·16/9` at `0x435d44`) and `ys` is multiplied by A/(4/3)
  (`0x435d87`). Seen at run time: at 1920x1080 the centre of the picture is
  identical to the 1440x1080 one and the sides show more. The game's terrain cull uses a 90° wedge
  (below), which may need widening at the screen edges.

## Draw distance (E4)

Three distances, all from stage data at run time [O]:

- **Far plane** `0x4f683c`, from the stage's `.ars` file (`FUN_00480960`:
  object wedge `0x4f6838`, far plane, LOD wedge `0x4f6840`); usually
  150/150/100 m, some stages 200 m. Section triggers (`.dat` types 7..0xb,
  `FUN_00403e70`) set it to `table 0x4ea38c` = {50, 75, 100, 150, 250} m.
- **Per frame** (`FUN_00435d10`): object wedge × factor(`cfg[0xf]`, "object
  distance") and far plane × factor(`cfg[0xe]`, "track distance"), factor
  `0.5 + 0.25·k`; the menu offers k = 0..3, the code handles 0..6 (×2.0).
  Country 1 caps both at 200 m. Then the projection `zf`.
- **Terrain cull** `FUN_00490040`: cells within `zf / cell size` in a 90°
  wedge plus the 3x3 around the camera; objects are bucketed in visible
  cells (`FUN_004a07d0`), so nothing draws past `zf`. Two object LODs,
  switched at the LOD wedge (not menu-scaled). Billboards (trees) have no
  distance LOD.
- **Fog** is weather-only software vertex fog (10..80/140 m, never fully
  opaque); clear weather has none, so a longer distance shows no fog wall.
- Stages are loaded whole (one `.c3d`, no streaming): geometry beyond the
  limit exists.
- Plan: scale the raw sources by s (the 5 table dwords, the three `.ars`
  values via a detour at `0x4809a7`, the country-1 cap at
  `0x435e91/0x435e98/0x435f6a/0x435f71`); leave the front end. Risks: 16-bit
  Z precision (≈0.34 m at 150 m, ≈1.4 m at 300 m with zn = 1), cost grows
  with s², fixed C3D batch/vertex arenas (`FUN_004b2340`) unchecked.

## Frame rate and vsync (E3)

- Physics: fixed 25 Hz tick (50 Hz with a hidden config bit, `DAT_00551af0`,
  which changes physics results) [O].
- Mode A (config `+0x14 == 0`, believed default [I]): a WinMM timer thread
  runs the ticks (`0x4a7790` -> `0x443db0`); the main loop renders as fast
  as Flip allows and **interpolates** between ticks: alpha `0x4a7c90` (QPC
  since the last tick / last interval, Q16), poses `0x439ee0(alpha)` (car
  positions and orientations, sub-objects, object lists), camera
  `0x434d40(p, alpha)` [O].
- Mode B (`+0x14 != 0`): lock-step 25 fps, no interpolation [O].
- Present: `Flip(NULL, DDFLIP_WAIT)` (`0x49870f`), one back buffer: vsync'd
  double buffering [O]. Under Wine the expected result is refresh-rate
  rendering with interpolation [I]; to measure (ticks/s, flips/s, mode).
- If it judders: replace the alpha function with one based on the nominal
  period, or move the ticks to a fixed-step accumulator on the main thread
  (tick count and order unchanged, physics identical) [I].

### Measured (Wine 11.20, headless gamescope, llvmpipe renderer)

- Race at 1920x1080: about 190 presented frames per second (no vsync in
  headless gamescope), with physics at 25 Hz: rendering is decoupled from
  the tick and interpolated, as the code says (mode A).
- Race at 3840x2160 (F10 + F11 + F13): about 60 fps on the software
  renderer; HUD placed correctly.
- So no frame-rate patch is needed: on a real display the game presents at
  the refresh rate (Flip waits for vsync; gamescope caps to its output).
  If judder shows, the planned remedy is a steadier interpolation factor
  (replace `0x4a7c90` with one based on the nominal 40 ms period).

## Display paths for the 640x480 front end with a higher race mode

The front end must stay 640x480, so the display has to scale a 640x480
mode to the screen while also offering the race mode.
- gamescope (nested or headless) with Wine's X11 driver: no. The 640x480
  picture sits in the corner of a larger gamescope screen.
- Proton (GE-Proton 11): the front end shows as a clean 640x480 picture
  (its fullscreen scaling); a full check on a real screen is pending.
- Wine's Wayland driver on Hyprland: the 640x480 mode is scaled to a centred
  4:3 area of the monitor, and it does not trip over a portrait monitor
  (R15), but the picture stayed black in the first try (to investigate).
  It does not run inside gamescope (no `wl_subcompositor`).
- Tried and ruled out for the front end under gamescope: Wine's own mode
  emulation (`UseXRandR=N`, `UseXVidMode=N`: still drawn at 640x480 in the
  corner) and a Wine virtual desktop at 640x480 (same).
- Plan (own fix, independent of the display): in `cmrhooks.dll`, while the
  front end runs, keep the game's 640x480 back buffer as an off-screen
  surface and present it with a stretching `Blt` to a primary at the race
  (screen) resolution, centred 4:3, instead of the original `Flip`
  (`0x49870f`). The game keeps drawing exactly as before.

