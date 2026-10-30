# Upscaling the 2D art (study)

Status: **study, with a working offline pipeline**. The game does not load
upscaled images yet; this page records what was learned, the tool that
extracts and upscales the images, and the plan to show them in the game.
It is written for whoever picks this up next.

## What is meant

The game's 2D art was drawn for 640x480: the intro and menus, the loading
and legal screens, the front end's buttons, flags and car pictures, and
the race HUD (dials, pace-note arrows, positions). Today the launcher
enlarges it with sharp pixels (the menus as a whole picture, F16/E5; the
race HUD rectangle by rectangle, F13). The idea: replace each image with
an AI-upscaled version made on the player's machine, and draw it at the
screen's real size.

## Ground rules

- **Nothing derived from the game is ever committed or shipped**, not the
  images, not their upscaled versions, not comparison screenshots (see
  `AGENTS.md`). The tool reads the player's own game folder and writes to
  a folder of their choice, like the launcher's private runtime copy.
- The game folder is only read.
- The upscaler is open source and would be bundled like any other
  component: Real-ESRGAN (`realesrgan-ncnn-vulkan`, BSD-3-Clause, with its
  released models) runs on any Vulkan GPU on Windows and Linux. Bundling it
  needs a `THIRD_PARTY.md` row; the study uses a local download only.

## What the game has

Measured on the official European v1.5 (`cmr-hd list`):

| Class | Images | Pixels | Where |
| --- | ---: | ---: | --- |
| picture | 105 | 8.7 M | `New_Fe/Images/{Backdrop,cars,Country,Thumbs,Load}`, `Game/Welcome`, `Game/Images/Loading`, the full-screen `ini/*.bmp` (loading, legal) |
| interface | 574 | 19.3 M | the rest of `New_Fe/Images` (buttons, sliders, LEDs, panels, flags, logos), `Game/SetRep` |
| hud | 233 | 1.5 M | `Game/Images/osdnew` (`high/`, `low/`, `revs/` sets) |
| skip | 1096 | 7.0 M | effect and 3D textures (`Game/Images/{Dust,Grass*,Snow*,...}`), small sprites drawn as textures, font sheets |

Notes:

- **Storage.** Most files are a gzip stream whose payload is `CMPR`, a
  little-endian length, then an ordinary Windows bitmap; a few are plain
  bitmaps. The game reads both through one loader (`FUN_00495ed0`). Most
  images are 8-bit palettized (256 colours); the game shows them through a
  16-bit (RGB565) screen (F2).
- **Sizes.** Full screens are 640x480; the main menu backdrop is a 2244x960
  panorama that the menu pans across; HUD pieces are small (32x32 tiles,
  64x16 digit strips, a 256x128 pace-note sheet).
