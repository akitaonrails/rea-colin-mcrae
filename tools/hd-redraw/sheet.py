"""Redraw a family of small sprites together (the HUD's car icons) with Nano
Banana 2 at 4x. They are laid out on one black sheet and redrawn in one
request, so they share one style (one at a time, the model drifts and
invents details), then cut back out. `cmr-hd import` then holds each to
its original (colours, transparency).
usage: sheet.py <extracted> <out> <kind> <path regex>
kind: cars. Writes <out>/<path>.png (alpha: black is transparent) and
<out>/log.tsv. Key: GEMINI_API_KEY, never printed."""
import os, re, sys
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

S = 4
GAP = 6  # original pixels between sprites
PROMPTS = {
 "cars": "These are small side-view icons of 1998 rally cars from a video game's race display, on a black "
  "background, shown enlarged with visible pixels. Redraw each one from scratch as a crisp, clean high-resolution "
  "illustration: the same car model, livery, colours, stripes and sponsor shapes, the same proportions, facing the "
  "same way, in exactly the same place and size. Smooth outlines and shading, no pixel staircases, no blur. Do not "
  "add numbers, text or details that are not there. Keep the background pure flat black (#000000).",
}


def layout(imgs, width):
    """Shelf layout: [(x, y)] in original pixels, and the sheet's size."""
    places, x, y, row = [], GAP, GAP, 0
    for im in imgs:
        if x + im.width + GAP > width:
            x, y, row = GAP, y + row + GAP, 0
        places.append((x, y)); x += im.width + GAP; row = max(row, im.height)
    return places, (width, y + row + GAP)


def keyed_alpha(img):
    a = np.asarray(img.convert("RGB"), dtype=np.int32)
    alpha = np.where(a.max(axis=2) < 24, 0, 255).astype(np.uint8)
    out = img.convert("RGBA"); out.putalpha(Image.fromarray(alpha)); return out


def main():
    ext, out, kind, pattern = sys.argv[1:5]
    seen, rels = set(), []
    for line in open(os.path.join(ext, "manifest.tsv")):
        if line.startswith("#"): continue
        f = line.rstrip("\n").split("\t")
        if re.search(pattern, f[0]) and f[5] not in seen:
            seen.add(f[5]); rels.append(f[0])
    imgs = [Image.open(os.path.join(ext, r + ".png")).convert("RGBA") for r in rels]
    width = max(max(i.width for i in imgs) + 2 * GAP, int((sum(i.width * i.height for i in imgs) * 2) ** 0.5))
    places, size = layout(imgs, width)
    sheet = Image.new("RGB", size)
    for im, (x, y) in zip(imgs, places):
        sheet.paste(im, (x, y), im)
    print(f"{len(rels)} sprites on a {size[0]}x{size[1]} sheet", flush=True)
    pad, ratio, (ox, oy) = redraw_api.fit_ratio(sheet, (0, 0, 0))
    pw, ph = pad.size
    k = max(2, 1536 // max(pw, ph))
    best = None
    for _ in range(3):
        got = redraw_api.redraw(pad.resize((pw * k, ph * k), Image.NEAREST), ratio, PROMPTS[kind])
        got = got.resize((pw * S, ph * S), Image.LANCZOS).crop((ox * S, oy * S, (ox + size[0]) * S, (oy + size[1]) * S))
        small = np.asarray(got.resize(size, Image.BOX), np.float32)
        e = float(np.sqrt(((small - np.asarray(sheet, np.float32)) ** 2).mean()))
        if best is None or e < best[1]: best = (got, e)
    got, e = best
    os.makedirs(out, exist_ok=True)
    got.save(os.path.join(out, f"sheet-{kind}.png"))
    with open(os.path.join(out, "log.tsv"), "a") as log:
        for r, im, (x, y) in zip(rels, imgs, places):
            piece = keyed_alpha(got.crop((x * S, y * S, (x + im.width) * S, (y + im.height) * S)))
            dst = os.path.join(out, r + ".png"); os.makedirs(os.path.dirname(dst), exist_ok=True)
            piece.save(dst); log.write(f"{r}\t{e:.1f}\t{kind}\n")
    print(f"drift {e:.1f}", flush=True)


if __name__ == "__main__":
    main()
