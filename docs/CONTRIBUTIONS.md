# Contributions back

Fixes found while building this launcher that the community lacks, drafted
for PCGamingWiki, upstream projects, ProtonDB or the WineHQ AppDB
(`sdk/docs/playbooks/contribute-back.md`). These are public posts under the
owner's name: agents only draft them here; the owner reviews and posts
(by id: "post C3").

**Every fix of our own gets a row here**: each `docs/RESEARCH.md` row
decided as **Our own** cites its draft id, and each byte patch and the hook
library (`cmrhooks.dll`) have one. A fix not verified in the real game yet
stays a draft with status "needs verification". F1–F5 are not ours: the
community's Windows fixes and its pack's `Rally.exe` already carry them
(`docs/RESEARCH.md`). `sdk/scripts/launcher-check` (rule `own-fix-draft`)
checks this.

Before posting any of them: check the game's PCGamingWiki page again for
the same fix (step 1 of the playbook; the page was last read before these
fixes existed), and describe the fixes as what the launcher does, never as
files to download from elsewhere. The addresses are for the EU v1.5
`Rally.exe` (official `0eb6ea96…`, and the community-patched
`46500c8e…`, which differs only where F1–F5 write).

| Id | Finding | Target (page or project) | Evidence | Status |
|---|---|---|---|---|
| C1 | F6: Closing the window quits the game | PCGamingWiki: Colin McRae Rally, Issues fixed | THIN_APP_PLAN F6; RUNTIME_NOTES 2026-11-02 (Alt+F4 and WM_CLOSE quit paths) | draft |
| C2 | F7: Triggers and right stick as throttle and brake | PCGamingWiki: Colin McRae Rally, Input settings | THIN_APP_PLAN F7 (a full stage driven on an 8BitDo Ultimate 2 under Wine) | draft |
| C3 | F8: Pads no longer drop to the keyboard | PCGamingWiki: Colin McRae Rally, Input settings | THIN_APP_PLAN F8 (JOYSTICK 1 kept; menus and race on the pad) | draft |
| C4 | F9: Car circling in reverse after a joystick save | PCGamingWiki: Colin McRae Rally, Issues fixed | THIN_APP_PLAN F9 (broken save: the car waits at the start instead of circling) | draft |
| C5 | F10: Races at any resolution | PCGamingWiki: Colin McRae Rally, Video settings: Widescreen resolution | THIN_APP_PLAN F10 (1440×1080 and 1920×1080 under Wine, gamescope) | draft |
| C6 | F11: Hor+ widescreen | PCGamingWiki: Colin McRae Rally, Video settings: Widescreen resolution | THIN_APP_PLAN F11 (1920×1080 centre matches the 1440×1080 picture) | draft |
| C7 | F12: Longer draw distance | PCGamingWiki: Colin McRae Rally, Video settings: Field of view / draw distance | THIN_APP_PLAN F12 (×2.0: far plane and object wedge 300 m in memory) | draft |
| C8 | F13: HUD, messages and results scaled to the race resolution | PCGamingWiki: Colin McRae Rally, Video settings: Widescreen resolution | THIN_APP_PLAN F13 (1920×1080 and 3840×2160: menus, loading, HUD, pause, results) | draft |
| C9 | F14: Start pauses the race on a pad | PCGamingWiki: Colin McRae Rally, Input settings | THIN_APP_PLAN F14 (virtual pad: Start opens GAME PAUSED) | draft |
| C10 | F15: No multi-second wait at start and exit | PCGamingWiki: Colin McRae Rally, Issues fixed | THIN_APP_PLAN F15 (main menu ~6 s after start) | draft |
| C11 | F16: Intro and menus enlarged to the screen | PCGamingWiki: Colin McRae Rally, Video settings: Widescreen resolution | THIN_APP_PLAN F16 (1920×1080 and 3840×2160 headless) | draft |
| C12 | F17: Frame-rate cap and vsync choice | PCGamingWiki: Colin McRae Rally, Video settings: High frame rate / Vertical sync | THIN_APP_PLAN F17 (caps 30/60 exact, logic speed unchanged) | draft |
| C13 | F18: Mipmaps and trilinear filtering | PCGamingWiki: Colin McRae Rally, Video settings: Anisotropic filtering | THIN_APP_PLAN F18 (headless); palettized mip levels filled so smooth textures stay complete on a device with palettized textures (native Windows 11 + dgVoodoo2 2.87.6, RUNTIME_NOTES 2026-11-03) | draft |
| C14 | F19: Sharp (unfiltered) textures option | PCGamingWiki: Colin McRae Rally, Video settings | THIN_APP_PLAN F19 (headless) | draft |
| C15 | F20: No pop-in when the view distance changes | PCGamingWiki: Colin McRae Rally, Video settings | THIN_APP_PLAN F20 (headless) | draft |
| C16 | F21: Triggers as separate pedals (XInput) | PCGamingWiki: Colin McRae Rally, Input settings: Controller support | THIN_APP_PLAN F21 (headless) | draft |
| C17 | F22: Rumble on impacts and the driving feel on the trigger motors | PCGamingWiki: Colin McRae Rally, Input settings: Controller support | THIN_APP_PLAN F22 (headless); the trigger/road feel through Windows.Gaming.Input was tuned with a player over three rounds on recorded stages (native Windows 11, Xbox Elite Series 2; RUNTIME_NOTES 2026-11-03) | draft |
| C18 | F23: Button layout and d-pad | PCGamingWiki: Colin McRae Rally, Input settings: Controller support | THIN_APP_PLAN F23 (headless) | draft |
| C19 | F24: RUN JOYSTICK PANEL no longer freezes under Wine | PCGamingWiki: Colin McRae Rally, Issues fixed (Linux) | THIN_APP_PLAN F24 (applied, not exercised in the real game) | needs verification |
| C20 | F25: 24-bit depth buffer | PCGamingWiki: Colin McRae Rally, Video settings | THIN_APP_PLAN F25 (race at 3× distance renders cleanly; the reported Greece mountains not yet confirmed) | needs verification |
| C21 | F26: Fast depth test (24 → 60 fps at 4K under Wine) | PCGamingWiki: Colin McRae Rally, Issues fixed (Linux) / WineHQ AppDB note | THIN_APP_PLAN F26, RESEARCH R19 (60 fps at 4K, answers checked mid-race) | draft |
| C22 | F27: HD 2D art | PCGamingWiki: Colin McRae Rally, Video settings | THIN_APP_PLAN F27 (1080p and 4K: HUD and menus from twins); native Windows 11 + dgVoodoo2 2.87.6: the game's 4-pixel BMP row stride, the unset palette count of RGB textures, 8-bit twins for palettized-texture devices, the 4096 texture limit and the front-end surface copies fixed (RUNTIME_NOTES 2026-11-03) | draft |

