# HD 2D art

Status: **ships with the launcher** (`hd_art = on`, part of the Enhanced
and Steam Deck looks; fix F27). The pack (`hd-pack-3`) covers the race
HUD, the intro and legal screens, the main menu (panorama, ring words,
selected word), the race loading screens (`Game/Welcome`), every stage's
sky and distant scenery, and the front end's flat art (logos, flags,
icons, buttons of 32 colours or fewer).
Everything else is still drawn from the game's own 640x480 art. This page
says what works, what doesn't, how to rebuild the pack and how to extend
it. It is written for whoever picks this up next.

## Ground rules

- **The originals are never committed or shipped** (see `AGENTS.md`).
  The HD art pack, made from them, is the one exception (owner decision,
  2026-10-31): it is published as a release asset and bundled with
  releases, never committed. Nor are redrawn images, the extracted
  images or the HD sets: keep them under `.work/`.
- The game folder is only read.
- **Faithful first.** An HD image must be the same design: same layout,
  text, numbers, colours and lighting, only sharper. A redraw that is
  prettier but different is a bug (one panorama redraw turned the dial's
  "50" into a second "60"). Check every redraw against its original
  before it goes in a pack.

## What works and what doesn't

The game's 2D art falls into four kinds, and each needs its own method.
What was tried, on the official European v1.5:

