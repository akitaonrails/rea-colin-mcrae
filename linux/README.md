# Colin McRae Rally on Linux

The launcher is a Windows program; on Linux it runs through Wine or
Proton. There are three ways to play: the AppImage, which brings its own
Wine; Steam with Proton; or your own Wine with `cmr-play` from the zip.

## The AppImage

```sh
chmod +x CMRLauncher-*-x86_64.AppImage
./CMRLauncher-*-x86_64.AppImage
```

It runs `cmr-play` with the Wine inside it, so nothing else has to be
installed. Its prefix is `~/.local/share/cmr-launcher/prefix`. If
[gamescope](https://github.com/ValveSoftware/gamescope) is installed, the
game runs in it at the right size; otherwise in a plain window.

## Steam (Proton)

1. Unpack the zip, for example into `~/Games/cmr-launcher`.
2. In Steam: **Games → Add a Non-Steam Game to My Library… → Browse**, and
   pick `CMRLauncher.exe`.
3. Right-click it → **Properties → Compatibility** → tick **Force the use
   of a specific Steam Play compatibility tool** and choose **GE-Proton**
   (tested with GE-Proton 11).

Proton runs the launcher directly, without `cmr-play` or gamescope.

### Steam Deck

The same steps in Desktop Mode (GE-Proton comes from ProtonUp-Qt in
Discover), then play from Game Mode and pick the **Steam Deck** look. Not
tested on a Deck yet.

## Your own Wine

`cmr-play` starts the game through Wine inside gamescope, which gives the
game a landscape screen of the right size and scales it to your monitor
without changing its shape. That also avoids the crash some setups get
with a portrait (rotated) main monitor.

```sh
cmr-play          # the settings window, then the game
cmr-play --play   # the game with the saved settings, no window
cmr-play stop     # end the game
cmr-play status   # running or idle
```

The settings window is the launcher's own, the same as on Windows, shown
through Wine before gamescope starts. On Linux it has one more choice, the
scaler: how gamescope enlarges the picture. **Soft** (`linear`, the
default, suits the HD art), **Crisp pixels** (`nearest`, closest to the
original artwork), **Even pixels** (`pixel`) or **FSR** (`fsr`, smoothed,
then sharpened). Everything is saved in `launcher.cfg` inside the Wine
prefix (`drive_c/users/<you>/AppData/Local/CMRLauncher/`), or next to
`CMRLauncher.exe` for a portable install.

Launcher options may follow `--play` for one run, without changing
`launcher.cfg`; the scaler and the resolution reach gamescope too:

```sh
cmr-play --play --scaler nearest --resolution 1280x960
```

gamescope gets the race resolution (640×480 when it is unset, `original`
or `off`) and fits it to the screen. A `--look` given this way changes the
game's picture, but gamescope keeps the resolution from `launcher.cfg`;
give `--resolution` too. Inside a gamescope session (a Steam Deck in Game
Mode), or when gamescope isn't installed, the game runs directly.

`cmr-play` needs `wine` and, for the scaled screen, `gamescope`. If they
live in a container, set `CMR_RUN`, e.g. `CMR_RUN="distrobox enter gaming --"`.

## Environment

| Variable | Meaning | Default |
| --- | --- | --- |
| `CMR_PREFIX` | the Wine prefix | `~/.local/share/cmr-launcher/prefix` |
| `CMR_LAUNCHER` | path to `CMRLauncher.exe` | next to `cmr-play`, or one folder up |
| `CMR_RUN` | a wrapper that runs wine and gamescope, e.g. `distrobox enter gaming --` | none |
| `CMR_WAIT` | set: wait for the game to end (the AppImage sets it) | return once the game is starting |

## Where things are

| What | Where |
| --- | --- |
| Settings (`launcher.cfg`) and log (`launcher.log`) | in the prefix, `drive_c/users/<you>/AppData/Local/CMRLauncher/` |
| The launcher's copy of the game | in the prefix, `drive_c/users/<you>/AppData/Local/CMRLauncher/Rally` |
| Your saves | in the prefix, `drive_c/users/<you>/Saved Games/Colin McRae Rally` |
| `cmr-play`'s output, the settings window's output and the session lock | `~/.config/cmr-launcher/` (`play.log`, `settings.log`, `running.pid`) |

`cmr-play` runs the game in its own session and remembers it in
`running.pid`. A second `cmr-play` while that session runs, or is still
loading, says `already running` and starts nothing. `stop` (or Ctrl+C
while it waits) ends that session and the prefix's Wine processes
(`wineserver -k`); it never touches a `Rally.exe` started some other way.

## Troubleshooting

- **Steam/Proton: the game crashes right away with an NVIDIA card.** Add
  `PROTON_USE_WOW64=1` in front of `%command%` in the launch options.
- **Anything else:** look in `~/.config/cmr-launcher/play.log` and the
  launcher's `launcher.log` (above), and attach both to a
  [bug report](https://github.com/akitaonrails/rea-colin-mcrae/issues/new?template=bug-report.yml).

## For developers

`cmr-play` is built on the shared play library of
[rea-launcher-sdk](https://github.com/akitaonrails/rea-launcher-sdk)
(`rea-play.sh`, beside it in a package, `sdk/rea-play.sh` in the
repository); this script holds only the game's settings. Tests:
`sh linux/tests/<name>.sh` (`gpu-env.sh`, `scaler.sh`, `supervision.sh`,
`prefix.sh`), with fake programs, never Wine or the game.
`packaging/package.sh dist/CMRLauncher-<version>` builds the zip and
`CMRLauncher-<version>-x86_64.AppImage` (the SDK's
`sdk/packaging/appimage-wine.sh`, with Kron4ek's Wine 11.20 checked by
SHA-256).