## Drafts

Each draft is the text for the page's fix box, ready to paste once the
owner approves it; the executables are named by SHA-256 and version, never
attached or linked.

### C1: Closing the window quits the game (F6)

- **Target:** PCGamingWiki: Colin McRae Rally, "Issues fixed".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F6; RUNTIME_NOTES 2026-11-02 (Alt+F4 and WM_CLOSE quit paths).

> **Closing the window quits the game.** The game's window procedure ignores `WM_CLOSE`: Alt+F4 or the window manager's close button leaves the game running full screen (under gamescope it looks frozen). [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): on `WM_CLOSE` the game's own quit flag is set, so it shuts down through its normal teardown.
> Technical note: WndProc jump-table entry for `WM_CLOSE` at `0x49de78`; quit flag `0x96efe8` (EU v1.5).

### C2: Triggers and right stick as throttle and brake (F7)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F7 (a full stage driven on an 8BitDo Ultimate 2 under Wine).

> **Triggers and right stick as throttle and brake.** The game reads only X/Y and buttons of a joystick, so on a modern pad only the left stick drives. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the Z (triggers) and R (right stick) axes are read too, and the throttle/brake axis follows whichever is furthest from centre (axes present and centred at start).
> Technical note: analog joystick read `0x49cdf0` (sites `0x49ce1b`, `0x49ce6f`) (EU v1.5).

### C3: Pads no longer drop to the keyboard (F8)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F8 (JOYSTICK 1 kept; menus and race on the pad).

> **Pads no longer drop to the keyboard.** Control Setup clamps CONTROL METHOD to the pads that answered in the current frame; one transient `JOYERR_UNPLUGGED` (common under Wine) switches the player to the keyboard. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): a pad that answered once stays present for the session.
> Technical note: joystick probe `0x49d5c0` (site `0x49d60e`) (EU v1.5).

### C4: Car circling in reverse after a joystick save (F9)

- **Target:** PCGamingWiki: Colin McRae Rally, "Issues fixed".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F9 (broken save: the car waits at the start instead of circling).

> **Car circling in reverse after a joystick save.** A joystick type written to keyboard record 0 (from the F8 case, or an old save) makes the game read key codes as joystick axes: the car circles in reverse at the start. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): keyboard records 0 and 1 always keep the keyboard type; such saves are repaired at start-up.
> Technical note: device-type setter `0x49d670` (site `0x49d67d`) (EU v1.5).

