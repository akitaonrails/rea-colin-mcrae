# Linux helpers

## `cmr-play`

Starts the game through Wine inside [gamescope](https://github.com/ValveSoftware/gamescope),
which gives the game a landscape screen of the right size and scales it to
your monitor without changing its shape. It also avoids the crash some
setups get with a portrait (rotated) main monitor.

```sh
cmr-play                    # start with the saved settings
cmr-play play resolution=1920x1080 widescreen=true scaler=nearest
cmr-play play game="/home/you/Games/Colin McRae Rally"   # set the game folder
cmr-play check "/home/you/Games/Colin McRae Rally"       # is it supported?
cmr-play stop               # end the game
cmr-play get                # show the current settings
```

It needs `wine` and `gamescope`. Optional environment variables:

| Variable | Meaning | Default |
| --- | --- | --- |
| `CMR_PREFIX` | the Wine prefix | `~/.local/share/cmr-launcher/prefix` |
| `CMR_LAUNCHER` | path to `CMRLauncher.exe` | next to `cmr-play`, or one folder up |
| `CMR_RUN` | a wrapper that runs wine and gamescope, e.g. `distrobox enter gaming --` | none |

`cmr-play` refuses to start a second game while one is running or still
loading.

Scalers (how the picture is enlarged): `nearest` (sharp pixels, closest to
the original artwork), `pixel` (sharp, all pixels the same size), `fsr`
(smooth then sharpened), `linear` (smooth, blurry).

## Quickshell launcher

A small window with all the options, for desktops that have
[Quickshell](https://quickshell.org) (for example Hyprland setups):

```sh
qs -p linux/quickshell-launcher
```

Pick the game folder and press **Check** (it shows which version it found),
choose your options and press **Start**. Your choices are saved when you
press Start and shown again next time; the button stays locked while the
game loads, so a double click can't start it twice.
It uses `cmr-play`, so the same environment variables apply.

The photo of Colin McRae is by Carolyn Williams, CC BY 2.0
([source](https://commons.wikimedia.org/wiki/File:Colin_Mcrae_crop.jpg)).
