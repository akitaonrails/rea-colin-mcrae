# Changelog

What changed for players, release by release. Releases up to 0.7.0 are
described on the GitHub releases page.

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
