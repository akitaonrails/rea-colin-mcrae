# Changelog

What changed for players, release by release. Releases up to 0.7.0 are
described on the GitHub releases page.

## [Unreleased]

## [0.10.3] - 2026-11-07

### Changed

- Updated rea-launcher-sdk to v0.4.4: the AppImage's portable Wine is now
  11.21, which fixes the startup crash on machines that cannot reserve
  ~64 GiB of address space (16 GB handhelds).

## [0.10.2] - 2026-11-07

### Fixed

- An unknown `Rally.exe` now shows its SHA-256 under the message on the
  Play tab, with a **Copy SHA-256** button, like a known edition does, so it
  can be pasted into the report form instead of retyped (issue #4). The
  window also says the game was found but the version is unknown, instead of
  asking for the game folder again.
- The README explains the keyboard: the game plays with it, how to pick it
  in the game's Options, and that the launcher's pad fixes only raise the
  throttle or the brake while a pad's trigger is pressed (issue #3).

## [0.10.1] - 2026-11-06

### Fixed

- Native Windows 10 and 11 now work out of the box without installing
  anything by hand:
  - **dgVoodoo2 2.87.6 (`DDraw.dll` and `D3DImm.dll`) is now bundled** in
    `dgvoodoo/` and installed into the runtime folder (`%LOCALAPPDATA%\CMRLauncher\Rally`)
    with a generated `dgVoodoo.conf` whenever the launcher runs on Windows.
    Previously, Windows 10 and 11's built-in `ddraw.dll` failed DirectDraw /
    `IDirect3D2` initialization or high-resolution mode switches and the game
    exited immediately after pressing Play unless the player had placed
    dgVoodoo2 into the runtime folder by hand.
  - **Legacy Windows DirectPlay is no longer required to start the game.**
    `Rally.exe` statically imported `DPLAYX.dll`, which is absent on Windows
    10 and 11 unless the legacy `DirectPlay` Windows Feature is enabled. The
    runtime copy now detaches the static `DPLAYX.dll` import and loads
    `<SystemDir>\dplayx.dll` dynamically only when it is installed on the
    system, falling back cleanly for single-player when it is not.
  - **Crash and fatal Windows exit codes are now reported in a dialog**
    pointing to `%LOCALAPPDATA%\CMRLauncher\launcher.log`, and the crash
    handler is installed as soon as `cmrhooks.dll` attaches (before `WinMain`
    starts) instead of waiting for the first race update.

## [0.10.0] - 2026-11-05

### Added

- Driving feel on the pad, with rumble on: the triggers shake when the
  wheels spin (right) and when braking hard, pulsing when the wheels lock
  (left); the pad knocks at each gear change, on landings (small jumps
  too) and on bumps, and buzzes faintly on rough ground and at the rev
  limiter. Trigger motors are reached through Windows.Gaming.Input; without
  it (Wine) the two main motors still work through XInput.
- `trigger_rumble` and `road_rumble` (Controls tab, `--trigger-rumble`,
  `--road-rumble`): switch the trigger feel or the road buzz off. Both on
  unless set off.

### Fixed

- The exact `Rally.exe` SHA-256 is written to `launcher.log` as soon as the
  settings window opens, and the Play tab shows it with a Copy button for an
  issue report, whether or not Play was pressed (issue #2).
- Native Windows with dgVoodoo2:
  - Any race resolution crashed in the menus: the front end locked
    images the game had already released. Their pixels are now copied
    while the game draws from them.
  - HD art crashed after the developer splash: the game's BMP reader steps
    24-bit rows by the width rounded to 4 pixels, so HD images of other
    widths were read sheared and past their end. They are laid out its way.
  - Entering a race with HD art crashed: on a device with palettized
    textures the texture loader copies an unset palette size for 24-bit
    images. Such HD textures (skies, panoramas) are now made 8-bit with
    their own palette; they had drawn black.
  - HD textures over 4096 pixels (the x4 sky) use the x2 picture.
  - With smooth textures, scenery went missing and parts of the car
    transparent: the mipmap levels of palettized textures were never
    filled. They are now.
- The HD menus ran near 20 fps at 4K: composing them is spread over helper
  threads (the game's own thread is pinned to one core, often an
  efficiency core on hybrid CPUs). About 80 fps now.

## [0.9.2] - 2026-11-05

A player's antivirus flagged the launcher on Windows. These changes make it
look like what it is, so antivirus programs are less likely to flag it.

### Changed

- The launcher no longer writes into the running game. It used to start
  the game paused and change it in memory, the way some malware does. Now
  each start writes the fixes into the launcher's own copy of the game
  (your game folder stays untouched), and Windows loads `cmrhooks.dll`
  with the game. Everything plays as before: checked in the real game
  with a race at 4K, the HD art, Alt+F4 and closing the window.
- `CMRLauncher.exe` and `cmrhooks.dll` now say what they are: name,
  version, publisher and the launcher's icon, as shown in Windows'
  Properties and Task Manager.
- If you go back to an older version later, delete `Rally.exe` from the
  launcher's copy of the game first (`%LOCALAPPDATA%\CMRLauncher\Rally`), or
  the older version refuses to start the game.
- Built on rea-launcher-sdk v0.4.3.

## [0.9.1] - 2026-11-03

From a test of the launcher on native Windows 10
([rea-launcher-sdk#1](https://github.com/akitaonrails/rea-launcher-sdk/issues/1)).

### Changed

- Updated rea-launcher-sdk to v0.4.2.
- `--check` while the settings window is open says to use the window and
  exits with 5, like every other option; before, it ran the check next to
  the window. While the game runs it says the game is already running,
  also with 5.

### Fixed

- A mistyped flag or a bad value (`--bogus-flag`, `--look invalid`) from
  a terminal is printed with the options and exits with 2 at once. Before,
  a message box taller than a 768-pixel screen opened too, and a script
  waited until someone pressed OK. Started without a terminal, it shows a
  short message that points to `--help`.
- The README says how to get past SmartScreen's "Windows protected your
  PC" for the unsigned launcher, and lists every registry entry the
  launcher writes, with how to remove them.

## [0.9.0] - 2026-11-03

### Changed

- Updated rea-launcher-sdk to v0.4.1.
- A second start that can't bring the open window forward says the game
  is already running and exits with 5 (it exited with 0). A start with
  options while the settings window is open asks you to use the window,
  also with 5. `cmr-play` reports both as "already running".
- Under `cmr-play`, an error in the settings-window run no longer shows
  a dialog of its own; the window or `cmr-play` reports it.

### Fixed

- Play is refused while the game from the launcher's copy still runs,
  even if the launcher that started it was killed; before, a second
  game could start over the same files.
- On a fresh Wine prefix the settings window opens at the first start
  (the launcher starts itself once more with OpenGL through GLX).

## [0.8.0] - 2026-11-02

### Added

- `--look original|enhanced|steam_deck` (and `look =` in `launcher.cfg`):
  every picture setting of that look at once; picture flags given with it
  win.
- `--scaler` sets the Linux scaler for one run, like every other option,
  and `cmr-play --play --scaler fsr --resolution 1280x720` uses it for
  gamescope without changing `launcher.cfg`.
- One launcher at a time: a second start brings the open window forward,
  or says the game is already running.
- An unknown `Rally.exe` is refused with its SHA-256 and a link to report
  it.

### Changed

- Built on [rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk)
  v0.3.3: the runtime copy, the saves mirror, `launcher.cfg` editing, the
  settings window's widgets, the Linux helper, packaging and CI are the
  shared code of the rea launchers.
- Saves: a save changed on one side since the last run wins. A session
  that ended before its saves were copied back (a crash, a killed game) no
  longer has those saves overwritten at the next start.
- Errors that stop the launcher show a dialog under `cmr-play`, the
  AppImage and Proton too; `--help` and `--check` print to the terminal.
- `--check` exits 1 for a `Rally.exe` it doesn't know.
- The settings window: every option has a one-line description; control
  borders and texts meet WCAG AA contrast; screen readers get every
  control's name, state and description; Play says why it is disabled.
- `launcher.log` is capped at 1 MiB.
- The first-run `launcher.cfg` lists every option commented out, with its
  values; files written by 0.7 read unchanged.
- The AppImage is named `CMRLauncher-<version>-x86_64.AppImage`.
- The looks carry the pad pedals: Original 1998 drives with the left
  stick only, as the game did; Enhanced and Steam Deck use the triggers
  and right stick. `gamepad_axes` and `close_quits` lose their window
  controls and stay as advanced `launcher.cfg` keys and flags; closing the
  window quits in every look. The pad's rumble and button layout share
  the Controls tab (the Buttons tab is gone).
- `language` has no window control any more: in the real game, rewriting
  the registry's `Language` changed nothing (the game keeps its own). The
  key stays, seeded into the registry when missing (advanced).

### Fixed

- Windows builds link with LLD (GNU ld 2.47 could leave imports
  unresolved).
- The runtime/game folder overlap check works on Windows paths under
  Wine (case, `\\?\` prefixes).
- The settings window opened a `launcher.cfg` with `look = enhanced` or
  `steam_deck` on the Original look, and Play then saved Original: it
  now opens on the file's look and saves that look's values.
- `cmr-play`: a look (`look =` in `launcher.cfg` or `--look`) with no
  resolution next to it left gamescope at 640×480 while the race was
  drawn at the look's size (Steam Deck 1280×800 in a 640×480 screen).
  gamescope now gets the look's size: Steam Deck 1280×800, Enhanced the
  screen, Original 640×480; `--resolution` still wins.
- `cmr-play`: a second start refused because another launcher holds the
  prefix no longer waits for that launcher's game to end before it
  returns.
- The settings window over a `launcher.cfg` that exists but can't be read
  (permissions, a lock) treated it as a first run, and Play wrote the
  template over it. The file is now left as it is, the window says so,
  and the run plays with the defaults, as without the window.