### C5: Races at any resolution (F10)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Widescreen resolution".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F10 (1440×1080 and 1920×1080 under Wine, gamescope).

> **Races at any resolution.** Races offer at most 800×600. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the race's 640×480 mode slot returns the chosen resolution; the front end is unchanged.
> Technical note: race mode-slot load `0x497d43` (EU v1.5).

### C6: Hor+ widescreen (F11)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Widescreen resolution".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F11 (1920×1080 centre matches the 1440×1080 picture).

> **Hor+ widescreen.** The software projection scales by the aspect ratio, so a 16:9 race is 1.78× wider and 0.75× shorter than 4:3 (stretched). [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): projection terms corrected so a widescreen race shows more at the sides (Hor+), identical at 4:3; the culling wedges are widened to match.
> Technical note: projection `0x435d44`, `0x435d87`; culling `0x4902c6`, `0x4902e5`, `0x4a097e`, `0x4a099f`, `0x4a0a21`, `0x4a0a32` (EU v1.5).

### C7: Longer draw distance (F12)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Field of view / draw distance".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F12 (×2.0: far plane and object wedge 300 m in memory).

> **Longer draw distance.** The view distance comes from stage data (usually 150 m) and is capped at 200 m. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): stage object/far/LOD values, section-trigger values and the cap are multiplied by a chosen factor.
> Technical note: `0x4809a7`, `0x403ea7`, caps `0x435e91/98`, `0x435f6a/71` (EU v1.5).

### C8: HUD, messages and results scaled to the race resolution (F13)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Widescreen resolution".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F13 (1920×1080 and 3840×2160: menus, loading, HUD, pause, results).

> **HUD, messages and results scaled to the race resolution.** At a resolution the game never had, its 2D (HUD, messages, results, loading screen) mixes 640×480 rectangles, screen fractions and fixed pixel tables: parts land off screen or tiny. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the race's 2D sees the 640×480 screen it was made for and every rectangle is mapped onto a centred 4:3 area (hook library `cmrhooks.dll`).
> Technical note: display set-up `0x497be0`, 3D set-up `0x4a6950`, projection `0x435d10`, scene draw `0x49fe30` and the 2D primitives (EU v1.5).

### C9: Start pauses the race on a pad (F14)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F14 (virtual pad: Start opens GAME PAUSED).

> **Start pauses the race on a pad.** The pause only reads Escape; a pad has no way to pause. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): escape also reads as held while the pad's Start (button 8) is held.
> Technical note: race pause check `0x449714` (EU v1.5).

### C10: No multi-second wait at start and exit (F15)

- **Target:** PCGamingWiki: Colin McRae Rally, "Issues fixed".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F15 (main menu ~6 s after start).

> **No multi-second wait at start and exit.** The CPU clock check busy-waits a second counting `rdtsc` in 32 bits and retries on wrap; above ~2 GHz it wraps, so start-up and exit hang for several seconds. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the check returns 1 GHz at once (only compared with 198 MHz for the default detail level).
> Technical note: CPU clock check `0x4ad240` (EU v1.5).

### C11: Intro and menus enlarged to the screen (F16)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Widescreen resolution".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F16 (1920×1080 and 3840×2160 headless).

> **Intro and menus enlarged to the screen.** The intro and menus are drawn at 640×480 and sit small in a corner at higher resolutions. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the front end runs in the race resolution's mode and each 640×480 frame is enlarged, centred 4:3 (hook library).
> Technical note: front-end mode set `0x497db9`, legal screen `0x4c1e5f`, the frame's `Flip` (EU v1.5).

### C12: Frame-rate cap and vsync choice (F17)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: High frame rate / Vertical sync".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F17 (caps 30/60 exact, logic speed unchanged).

> **Frame-rate cap and vsync choice.** The game draws as fast as it can, with no cap, and vsync can't be chosen. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): each frame's flip is paced to a chosen cap with the chosen vsync; game logic keeps its fixed rate (hook library).
> Technical note: frame `Flip` `0x498705` (EU v1.5).

### C13: Mipmaps and trilinear filtering (F18)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings: Anisotropic filtering".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F18 (headless).

> **Mipmaps and trilinear filtering.** Race textures have no mipmaps: distant road and scenery shimmer. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): mipmap chains are filled after each texture upload and minification is trilinear (hook library).
> Technical note: texture `CreateSurface` `0x49eb66`, `Load` calls `0x49a142`, `0x49a448`, `0x49acfa`, filter `0x4a243e` (EU v1.5).

