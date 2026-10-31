# Upscaling the 2D art (study)

Status: **works in the game and ships with the launcher** (`hd_art = on`,
part of the Enhanced and Steam Deck looks; F27) for the race HUD and
the front end's images (logos, flags, buttons, car pictures); full-screen
pictures larger than the game's 640x480 (the menu panorama) are not shown. This
page records what was learned, the tool that extracts and upscales the
images, how the game shows them, and what is left. It is written for
whoever picks this up next.

## What is meant

The game's 2D art was drawn for 640x480: the intro and menus, the loading
and legal screens, the front end's buttons, flags and car pictures, and
the race HUD (dials, pace-note arrows, positions). Today the launcher
enlarges it with sharp pixels (the menus as a whole picture, F16/E5; the
race HUD rectangle by rectangle, F13). The idea: replace each image with
an AI-upscaled version made on the player's machine, and draw it at the
screen's real size.

## Ground rules

- **The originals are never committed or shipped** (see `AGENTS.md`).
  The HD art pack, made from them, is the one exception (owner decision,
  2026-10-31): it is published as a release asset and bundled with
  releases, never committed.
- The game folder is only read.
- Real-ESRGAN (`realesrgan-ncnn-vulkan`, BSD-3-Clause) is used to make the
  pack and is not shipped; players get the finished pack.

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
- **Real-ESRGAN's folder mode is broken.** `realesrgan-ncnn-vulkan`
  20220424, given a folder, saved many results under other images' names
  (same size, different picture) or as noise, differently on every run
  (RTX 5090, NVIDIA driver; one thread per stage did not help). One image
  per run is always right, but each run spends about 5 s building its GPU
  pipelines. So `cmr-hd` packs each model's images into sheets of up to
  2048x2048, each image in a 16-pixel gutter of its own edge pixels (more
  than the model's reach), runs once per sheet and cuts the results out
  (`src/atlas.rs`). The 233 HUD images make one sheet: about 35 s.
- **Flat art is redrawn, not upscaled.** The AI models draw the
  original's one-pixel staircases as wavy or still-stepped edges. Flat art
  (the HUD, and interface pieces of 32 colours or fewer: logos, flags,
  icons) is instead redrawn with smooth outlines by the Kopf–Lischinski
  "Depixelizing Pixel Art" method (`src/depixel.rs`, ported from the MIT
  depixel-js): it joins the pixels into regions, fits curves through their
  staircases and keeps one-pixel lines connected, as if traced by hand in a
  vector editor. Real-ESRGAN stays for shaded art (car renders, photos).
  Each image is first continued past its border by its own edge pixels
  and cut back after the redraw: the game joins sprites edge to edge and
  stretches thin tiles (the HUD frame is 4x4 bar tiles stretched along
  the screen, meeting 16x16 corners), so a line that runs off one piece
  has to leave it straight and at full width to meet the next. Traced
  inside its own borders, the frame's corner bent into an S and swelled
  where it met the bars.
- **The upscalers redraw colours.** On the HUD the artwork model turned
  one-pixel grey outlines into thin dark lines and drew the original's
  one-pixel staircases as wavy edges. Every result is therefore held to
  the original (`src/lock.rs`):
  - *HUD textures* (kept as 8-bit palette indices by the game): each
    pixel of the redrawn version takes one of the colours the
    original has within two pixels of the spot, the one the (lightly
    smoothed) upscale is closest to, favouring nearer and more common
    ones. Colours stay exactly the artist's, outlines keep their width,
    and lone anti-aliasing pixels at the edges drop out; the upscale only
    decides where the edges run. Guiding by a blurred enlargement instead
    of the model was tried and broke thin lines (tick marks) into dashes.
  - *pictures and interface*: back-projection. The upscale is corrected
    eight times by the (smoothly spread) difference between the original
    and the upscale averaged back to the original's size, so colours and
    shading match the original and the added detail stays.
- **Two sets, chosen by resolution.** The game's 2D is laid out for 480
  lines: 1080p needs 2.25x, 1440p 3x, 4K 4.5x. `cmr-hd upscale` writes `x4/`
  (the model's 4x, for 1440p and up) and `x2/` (the 4x made half as large
  with Lanczos, sharper than enlarging the original twice, for 1080p). The
  runtime picks the smallest set at least as large as the screen needs,
  else `x4`, and the GPU's filtered stretch does the rest.

## The tool

Real-ESRGAN runs once per sheet (see Findings). Flat art skips it
(`depixel`). All 912 images, both sets: about 18 minutes.


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
- `src/atlas.rs`: packing images into sheets for the upscaler, with tests.
- `src/lock.rs`: holding results to the original's colours, with tests.
- `src/depixel.rs`: the smooth-outline redraw for flat art, with tests
  checked against depixel-js's own output.
- `src/palette.rs`: the 8-bit runtime copies, in the original's palette.
- `manifest.tsv` records each image's path, class, the SHA-256 of the game
  file it came from, its size and the model. A runtime loader must check
  the digest, so a different game version never gets the wrong picture.

Get Real-ESRGAN from its releases (the package with `models/`), e.g.
`realesrgan-ncnn-vulkan-20220424-ubuntu.zip` from
<https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0>.

## Showing them in the game

The general mechanism follows the rule of fixing a whole class in one
place: **keep the game's own textures, sizes and layout exactly as they
are, and swap pixels only where they are drawn.**

### The race HUD (done, F27)

The HUD is drawn as Direct3D textured quads, not with the 2D blit (which
the race never calls).

1. **Files.** `cmr-hd upscale` writes, for each HUD image, a BMP named by
   the FNV-1a 64 hash of the game's own BMP bytes (`crates/cmr-hooks/src/hd.rs`,
   shared by the tool and the hook library), in `x2/` and `x4/`. An 8-bit
   original gets an 8-bit copy in its own palette (the game keeps 8-bit
   textures as palette indices, with index 0 the colour key); others get
   24-bit.