| Art | Method that works | Tried and dropped |
| --- | --- | --- |
| **Flat art**: the HUD (dials, rev bar, frame, pace notes), logos, flags, icons | `depixel` in `cmr-hd` (Kopf–Lischinski "Depixelizing Pixel Art"): smooth vector-like outlines in the exact original colours, deterministic, local | Real-ESRGAN (stepped or wavy diagonals, dark halos); redrawing each piece with an image model (pieces that join on screen stop lining up) |
| **Lettering on a curve**: the main menu's ring of words | `tools/hd-redraw/ring.py`: unwrap the ring, redraw the frame's words together on one sheet, wrap back | asking the model for the rotated ring (garbled and mirrored words); one word per request (the model swaps the condensed typeface for a generic one); a smoothed unwrap as input (soft, ghosted letters) |
| **Large detailed pictures**: the menu panorama, the loading screens | `tools/hd-redraw/tiles.py`: redraw in 320-pixel tiles shown 4x, keep only fine detail from the redraw and the tone from the original | redrawing the whole picture in one request (invents a different texture, changes numbers, adds objects, moves faint lines); Real-ESRGAN (invents a cracked-leather texture on the carbon weave, or flattens it) |
| **Small pictures and HUD sprites**: full-screen `ini/` screens, menu labels, the pace-note signs, the rev dial, car silhouettes | `tools/hd-redraw/redraw.py` (one request per image, best of three), then `filter.py` and `align.py`; HUD textures are then held to the original's colours and transparency by `cmr-hd` | importing HUD redraws as they come (the model's near-black arrow became the colour key, so the signs were see-through, and their fills drifted) |
| **Car icons** (the race's positions board, `osdnew/*/common/<car>.bmp`, `car1-4`) | `tools/hd-redraw/sheet.py`: all of them on one sheet in one request, cut back out; a few sheets, then `pick.py` takes each icon from the first sheet whose writing is real | one icon per request (the model drifted in style and invented a race number); a sheet taken whole (made-up sponsor words on two cars) |
| **Manufacturer badges and flags of the race's own screens** (stage results: `Game/Images/LOGOS2.BMP`, `CUNTLOGO.BMP`) | `redraw.py` on the whole sheet (class `hud` through `RACE_SHEETS`) | — |
| **HUD pieces the game joins on screen**: the rev bar's segments (`2wheel/rev0*`) | `depixel` (`redraw.py` skips them) | the image model (kept the staircases or broke the pieces apart) |
| **Car textures** (the bitmaps inside `Objects/Cars/**/*.c3d`: UV atlases of the livery, glass, wheels and cockpit) | `redraw.py` (class `car`, an atlas prompt: everything in exactly the same place, writing legible and real), best of three by drift, then `pick.py`; back-projected onto the original by `cmr-hd`; reviewed by eye | spinning wheels (redrawn, they show still spokes at speed) and plain panels (the model engraves details) keep the original: `redraw.py` finds them by their angular detail against a sibling; some tries add parts (a spare wheel, a bonnet scoop) |
| **Trackside signs** (start and finish banners, sponsor billboards, rally plates, road and message boards: among the bitmaps inside each stage's `*.obj`, class `scenery`) | `tools/hd-redraw/signs.py`: a vision model picks the signs out of the 1372 trackside textures (164 are signs), says how each turns to read and transcribes its writing; each is redrawn upright with that exact writing in the prompt; two passes, then `pick.py`; back-projected by `cmr-hd` | redrawing without the transcription (the model misread SNR as SMX, doubled letters: New ZeZealand); taking a single pass (some banners came back upside down or with doubled words) |
| **Sky and distant scenery**: the bitmaps inside each stage's `*.hzn` (two bands that make the horizon panorama, stored on their side, and a ground texture that tiles) | `tools/hd-redraw/sky.py`: bands turned upright and joined into one panorama, redrawn soft and hazy in one request, tone from the original, borders faded back to it; single-colour textures and redraws that stray far (drift above 50) keep the original | redrawing single-colour textures (night grounds: the model scatters speckles); one stage's sky came back smeared and one city skyline blurred (a second try was right) |

Not done yet: the spinning globe and its flag strip (drawn by a path that
isn't a blit, see "The front end" below), fonts and the race timer's
digit sheets, the car and country pictures and thumbnails (pack 1 had
Real-ESRGAN versions; they were dropped as not faithful enough).

### Flat art: `depixel`

- `crates/cmr-hd/src/depixel.rs` (a Rust port of the MIT depixel-js,
  checked against its output) joins the pixels into regions, fits curves
  through their staircases and keeps one-pixel lines connected.
- HUD textures are then held to the original's palette (`src/lock.rs`):
  each pixel takes one of the colours the original has nearby, so colours
  and line widths stay the artist's.
- **Edges continue.** Each image is first extended by its own edge
  pixels and cut back after the redraw. The game joins sprites edge to
  edge and stretches thin tiles (the HUD frame is 4x4 bar tiles stretched
  along the screen, meeting 16x16 corners), so a line that runs off one
  piece has to leave it straight and at full width. Traced inside its own
  borders, the frame's corner bent into an S where it met the bars.
- `cmr-hd` picks it for the HUD (`Game/Images/osdnew`) and for interface
  pieces of 32 colours or fewer (`src/classes.rs`).

- **Held to the original, whatever made it.** `cmr-hd` holds every 8-bit
  texture (HUD, car) to the original (`held` in `src/main.rs`) and takes
  its transparency from the original: inside the original's opaque area
  (a pixel and its eight neighbours opaque) the result is opaque, so a
  redraw's black can't become the colour key; only near the original's
  own edges does the redraw shape the outline. Flat redraws (depixel)
  take, pixel by pixel, one of the original's colours nearby. An image
  model's redraws keep their own edges and shading: HUD art only takes
  the original's tone (`lock::tone`, colours smoothed over 3 original
  pixels), because it is drawn several times its size and anything
  stricter shows the original's pixels again (the car icons and badges
  came out as enlarged pixel art under the per-pixel lock and under
  back-projection); car textures, wrapped on the car near their own
  size, are back-projected, so each original pixel's area averages to
  its colour, which also wipes most of what a redraw invents on a plain
  surface.
- **Writing must be real.** `redraw_api.writing_ok` asks a vision model
  (`gemini-3.5-flash`) whether every word and number in a redraw is real,
  correctly spelled and makes sense there; `pick.py` takes, per image, the
  first candidate that passes (several runs into separate folders), and
  leaves the original where none does. It doesn't know whether a real
  sponsor name is the right one: look.

### Image-model redraws (Nano Banana 2)

`tools/hd-redraw/` calls Google's `gemini-3.1-flash-image` ("Nano Banana
2") image-to-image, at 4K. Lessons that hold for all of it:

- **Show the model big pixels.** Send small art enlarged with nearest
  neighbour to about 1000–1500 pixels. Sent small, the model copies the
  staircases; sent blurred, it draws soft, ghosted shapes.
- **Give it context.** The model keeps a typeface or texture when it sees
  several samples at once: all of a frame's words on one sheet, not one
  word per request.
- **Keep the request small enough to copy.** A whole 2244x960 panorama
  is reinterpreted; 320-pixel tiles are copied. Large pictures go in
  tiles with 64 pixels of overlap, feathered together.
- **Never trust its colours.** `tiles.py` replaces everything below
  1.5 original pixels (tone, lighting, glow) with the original's and keeps
  only the redraw's fine detail; `redraw.py` results are checked for
  drift (`filter.py`) and aligned to the original (`align.py`).
- **Transparency.** Ask for a background that can't be confused with the
  art. The ring words have a dark shadow, so they are drawn on pure green
  and unmixed: the model never paints exactly #00FF00, so the green is
  estimated from pixels far from any ink and smoothed, then
  alpha = 1 − (G − (R+B)/2) of the pixel over that of the background.
  Other keyed images use black (the game's colour key).
- **Pick the best of three tries** by a measure that ignores what the
  model is allowed to change: RGB drift at the original's size for
  pictures, the shape of the ink for lettering.
- **Look at every result** next to its original, at the size the game
  shows it. Drift numbers miss a changed digit.
- Cost: pack 2 took about 130 requests (each tile and each try is one);
  the skies of pack 3 about 100 more.

## The pipeline

Tools: `crates/cmr-hd` (Rust, host program) and `tools/hd-redraw`
(Python 3 with numpy, scipy and Pillow: `pip install -r
tools/hd-redraw/requirements.txt`). The redraw scripts need
`GEMINI_API_KEY` in the environment and never print it. Put all output
under `.work/`.

```sh
cargo build --release -p cmr-hd
W=.work/hd
cmr-hd extract "$CMR_GAME_DIR" $W/extracted            # PNGs + manifest.tsv

# Flat art (no model, no key): the HUD, logos, flags, icons.
cmr-hd upscale $W/extracted $W/set                      # writes set/x4, set/x2

# Small pictures and menu pieces, then check and align them.
python tools/hd-redraw/redraw.py  $W/extracted $W/redrawn
python tools/hd-redraw/filter.py  $W/extracted $W/redrawn
python tools/hd-redraw/align.py   $W/extracted $W/redrawn $W/final

# The menu ring words, one frame at a time (seven frames).
A=New_Fe/Images/MainOpts/English/Anim
for f in 0 1 2 3 4 5 6; do
  python tools/hd-redraw/ring.py $W/extracted/$A/english$f.bmp.png $W/final/$A/english$f.bmp.png
done

# Large pictures in tiles: the panorama (only the part the menu shows, at
# 3x so it stays under the game's 8192-pixel limit) and the loading screens.
python tools/hd-redraw/tiles.py $W/extracted/New_Fe/Images/Backdrop/Main.bmp.png \
  $W/final/New_Fe/Images/Backdrop/Main.bmp.png 3 600 0 1832 760
for f in $W/extracted/Game/Welcome/*/Flag_*.png; do
  out=$W/final/${f#$W/extracted/}; mkdir -p "$(dirname "$out")"
  python tools/hd-redraw/tiles.py "$f" "$out" 4
done

# Every stage's sky and distant scenery (stages that share a sky once).
python tools/hd-redraw/sky.py $W/extracted $W/final

# The race's car icons, together on one sheet; the race screens' logo
# sheets; a car's textures (review each one, see the table above).
I='^Game/Images/osdnew/(high|low)/common/(audi|celica|ford|golf|lancia|mitsbshi|mk2|mr4|rs200|seat|skoda|subaru|toyota|car[1-4])\.'
for n in 1 2 3; do python tools/hd-redraw/sheet.py $W/extracted $W/icons$n cars "$I"; done
python tools/hd-redraw/pick.py $W/final $W/icons1 $W/icons2 $W/icons3
python tools/hd-redraw/redraw.py $W/extracted $W/final Game/Images/LOGOS2.BMP Game/Images/CUNTLOGO.BMP
for n in 1 2; do python tools/hd-redraw/redraw.py $W/extracted $W/cars$n Game/Objects/Cars/; done
python tools/hd-redraw/pick.py $W/final $W/cars1 $W/cars2

# The trackside signs: classified and transcribed once (signs.tsv; copy
# it into the second folder), redrawn twice, picked.
python tools/hd-redraw/signs.py $W/extracted $W/signs1
mkdir -p $W/signs2 && cp $W/signs1/signs.tsv $W/signs2/
python tools/hd-redraw/signs.py $W/extracted $W/signs2
python tools/hd-redraw/pick.py $W/final $W/signs1 $W/signs2

# Into the set (a redraw replaces the same image's depixel version), then
# the pack.
cmr-hd import $W/extracted $W/final $W/set
cmr-hd pack $W/set $W/pack
```

`cmr-hd upscale --esrgan <realesrgan-ncnn-vulkan>` also runs Real-ESRGAN
on the classes that aren't flat art (see "Real-ESRGAN" below); packs 2
and 3 don't use it.

`cmr-hd`'s files:

- `src/asset.rs`: the stored-file container (gzip, `CMPR`), with tests.
- `src/classes.rs`: which folders are pictures, interface or HUD, and
  which are flat art, with tests.
- `src/key.rs`: the colour key to alpha and back, with tests.
- `src/depixel.rs`, `src/lock.rs`: the flat-art redraw and the colour
  lock, with tests.
- `src/atlas.rs`: packing images into sheets for Real-ESRGAN, with tests.
- `src/palette.rs`: the 8-bit runtime copies, in the original's palette.
- `src/pack.rs`: the compact pack (below).
- `manifest.tsv` records each image's path, class, the SHA-256 of the
  game file it came from, its size and the method. `upscale` and
  `import` add to a set's manifest, replacing rows for the same path; an
  import also marks every other path of the same game file (the same
  runtime name, so the same twin), such as the HUD sheets the game keeps
  in several folders.
- A stage's sky and ground are whole bitmaps inside its horizon model
  (`*.hzn`, a stored `CM3D`), and so are a car's textures (`*.c3d`) and a
  stage's trackside objects (`*.obj`); `extract` lists them as
  `<path>.<ext>/<n>.bmp` (`asset::embedded_bitmaps`), classes `sky`,
  `car` and `scenery`. The game loads
  them through the same loader as the HUD's textures, so F27 shows their
  twins with no hook of its own. Car textures are colour-keyed 8-bit
  (a palette entry equal to entry 0's colour is transparent), so their
  twins keep the original's palette like the HUD's.

## How to extend it

1. **Find what a screen draws.** Run the game with
   `CMR_HD_TRACE=<file>` (see "The draw trace"): every distinct HUD sprite
   and front-end blit is listed once with its runtime name (the hash in
   `manifest.tsv`'s sixth column), source and destination rectangles.
   Map the names to paths through the manifest. Draws that aren't in the
   trace (the globe) need a new hook, not new art.
2. **Pick the method** from the table above. Flat art needs nothing but
   `cmr-hd upscale`. For a new kind of art, start from the closest script:
   - New lettering that isn't straight: reuse `ring.py`'s idea (map it to
     straight text, redraw all of it on one sheet, map back).
   - A large or detailed picture: `tiles.py <in> <out> <scale> [box]`;
     restrict the box to what the game shows (the trace says).
   - Anything else: `redraw.py <extracted> <out> <path prefix>`.
3. **Scale.** 4x for 640x480 art; 3x for images wider than 640 (a twin's
   side must stay at or below 8192 pixels, `MAX_TWIN` in `cmr-hd`). The
   launcher picks the `x2` set up to 1080p and `x4` above; `import` makes
   `x2` from the redraw.
4. **Check it** next to the original at the size the game shows it, then
   in the game headless at 1080p and 4K (the front end should stay near
   40 fps at 4K, races at 60).
5. **Ship it**: `cmr-hd import` into the set, `cmr-hd pack`, then publish
   as below.

## Showing them in the game

The general mechanism follows the rule of fixing a whole class in one
place: **keep the game's own textures, sizes and layout exactly as they
are, and swap pixels only where they are drawn.** Twins are BMPs named by
the FNV-1a 64 hash of the game's own BMP bytes
(`crates/cmr-hooks/src/hd.rs`, shared by the tool and the hook library),
in `x2/` and `x4/`.

### The race HUD (F27)

The HUD is drawn as Direct3D textured quads, not with the 2D blit (which
the race never calls).

1. **Files.** An 8-bit HUD original gets an 8-bit twin in its own
   palette (the game keeps 8-bit textures as palette indices, with index
   0 the colour key); others get 24-bit.
2. **Load.** F27 wraps the game's bitmap loader `0x499ec0(bmp, flags)`,
   which every image goes through. For a texture (flags bit 1) with a file
   in the HD set (`CMR_HD_ART`) it loads that file through the same
   loader: the twin. The game's own texture is untouched.
3. **Bind.** Every texture bind goes through the device's
   `SetRenderState(TEXTUREHANDLE, h)` (slot 23 of the `IDirect3DDevice2`
   at `[[0x50283c]+0x10]`). F27 takes that one slot over: a game handle
   with a twin binds the twin's handle instead, and switches that draw to
   smooth (bilinear) sampling; the game's own filters return with its next
   texture. Texture coordinates are fractions of the picture, so the twin
   needs no change to the layout. Pairs are checked against each record's
   current handle at every bind and forgotten outside races.

### The front end (F27)

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
   transparent edges from the image under it. Backgrounds, overlaps,
   the panning panorama and animation compose in the game's own order.
   Pixels are cached once per load; at 4K the menu runs at about 40 fps.

### The draw trace

`CMR_HD_TRACE=<file>` makes the hook library write each distinct HUD
sprite (`hud`, from the sprite queue `0x49b420`) and front-end blit (`fe`)
once, as `<kind> <runtime name> src x y w h dst x y w h` in the game's
640x480. It shows which images make up each screen (the race loading
screen is the country's `Game/Welcome/*/Flag_*.BMP`; the main menu pans
`Backdrop/Main.bmp` across x 640–1791, y 0–720; `select/select.bmp` is a
plain red square, not part of the dial) and where every HUD piece goes.

## The pack in releases

The HD set is about 800 MB of BMPs; releases carry it compactly:

1. `cmr-hd pack <set> <pack>` stores each runtime BMP as a palette PNG
   (the HUD's 8-bit textures, exact) or a colour image: a lossless PNG,
   or a near-lossless WebP (quality 92, about 41 dB) when that is much
   smaller. Pack 3 is about 28 MB.
2. Zip `pack.txt`, `x2/` and `x4/` from the pack folder and publish the
   zip once as a release asset under its own tag (`hd-pack-<n>`, a
   pre-release). `packaging/hd-pack.txt` pins its URL and SHA-256; CI
   downloads and checks it and puts it in the zip and the AppImage as
   `hd/`, next to `CMRLauncher.exe`.
3. With `hd_art = on`, the launcher installs the pack into the runtime
   folder (`<runtime>/hd/<id>/x2`, `x4`; `crates/cmr-launcher/src/hdpack.rs`
   with the format in `crates/cmr-hdpack`), once per pack, and points the
   hook library at the set for the resolution.

## Real-ESRGAN

`cmr-hd upscale --esrgan` runs `realesrgan-ncnn-vulkan` (BSD-3-Clause,
used to make a pack, never shipped) on pictures (`realesrgan-x4plus`) and
interface pieces (`realesrgan-x4plus-anime`), then back-projects the
result onto the original (`src/lock.rs`). Pack 1 used it for 912 images.
It was dropped from pack 2 on: it invents textures (cracked leather on the
carbon weave), draws one-pixel staircases as wavy edges and outlines menu
lettering. It may still suit photographs. Its folder mode is broken
(results saved under other images' names, or noise, differently each
run), so `cmr-hd` packs images into sheets and runs it once per sheet
(`src/atlas.rs`).

## Next

- **The globe and its flag strip** are drawn by a path that isn't the
  blit; they need their own hook before they can have twins.
- **Fonts** (`*.fon`, `ini/cdFONT.BMP`, the race timer's digit sheets):
  a separate class; text could be redrawn from a vector font.
- **Car and country pictures, thumbnails**: `tiles.py` should suit them.
- **More of the scenery**: the rest of each stage's `*.obj` (buildings,
  trees, spectators, vehicles), its `*.sht` and the road and terrain
  textures in its `*.c3d` are whole bitmaps too, loaded the same way: a
  twin is all they need.
- Palette effects: a few front-end screens fade or cycle palettes; check
  that HD twins follow.
