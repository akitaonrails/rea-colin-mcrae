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
scaler (how gamescope enlarges the picture): Sharp (`nearest`, crisp pixels,
closest to the original artwork), Even pixels (`pixel`), FSR (`fsr`, smooth
then sharpened) and Smooth (`linear`). Everything is saved in
`launcher.cfg` inside the Wine prefix.

It needs `wine` and `gamescope`. Optional environment variables:

| Variable | Meaning | Default |
| --- | --- | --- |
| `CMR_PREFIX` | the Wine prefix | `~/.local/share/cmr-launcher/prefix` |
| `CMR_LAUNCHER` | path to `CMRLauncher.exe` | next to `cmr-play`, or one folder up |
| `CMR_RUN` | a wrapper that runs wine and gamescope, e.g. `distrobox enter gaming --` | none |

`cmr-play` refuses to start a second game while one is running or still
loading. Inside a gamescope session (a Steam Deck in Game Mode), or when
gamescope isn't installed, it runs the game directly.

## AppImage

`packaging/appimage/build.sh <unpacked zip folder> <output folder>` builds
`CMRLauncher-x86_64.AppImage`: the launcher, `cmr-play` and a portable Wine
(Kron4ek's 11.20 WoW64 build, checked by SHA-256), so players need no Wine.
Its `AppRun` puts that Wine first on the `PATH` and runs `cmr-play`, which
waits for the game (`CMR_WAIT`) because the AppImage is unmounted when it
returns. CI builds it with every package.