- **Transparency.** Sprites use pure black as the colour key (the blit
  path's key 0). A plain upscale would smear black into dark halos, so the
  tool turns black into alpha first and restores an exact key afterwards
  (opaque pixels are nudged off pure black so the key still matches).

## Findings

- **No single model suits everything.** On the menu art the general model
  (`realesrgan-x4plus`) gives crisp lettering but invents a cracked-leather
  texture on the carbon-fibre weave; the artwork model
  (`realesrgan-x4plus-anime`) gives the cleanest edges and text but smooths
  the weave flat. Photographs and painted screens suit the general model;
  interface pieces and the HUD suit the artwork model. `cmr-hd` picks by
  folder (`crates/cmr-hd/src/classes.rs`); per-image overrides are the
  obvious next step.
- **The HUD upscales very well**: the pace-note arrows come out crisp with
  clean borders and no halo, at 4x.
- **Fine dithered patterns** (the carbon weave, the 8-bit dithering in
  gradients) are where the models disagree with the artist. Options to try:
  blend the upscale with a plain Lanczos enlargement, or keep those few
  images at sharp pixels.
- **Speed.** Real-ESRGAN loads its model once per run (seconds), so the
  tool runs each model once over a folder: the 233 HUD images take about
  12 s on an RTX 5090; all 912 images, both sets, 83 s (68 MB for `x4/`,
  28 MB for `x2/`). One-off, at set-up time.
- **Two sets, chosen by resolution.** The game's 2D is laid out for 480
  lines: 1080p needs 2.25x, 1440p 3x, 4K 4.5x. `cmr-hd upscale` writes `x4/`
  (the model's 4x, for 1440p and up) and `x2/` (the 4x made half as large
  with Lanczos, sharper than enlarging the original twice, for 1080p). The
  runtime picks the smallest set at least as large as the screen needs,
  else `x4`, and the GPU's filtered stretch does the rest.

## The tool

`crates/cmr-hd` (a host program; not part of the launcher package):

```sh
cargo build --release -p cmr-hd
cmr-hd list    "<game folder>"
cmr-hd extract "<game folder>" <work>/extracted          # PNGs + manifest.tsv
cmr-hd upscale <work>/extracted <work>/hd --esrgan <path>/realesrgan-ncnn-vulkan
#              writes <work>/hd/x4/... and <work>/hd/x2/...
```

- `src/asset.rs`: the stored-file container (gzip, `CMPR`), with tests.
- `src/classes.rs`: which folders are pictures, interface or HUD, and the
  model for each, with tests. Change the rules here.
- `src/key.rs`: the colour key to alpha and back, with tests.
- `manifest.tsv` records each image's path, class, the SHA-256 of the game
  file it came from, its size and the model. A runtime loader must check
  the digest, so a different game version never gets the wrong picture.

Get Real-ESRGAN from its releases (the package with `models/`), e.g.
`realesrgan-ncnn-vulkan-20220424-ubuntu.zip` from
<https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0>.

## Showing them in the game (the plan)

The general mechanism follows the rule of fixing a whole class in one
place: **keep the game's own surfaces, sizes and layout exactly as they
are, and swap pixels only where 2D is drawn.**

1. **Load.** Where the game turns a loaded bitmap into a DirectDraw surface,
   look the file up in the HD set (by game path and the manifest's
   SHA-256). If there is one, create a *shadow* surface at the HD size and
   remember the pair (game surface -> shadow, factor). The game keeps its
   original surface and sizes, so its layout code sees nothing new.
2. **Draw.** The 2D primitives F13 already wraps (sprite queues
   `0x49b420`/`0x49b1d0`, blit `0x49e3e0`, panel `0x49e200`) scale every
   destination rectangle to the real screen. When the source surface has a
   shadow, draw from the shadow instead, with the source rectangle
   multiplied by its factor. One place, every HUD element.
3. **Front end.** The menus are drawn into a 640x480 picture that F16/E5
   enlarges as a whole, which has no per-image step to hook. To get HD
   menus, the front end moves to the race's model: a virtual 640x480
   with rectangles scaled at the same primitives (F13's design), drawn
   into the real-size back buffer. Then step 2 covers the menus too, and
   any image without an HD version is still enlarged as today.
4. **Settings.** An opt-in `hd_art` setting (default off, the faithful
   game first) and a set-up step in the launcher window that runs the
   extraction and upscaling once, with progress, into the launcher's data
   folder.

Things to watch:

- Palette effects: a few front-end screens fade or cycle palettes; the
  16-bit path (F2) already converts at load time, so check that HD copies
  follow the same conversion when the palette changes.
- Animated sprites made from strips (digits, LEDs): the factor applies to
  the source rectangle, so strips keep working if every frame is upscaled
  in one image (they are).
- Fonts (`*.fon`, `ini/cdFONT.BMP`) are a separate format and class; text
  could later be redrawn with a vector font instead.
- 3D textures (cars, stages) are out of scope here: they live inside
  `.c3d` files and go through Direct3D, not the 2D blits.
