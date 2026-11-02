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
| dgVoodoo2 (often recommended with the fixes) | DirectDraw/D3D wrapper | freeware, no redistribution licence | yes | Not used: the game runs on Wine's and Windows' own DirectDraw with the launcher's fixes; nothing to wrap so far |
| The community "fan pack" EU v1.5 `Rally.exe` (ChemicalFlood) | The EU v1.5 exe already patched on disk with the CD/OS checks removed | n/a (the player's own file) | n/a | Supported edition (`46500c8e…`): the launcher skips F1–F5 for it |

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
