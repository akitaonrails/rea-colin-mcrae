# Linux helpers

## `cmr-play`

Starts the game through Wine inside [gamescope](https://github.com/ValveSoftware/gamescope),
which gives the game a landscape screen of the right size and scales it to
your monitor without changing its shape. It also avoids the crash some
setups get with a portrait (rotated) main monitor.

```sh
cmr-play          # the settings window, then the game
cmr-play --play   # the game with the saved settings, no window
cmr-play stop     # end the game
cmr-play status   # running or idle
```

The settings window is the launcher's own (the same as on Windows), shown
through Wine before gamescope starts. On Linux it has one more choice, the
scaler (how gamescope enlarges the picture): Smooth (`linear`, the
default, suits the HD art), Sharp (`nearest`, crisp pixels, closest to the
original artwork), Even pixels (`pixel`) and FSR (`fsr`, smooth then
sharpened). Everything is saved in `launcher.cfg` inside the Wine prefix
(`drive_c/users/<you>/AppData/Local/CMRLauncher/`), or next to
`CMRLauncher.exe` for a portable install.

Launcher flags may follow `--play` for one run, without changing
`launcher.cfg`; the scaler and the resolution reach gamescope too:

```sh
cmr-play --play --scaler nearest --resolution 1280x960
```

gamescope gets the race resolution (`-w`/`-h`; 640×480 when it is unset,
`original` or `off`) and fits it to the screen. Inside a gamescope session
(a Steam Deck in Game Mode), or when gamescope isn't installed, the game
runs directly.

It needs `wine` and, for the scaled screen, `gamescope`. Optional
environment variables:

| Variable | Meaning | Default |
| --- | --- | --- |
| `CMR_PREFIX` | the Wine prefix | `~/.local/share/cmr-launcher/prefix` |
| `CMR_LAUNCHER` | path to `CMRLauncher.exe` | next to `cmr-play`, or one folder up |
| `CMR_RUN` | a wrapper that runs wine and gamescope, e.g. `distrobox enter gaming --` | none |
| `CMR_WAIT` | set: wait for the game to end (the AppImage sets it) | return once the game is starting |

### The session

`cmr-play` runs the game in its own session and remembers it in
`~/.config/cmr-launcher/running.pid` (process id and start time). A second
`cmr-play` while that session runs, or is still loading, says `already
running` and starts nothing. `stop` (or Ctrl+C while it waits) ends that
session and the prefix's Wine processes (`wineserver -k`); it never touches
a `Rally.exe` started some other way. Its output is in
`~/.config/cmr-launcher/play.log`, the settings window's in `settings.log`.

`cmr-play` is built on the shared play library of
[rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk)
(`rea-play.sh`, beside it in a package, `sdk/rea-play.sh` in the
repository); this script only holds the game's settings.

Tests: `sh linux/tests/<name>.sh` (`gpu-env.sh`, `scaler.sh`,
`supervision.sh`, `prefix.sh`), with fake programs, never Wine or the game.

## AppImage

`packaging/package.sh dist/CMRLauncher-<version>` builds the zip folder,
the zip and `CMRLauncher-<version>-x86_64.AppImage` with the SDK's
`sdk/packaging/appimage-wine.sh`: the launcher, `cmrhooks.dll`, the HD art
pack, `cmr-play` with `rea-play.sh` and a portable Wine (Kron4ek's 11.20
WoW64 build, checked by SHA-256), so players need no Wine. Its `AppRun`
puts that Wine first on the `PATH` and runs `cmr-play`, which waits for the
game (`CMR_WAIT`) because the AppImage is unmounted when it returns. CI
builds both with every package.
