# Research: Colin McRae Rally (1998)

What the community already solved for the PC game, and what this launcher
does with it (rea-launcher-sdk `docs/STANDARDS.md`, "Research before
building"). Written up after the launcher was built (the in-memory fixes
predate the SDK's standards); the decisions below were checked against
what the launcher does today. Facts from community pages are marked as
such and were not all re-verified.

## Where we looked

- [x] PCGamingWiki: the game's page and the community file area
      (Patrxgt's unofficial modern patch,
      https://community.pcgamingwiki.com/files/file/2416-patrxgts-unofficial-modern-patch-for-colin-mcrae-rally-1998)
- [x] Steam community discussions (store app 287340 threads on running
      the 1998 game today, widescreen exe files)
- [x] The community "fan pack" EU v1.5 executable (its `Rally.exe` is a
      supported edition, below)
- [ ] ProtonDB, WineHQ AppDB (not checked yet)
- [ ] GitHub searches for "Colin McRae Rally" + fix / widescreen (not
      checked yet)

## Community projects

| Project | What it fixes (as described by the community) | Licence | Maintained | Decision |
|---|---|---|---|---|
| Official Codemasters v1.5 update | The last official patch | Codemasters' | no (1998/99) | Required: the launcher supports the EU v1.5 `Rally.exe` |
| kFYatek's Windows 7 fix | Patches the installed files for Windows 7 and newer and removes the CD check | none found | no | Not used: it patches the game folder on disk; the launcher's fixes F1–F5 do the same in memory, and the game folder stays read-only. The community pack's exe below already carries an equivalent |
| Patrxgt's unofficial modern patch (PCGamingWiki files) | Alternative when kFYatek's fix fails on Windows 8+ | none found | unclear | Not used, same reason |
| 16:9 widescreen exe files (community) | Pre-edited executables for 16:9 | none found | no | Not used: replacing the exe means an unknown build; the launcher's F11 widescreen works on the identified builds |
| dgVoodoo2 (often recommended with the fixes) | DirectDraw/D3D wrapper | freeware, no redistribution licence | yes | **Used by hand on native Windows** (2.87.6): Windows' DirectDraw has no Ramp device and the game doesn't start without a wrapper. The launcher doesn't bundle it (no redistribution licence), so native Windows keeps one manual step until the owner decides on a bundled or downloadable wrapper (CMR4's launcher bundles 2.80 by an owner decision). Wine and Proton need nothing |
| The community "fan pack" EU v1.5 `Rally.exe` (ChemicalFlood) | The EU v1.5 exe already patched on disk with the CD/OS checks removed | n/a (the player's own file) | n/a | Supported edition (`46500c8e…`): the launcher skips F1–F5 for it |

## Our own fixes

Fixes nobody had, found by our reverse engineering (`docs/THIN_APP_PLAN.md`)
and applied in memory, the hook library's included; each has a draft in
`docs/CONTRIBUTIONS.md`. F1–F5 are the community's (above).

| Fix | What | Decision |
|---|---|---|
| F6 | Closing the window quits the game | **Our own** (draft C1) |
| F7 | Triggers and right stick as throttle and brake | **Our own** (draft C2) |
| F8 | Pads no longer drop to the keyboard | **Our own** (draft C3) |
| F9 | Car circling in reverse after a joystick save | **Our own** (draft C4) |
| F10 | Races at any resolution | **Our own** (draft C5) |
| F11 | Hor+ widescreen | **Our own** (draft C6) |
| F12 | Longer draw distance | **Our own** (draft C7) |
| F13 | HUD, messages and results scaled to the race resolution | **Our own** (draft C8) |
| F14 | Start pauses the race on a pad | **Our own** (draft C9) |
| F15 | No multi-second wait at start and exit | **Our own** (draft C10) |
| F16 | Intro and menus enlarged to the screen | **Our own** (draft C11) |
| F17 | Frame-rate cap and vsync choice | **Our own** (draft C12) |
| F18 | Mipmaps and trilinear filtering (including the palettized levels of native-Windows devices) | **Our own** (draft C13) |
| F19 | Sharp (unfiltered) textures option | **Our own** (draft C14) |
| F20 | No pop-in when the view distance changes | **Our own** (draft C15) |
| F21 | Triggers as separate pedals (XInput) | **Our own** (draft C16) |
| F22 | Rumble on impacts, plus the driving feel on the trigger motors (wheelspin, locking, gear changes, landings, bumps, rough ground, rev limiter) | **Our own** (draft C17) |
| F23 | Button layout and d-pad | **Our own** (draft C18) |
| F24 | RUN JOYSTICK PANEL no longer freezes under Wine | **Our own** (draft C19) |
| F25 | 24-bit depth buffer | **Our own** (draft C20) |
| F26 | Fast depth test (24 → 60 fps at 4K under Wine) | **Our own** (draft C21) |
| F27 | HD 2D art, including the native-Windows dgVoodoo2 fixes (the game's BMP row stride, the palette count of RGB textures, 8-bit twins, the 4096 texture limit, front-end surface copies, parallel composition) | **Our own** (draft C22) |

## Editions

| Build | Source | `Rally.exe` SHA-256 | Notes |
|---|---|---|---|
| Official EU v1.5 | retail CD + official update | `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a` | F1–F27 |
| EU v1.5, community-patched | fan pack | `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb` | F6–F27 (F1–F5 already on disk) |

Any other build is refused with its SHA-256 and the unknown-edition form.
US, v1.0 and other regional builds are not known yet.

## The game's files (static analysis)

The reverse engineering lives in `docs/THIN_APP_PLAN.md` (the fix list),
`docs/GRAPHICS.md` and `docs/UPSCALING.md`; observations from the running
game in `docs/RUNTIME_NOTES.md`.

- **Renderer:** DirectDraw / Direct3D (immediate mode), 640×480 menus.
- **Input:** WinMM joysticks (`joyGetPosEx`), keyboard.
- **Audio:** `qmixer.dll` from the game folder.
- **DRM, disc checks:** a CD-ROM check (F1) and an operating-system check
  (F4), with F2, F3 and F5 for its display and palette set-up on NT, all
  in memory on the official build.
- **Files and registry keys it writes:** `save\cmRally.cfg`,
  `SAVE\SAVESLOT*.cms`; `HKLM\SOFTWARE\Codemasters\Rally\1.00`.

## Old-game trouble spots

| Spot | Applies? | Finding | Fixed by |
|---|---|---|---|
| Timers and frame pacing on fast CPUs | yes | updates run on a timer thread (RUNTIME_NOTES 2026-10-29) | F17 frame-rate cap and vsync; the driving keeps its fixed rate |
| Multi-core affinity | not seen | — | — |
| DirectSound3D / EAX audio | no | qmixer | — |
| CD audio | no | — | — |
| Intro-video codecs | not looked at in this write-up | — | — |
| Resolution and aspect tables | yes | 640×480 menus, up to 800×600 races | F10/F11/F13 race resolution, widescreen, 2D remap |
| Focus loss | yes | windowed mode and focus (RUNTIME_NOTES 2026-10-30, E6) | see RUNTIME_NOTES; F6 makes closing the window quit |