2. **Load.** F27 wraps the game's bitmap loader `0x499ec0(bmp, flags)`,
   which every image goes through. For a texture (flags bit 1) with a file
   in the HD set (`CMR_HD_ART`, which the launcher points at `x2` up to
   1080p, else `x4`) it loads that file through the same loader: the twin.
   The game's own texture is untouched.
3. **Bind.** Every texture bind goes through the device's
   `SetRenderState(TEXTUREHANDLE, h)` (slot 23 of the `IDirect3DDevice2`
   at `[[0x50283c]+0x10]`). F27 takes that one slot over: a game handle with
   a twin binds the twin's handle instead, and switches that draw to
   smooth (bilinear) sampling so the HD edges come out as straight lines;
   the game's own filters return with its next texture. The texture
   coordinates are fractions of the picture, so the twin needs no change
   to the layout. Pairs are checked against each record's current handle
   at every bind and forgotten outside races.
4. **Checked** headless at 1080p and 4K: 35 pairs, about 6000 twin binds
   per race, none stale, 60 fps at 4K.

### The front end (done, F27)

The menus are one 640x480 picture made of blits, enlarged to the screen
each frame.

1. **Load.** Their images load as plain surfaces through
   `0x499320(record, bmp, flags)`; F27 loads a twin beside each image
   that has an HD file (`cmr_hd_load_2d`).
2. **Note.** F13's hook at the entry of the blit `0x49e3e0` notes every
   blit of the frame: image, source and destination rectangles, colour
   key, and the source rectangle in the twin's pixels.
3. **Compose** (`crates/cmr-hooks/src/fe_compose.rs`). At present time
   the frame's blits are replayed to know, for each picture pixel, which
   image drew it last and which one is under it; pixels whose replayed
   colour isn't the picture's were drawn some other way (the menu's
   globe) and keep the picture. While the picture is enlarged, a screen
   pixel owned by an image with a twin is taken from the twin, its
   transparent edges from the image under it. Backgrounds, overlaps and
   animation compose in the game's own order. The game's and the twins'
   pixels are cached once per load; at 4K the menu runs at about 40 fps.

### The draw trace

`CMR_HD_TRACE=<file>` makes the hook library write each distinct HUD
sprite (`hud`, from the sprite queue `0x49b420`) and front-end blit (`fe`)
once, as `<runtime name> src x y w h dst x y w h` in the game's 640x480.
It shows which images make up each screen (the race loading screen is the
country's `Game/Welcome/*/Flag_*.BMP`, the main menu the panorama
`Backdrop/Main.bmp` panned), and where every HUD piece goes: enough to
rebuild the whole HUD as one picture, redraw it in one piece and cut the
pieces out again, so that joints line up.

### The pack in releases

The HD set is 1.5 GB of BMPs; releases carry it compactly instead:

1. `cmr-hd pack <hd> <pack>` stores each runtime BMP as a palette PNG (the
   HUD's 8-bit textures, exact) or a colour image: a lossless PNG, or a
   near-lossless WebP (quality 92, about 41 dB) when that is much smaller.
   Lossless files are checked to give back the same pixels. About 38 MB.
2. The zipped pack is published once as a release asset (`hd-pack-1`).
   `packaging/hd-pack.txt` pins its URL and SHA-256; CI downloads and
   checks it and puts it in the zip and the AppImage as `hd/`, next to
   `CMRLauncher.exe`.
3. With `hd_art = on`, the launcher installs the pack into the runtime
   folder (`<runtime>/hd/<id>/x2`, `x4`, `crates/cmr-launcher/src/hdpack.rs`
   with the format in `crates/cmr-hdpack`), once per pack, in a few
   seconds, and points the hook library at the set for the resolution.

To make a new pack: `cmr-hd extract`, `cmr-hd upscale`, `cmr-hd pack`,
zip `pack.txt`, `x2/` and `x4/`, publish it as `hd-pack-<n>`, and update
`packaging/hd-pack.txt`.

### Next

- **Fonts** (`*.fon`, `ini/cdFONT.BMP`, the race timer's digit sheets) are
  a separate class; text could be redrawn from a vector font.
- **Set-up in the launcher**: run the extraction and upscaling once from
  the settings window, with progress, into the launcher's data folder
  (bundling Real-ESRGAN needs a `THIRD_PARTY.md` row).
- **Speed.** The whole set takes about 18 minutes on a 16-core CPU, mostly
  the back-projection of the large pictures; it is plain single-threaded
  code and would split across cores easily.

Things to watch:

- Palette effects: a few front-end screens fade or cycle palettes; check
  that HD copies follow when the palette changes.
- Animated sprites made from strips (digits, LEDs) keep working as long
  as every frame is upscaled in one image (they are).
- 3D textures (cars, stages) are out of scope: they live inside `.c3d`
  files.