### C14: Sharp (unfiltered) textures option (F19)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F19 (headless).

> **Sharp (unfiltered) textures option.** Some players prefer the unfiltered look of software rendering. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the device reports that it can't filter, so textures are drawn with nearest pixels.
> Technical note: device-can-filter `0x4a9fe0` (EU v1.5).

### C15: No pop-in when the view distance changes (F20)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F20 (headless).

> **No pop-in when the view distance changes.** Section changes switch the view distance at once: scenery pops in or out. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the far plane eases towards the new value (half the gap per 0.33 s) (hook library).
> Technical note: far-plane reads `0x435ffe`, `0x436018` (EU v1.5).

### C16: Triggers as separate pedals (XInput) (F21)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings: Controller support".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F21 (headless).

> **Triggers as separate pedals (XInput).** Through WinMM a pad's triggers are one combined axis: pressing both cancels out. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the XInput triggers are added as separate throttle and brake (hook library).
> Technical note: in-race control map `0x449580` (EU v1.5).

### C17: Rumble on impacts (F22)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings: Controller support".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F22 (headless).

> **Rumble on impacts.** The PC game has no force feedback for pads. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): impacts drive the pad's rumble by severity, with a felt minimum and a square-root curve (hook library, XInput).
> Technical note: impact sound `0x469e80` (EU v1.5).

### C18: Button layout and d-pad (F23)

- **Target:** PCGamingWiki: Colin McRae Rally, "Input settings: Controller support".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F23 (headless).

> **Button layout and d-pad.** Pad buttons are fixed to their 1998 order and the d-pad is ignored. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): a chosen button layout, and the d-pad as directions (hook library).
> Technical note: pad buttons in the joystick read `0x49cef6` (EU v1.5).

### C19: RUN JOYSTICK PANEL no longer freezes under Wine (F24)

- **Target:** PCGamingWiki: Colin McRae Rally, "Issues fixed (Linux)".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** needs verification. Evidence: THIN_APP_PLAN F24 (applied, not exercised in the real game).

> **RUN JOYSTICK PANEL no longer freezes under Wine.** Under Wine the controls screen's RUN JOYSTICK PANEL waits forever for a panel that never shows. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the item does nothing under Wine.
> Technical note: `0x4cdbc0` calling `0x49cb90` (EU v1.5).

### C20: 24-bit depth buffer (F25)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** needs verification. Evidence: THIN_APP_PLAN F25 (race at 3× distance renders cleanly; the reported Greece mountains not yet confirmed).

> **24-bit depth buffer.** With a longer view the 16-bit depth buffer makes distant hills flicker through each other. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): the game asks for a 24-bit depth buffer.
> Technical note: depth buffer request `0x4a6e9b` (EU v1.5).

### C21: Fast depth test (24 → 60 fps at 4K under Wine) (F26)

- **Target:** PCGamingWiki: Colin McRae Rally, "Issues fixed (Linux) / WineHQ AppDB note".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F26, RESEARCH R19 (60 fps at 4K, answers checked mid-race).

> **Fast depth test (24 → 60 fps at 4K under Wine).** The sun-glare visibility test locks the whole depth buffer to read one value; under Wine on a GPU every frame copies all of it back. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): only the one pixel is locked and read (hook library).
> Technical note: depth test `0x49fa70` (EU v1.5).

### C22: HD 2D art (F27)

- **Target:** PCGamingWiki: Colin McRae Rally, "Video settings".
- **Applies to:** `Rally.exe` EU v1.5 (SHA-256
  `0eb6ea9652219b9a35c0a6134bf0cdf316dce21215b724cdfc10ba85eb95982a`, and
  the community-patched
  `46500c8e22ce3e943b6855eca63ae9288261162347ca76fa36cad77cbb19c0bb`).
- **Status:** draft. Evidence: THIN_APP_PLAN F27 (1080p and 4K: HUD and menus from twins).

> **HD 2D art.** The 2D art made for 640×480 is enlarged with sharp pixels at high resolutions. [https://github.com/akitaonrails/rea-colin-mcrae CMRLauncher] fixes it in memory
> when it starts the game (the files on disk are not changed): redrawn/upscaled twins of the game's images load beside them and are sampled smoothly (hook library; the art pack is made by `cmr-hd` from the player's own files, `docs/UPSCALING.md`).
> Technical note: texture loader `0x499ec0`, 2D surface loader `0x499320`, blit hook `0x49e3e0` (EU v1.5).
