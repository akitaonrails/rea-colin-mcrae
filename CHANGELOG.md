# Changelog

What changed for players, release by release. Releases up to 0.7.0 are
described on the GitHub releases page.

## [Unreleased]

### Added

- `--look original|enhanced|steam_deck` (and `look =` in `launcher.cfg`):
  every picture setting of that look at once; picture flags given with it
  win.
- The game's language has a control in the settings window (Play tab).
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
