# Colin McRae Rally — play it today

> **Work in progress.** It plays well under Linux (Wine and Proton), but it
> still needs more testing and bug fixes, and it hasn't been tried on native
> Windows yet. Help is very welcome, especially from **Windows** players:
> try it and [report what happens](https://github.com/akitaonrails/rea-colin-mcrae/issues),
> or send a pull request.

Play your own copy of the 1998 PC game **Colin McRae Rally** on a modern
computer: Windows 10/11, or Linux through Steam (Proton) or Wine.

You point the launcher at your installed game and it does the rest. No game
patches, no compatibility settings, no registry edits. Your game folder is
never changed: the launcher works on its own private copy.

[![A full stage in New Zealand, played with the launcher: high resolution, 16:9 widescreen, HUD in the corners](https://new-uploads-akitaonrails.s3.us-east-2.amazonaws.com/2026/10/29/colin-mcrae-rally/showcase-poster.jpg)](https://new-uploads-akitaonrails.s3.us-east-2.amazonaws.com/2026/10/29/colin-mcrae-rally/showcase-h264.mp4)

*A full stage in New Zealand at high resolution and 16:9, with the HUD in
the corners. Click to watch (3½ minutes, with sound).*

![The settings window](docs/images/launcher.png)

## What it adds to the original game

**Runs on today's computers**

- Starts on modern Windows and on Linux; the 1998 game alone stops at its
  CD check, its Windows version check and its old graphics modes.
- No CD needed: the game runs from its installed files.
- Closing the window (or Alt+F4) quits the game; the original ignored it.

**Gamepads**

- Accelerate and brake with the triggers or the right stick, not only the
  left stick; with an Xbox-style pad both triggers work at once (left-foot
  braking).
- Optional rumble on impacts.
- Start pauses the race.
- Your controller choice no longer falls back to the keyboard by itself, and
  saves broken by that bug (car turning in circles) are repaired.

**Picture** (all optional)

- Races at higher resolutions, up to 4K (the original stops at 800×600).
- The intro and menus are enlarged to your screen with sharp pixels, and
  everything drawn during a race (HUD, messages, pause menu, results,
  loading screen) scales with the resolution; the HUD sits in the corners
  of wide screens.
- 16:9 widescreen that shows more to the sides instead of stretching.
- A longer view distance, up to 3× further down the stage, eased when the
  stage changes it so scenery doesn't pop in.
- Widescreen keeps the scenery at the edges of the wider view.
- A frame-rate cap and a vsync choice, for smooth, even frames.
- Smooth textures (mipmaps and trilinear filtering) that don't shimmer in
  the distance, or a sharp, unfiltered look.

**Comfort**

- Your saves live in a folder you choose, outside the game.
- Your game folder is never modified.
- A settings window, the same on Windows and Linux, and a commented settings file.

This project contains **no game files**. You need your own copy of the game.

## 1. The game you need

**Colin McRae Rally for Windows PC, the original 1998 CD-ROM release by
Codemasters, installed and updated to version 1.5.**

- Install it from your CD with the **full install** option (the launcher
  can't read data that was left on the CD).
- Apply the official Codemasters **v1.5 update** if your copy isn't updated
  yet. The installed folder contains `Rally.exe` next to the folders `Game`,
  `New_Fe` and `ini`.
- Already have an installed copy from an older PC, or a backup of the game
  folder? That works too: the launcher only needs that folder.

The launcher checks `Rally.exe` and tells you if it doesn't recognise your
version. Recognised today:

| `Rally.exe` | Version |
| --- | --- |
| SHA-256 `0eb6ea96…5982a` | Official European v1.5 |
| SHA-256 `46500c8e…c0bb` | European v1.5 from a community fan pack |

If yours isn't recognised, the launcher shows its SHA-256. Please
[open an issue](https://github.com/akitaonrails/rea-colin-mcrae/issues) with
that number and where your copy comes from (US release, v1.0, another
language…), and support can be added.

Not supported: the PlayStation version, *Colin McRae Rally 2.0* and later
games.

## 2. Download

Download `CMRLauncher-….zip` from the
[latest release](https://github.com/akitaonrails/rea-colin-mcrae/releases/latest)
(under **Assets**; older versions are on the
[Releases page](https://github.com/akitaonrails/rea-colin-mcrae/releases)) and
unpack it anywhere, for example into your game folder. It contains:

- `CMRLauncher.exe` — the launcher
- `cmrhooks.dll` — needed for the HUD at high resolutions; keep it next to the launcher
- `linux/` — `cmr-play`, the Linux helper for Wine and gamescope

On Linux without Steam you can instead download
`CMRLauncher-…-x86_64.AppImage`, which brings its own Wine (see below).

## 3. Play on Windows

1. Unpack the zip anywhere, for example into your game folder.
2. Double-click `CMRLauncher.exe`. The settings window opens.
3. If the game folder is empty, press **Browse…** and pick the folder with
   your `Rally.exe`; the window shows which version it found.
4. Choose a look: **Original 1998**, **Enhanced** (fitted to your screen),
   **Steam Deck** or **Custom**, then press **Play** (or Enter).

The first start takes a few seconds while the launcher makes its private copy
of the game. Your choices are remembered for next time.

> Windows support is new: the launcher has so far been tested under Wine and
> Proton. Whether it works or not, please
> [send a test report](https://github.com/akitaonrails/rea-colin-mcrae/issues/new?template=test-report.yml)
> with your `launcher.log` (see [Where things are](#where-things-are)).

## 4. Play on Linux

### AppImage (no Wine needed)

Download `CMRLauncher-…-x86_64.AppImage`, make it executable and run it:

```sh
chmod +x CMRLauncher-*-x86_64.AppImage
./CMRLauncher-*-x86_64.AppImage
```

It contains the launcher and a portable Wine (11.20), so nothing else has to
be installed. The settings window opens: press **Browse…** and pick your game
folder (your Linux files are under drive `Z:`), choose a look and press
**Play**. If [gamescope](https://github.com/ValveSoftware/gamescope) is
installed, the game runs in it at the right size (recommended, especially
with a rotated monitor); otherwise in a plain window. Its Wine prefix is
`~/.local/share/cmr-launcher/prefix` (change it with `CMR_PREFIX`).

### With Steam

1. Unpack the zip, for example into `~/Games/cmr-launcher`.
2. In Steam: **Games → Add a Non-Steam Game to My Library… → Browse** and pick
   `CMRLauncher.exe`.
3. Right-click it in your library → **Properties → Compatibility** → tick
   **Force the use of a specific Steam Play compatibility tool** and choose
   **GE-Proton** (tested with GE-Proton 11; Proton Experimental should work
   too).
4. Press **Play**. The settings window opens: press **Browse…** and pick
   your game folder (your Linux files are under drive `Z:`), choose a look
   and press **Play**. Everything is remembered for next time.

### Steam Deck

Not tested on a Deck yet; if you try it, please
[report how it went](https://github.com/akitaonrails/rea-colin-mcrae/issues/new?template=test-report.yml).
Follow **With Steam** above in Desktop Mode (GE-Proton is installed with
ProtonUp-Qt from Discover), then play from Game Mode:

- The settings window works with the touchscreen or the right trackpad.
  Pick **Steam Deck**: 1280×800 widescreen at 60 fps, easy on the battery.
- The buttons are **Modern** by default: A is the handbrake, the bumpers
  change gear, Y changes the view, the triggers are the pedals and Start
  pauses.
- In the game: **Options → Settings → Controller 1**, Joystick 1, Analogue
  joystick, and steering, acceleration and braking on Analogue (see
  [Gamepad](#gamepad)).
- Rumble should work: under Proton, Steam shows the Deck's controls to the
  game as an Xbox-style pad.

### With Wine and gamescope

If you have Wine and [gamescope](https://github.com/ValveSoftware/gamescope)
installed, the `linux/cmr-play` script runs the game in its own correctly
sized screen:

```sh
~/Games/cmr-launcher/linux/cmr-play
```

It shows the settings window first (with one more choice, the scaler: how
gamescope enlarges the picture), then starts the game in gamescope. See
[linux/README.md](linux/README.md).

## 5. Settings

Everything is optional; the defaults give you the original game, with
working gamepads and a window that closes properly.

The settings window covers all of them: **Play** has the game folder, the
look and the saves folder, **Picture** each picture setting and
**Controls** the gamepad options. They are kept in a text file,
`launcher.cfg` (see [Where things are](#where-things-are)), which you can
also edit by hand; lines starting with `#` are switched off. Starting
`CMRLauncher.exe` with any option (for example `--resolution 1920x1080`)
skips the window and plays straight away; `--no-window` does so with the
saved settings.

| Setting | What it does | Default |
| --- | --- | --- |
| `resolution = 1920x1080` | Races at this resolution, up to `3840x2160`. Leave the game's own **Resolution** option (in its graphics settings) on 640×480: that's the choice this replaces. | the game's own (up to 800×600) |
| `widescreen = true` | On a 16:9 screen, shows more to the sides instead of stretching the picture. | off |
| `draw_distance = 1.5` | Sees further down the stage, from `1.0` to `3.0`. Longer distances can flicker far away and cost speed. | off |
| `fps_limit = 60` | Caps the frame rate (10 to 1000, or `off`). The driving keeps its own fixed rate; only the redraws change. | off |
| `texture_filter = smooth` | `smooth` adds mipmaps and trilinear filtering (no shimmering far away); `sharp` shows crisp, unfiltered pixels everywhere; `original` keeps the game's. | the game's own |
| `smooth_distance = true` | Eases the view distance when a stage changes it between sections, so scenery doesn't pop in or out at once. | off |
| `vsync = true` | `true` waits for the screen's refresh (no tearing), `false` doesn't. | the game's own |
| `gamepad_axes = true` | Triggers and right stick work as accelerate/brake (with an Xbox-style pad, both triggers can be held at once); Start pauses. | on |
| `rumble = 60` | Gamepad rumble on impacts, strength 1–100. Needs an Xbox-style (XInput) pad. **Test** next to it in the window shakes the pad for half a second. | off |
| `buttons = original` | Pad buttons. `modern` (Xbox-style pads): A selects and is the handbrake, B goes back, Y changes the view, LB/RB change gear, Start pauses. `original`: the game's own 1998 order. Or eight pad button numbers for select, back, change view, gear down, handbrake, gear up, (unused), pause, e.g. `1,2,4,5,1,6,7,8`. The window's **Buttons** tab sets it. | `modern` |
| `close_quits = true` | Closing the window (Alt+F4) quits the game. | on |
| `saves = D:\Rally saves` | Where your saves are kept. | `Saved Games\Colin McRae Rally` in your user folder |
| `language = FRENCH` | Game language on first setup: ENGLISH, FRENCH, GERMAN, SPANISH or ITALIAN. | ENGLISH |
| `game = …` | Your game folder (filled in by `--game`). | — |

Every setting can also be given on the command line for one run, for
example in Steam's launch options:

```text
CMRLauncher.exe --resolution 1920x1080 --widescreen on --draw-distance 1.5
CMRLauncher.exe --resolution original          # back to the game's own sizes
CMRLauncher.exe --check --game "D:\Games\Colin McRae Rally"   # is this folder supported?
```

Flags: `--game`, `--saves`, `--runtime`, `--language`, `--resolution`,
`--widescreen on|off`, `--draw-distance`, `--fps-limit`, `--vsync on|off`,
`--texture-filter`, `--smooth-distance on|off`, `--rumble`,
`--gamepad-axes on|off`, `--close-quits on|off`. Only `--game` is remembered; the others override
`launcher.cfg` for that run. `CMRLauncher.exe --help` lists them.

### Gamepad

In the game: **Options → Settings → Controller 1**, set **Control method** to
your pad (Joystick 1) and **Controller type** to **Analogue joystick**, then set
steering, acceleration and braking to **Analogue** on the next screen.

Pick the buttons in the launcher window's **Buttons** tab (Modern is the
usual layout for today's pads); the d-pad moves through the menus. Under
Wine and Proton, **Run joystick panel** does nothing (it opens an old
Windows dialog that can't appear there, and the game used to wait for it).

To see what your pad sends to the game, run `CMRLauncher.exe --controllers`
and move every stick and button; the result is written to `launcher.log`.

## Where things are

| What | Where |
| --- | --- |
| Settings | `%LOCALAPPDATA%\CMRLauncher\launcher.cfg` (a `launcher.cfg` next to `CMRLauncher.exe` is used instead, for a portable setup) |
| Log | `%LOCALAPPDATA%\CMRLauncher\launcher.log` |
| The launcher's copy of the game | `%LOCALAPPDATA%\CMRLauncher\Rally` |
| Your saves | `%USERPROFILE%\Saved Games\Colin McRae Rally` (or your `saves` setting) |

Under Wine and Proton these are inside the Wine prefix (for Steam:
`~/.steam/steam/steamapps/compatdata/<id>/pfx/drive_c/users/steamuser/…`).

## Troubleshooting

- **The car doesn't move, or turns in circles.** The controls in your save
  point at a gamepad that isn't there. Plug the pad in, or set Control method
  back to Keyboard in Options → Settings → Controller 1.
- **The game crashes at start on a PC with a portrait (rotated) monitor.** The
  1998 game can't use a portrait main screen. Make a landscape monitor your
  main display, or on Linux use `linux/cmr-play` (gamescope gives the game a
  landscape screen).
- **Menus feel less smooth at 4K.** The menus are drawn at 640×480 and
  enlarged for your screen each frame; at 4K that copy is slow on some
  setups. 1440p or 1080p keeps them smooth; races aren't affected.
- **Steam/Proton: the game crashes right away with an NVIDIA card.** Some
  setups only give 32-bit programs working OpenGL through Proton's WoW64
  mode. Add `PROTON_USE_WOW64=1` in front of `%command%` in the launch
  options.
- **Anything else:** open an issue and attach `launcher.log`.

## Known limitations

- The menus are enlarged on the CPU; at 4K with software rendering they run
  at about 44 fps (on a GPU the copy costs much less).
- A short white flash before the legal screen at high resolutions.
- Split screen isn't adjusted for widescreen.
- Only the European v1.5 `Rally.exe` is recognised so far.

## For developers

```text
cargo test                                          # host tests
cargo build --release --target i686-pc-windows-gnu  # needs i686-w64-mingw32-gcc;
                                                    # builds CMRLauncher.exe and cmrhooks.dll
```

GitHub Actions builds the Windows package on every push and publishes a
release for tags named `v…`. How it works and what's left:
[docs/THIN_APP_PLAN.md](docs/THIN_APP_PLAN.md) (fix list and status),
[docs/GRAPHICS.md](docs/GRAPHICS.md), [docs/RUNTIME_NOTES.md](docs/RUNTIME_NOTES.md).
Rules for contributors are in [AGENTS.md](AGENTS.md).

The game fixes are applied in memory to the running game, keyed to the exact
`Rally.exe`; nothing is patched on disk.

## Legal

Colin McRae Rally is © Codemasters. This project is not affiliated with or
endorsed by Codemasters or Electronic Arts, contains no game code or assets,
and only runs a copy you own. Our code is MIT or Apache-2.0
([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE)); bundled
third-party parts and the launcher photo (Carolyn Williams, CC BY 2.0) are
listed in [THIRD_PARTY.md](THIRD_PARTY.md).
