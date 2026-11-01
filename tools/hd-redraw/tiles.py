"""Redraw a large picture with Nano Banana 2 in overlapping
tiles (the model keeps fine textures, numbers and lines when it sees them
large), each tile's tone and lighting taken from the original (only the
redraw's fine detail is kept), tiles feathered together.
usage: tiles.py <in.png> <out.png> <scale> [x0 y0 x1 y1]
Outside the box: the original enlarged (Lanczos). Key: GEMINI_API_KEY."""
import os, sys
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

T, O = 320, 64          # tile and overlap, original pixels
LOW = 1.5               # tone kept from the original below this (original px)
PROMPT = ("This is a crop of a menu background from a 1998 PC rally video game, a pre-rendered CG image, shown "
  "enlarged with visible pixels. Redraw it as the same image rendered natively at very high resolution: keep exactly "
  "the same composition, every line, edge, shape, number and letter in the same place and the same size, the same "
  "materials and textures at the same scale (a fine woven pattern stays the same fine weave, smooth areas stay smooth), "
  "the same colours and lighting. Only make it sharper and more detailed. Do not invent new patterns, objects or "
  "text; do not move, crop or extend anything.")


def starts(a, b):
    n = max(1, int(np.ceil((b - a - O) / (T - O))))
    return [a] if n == 1 else [round(a + i * (b - a - T) / (n - 1)) for i in range(n)]


def gemini(img):
    return redraw_api.redraw(img, "1:1", PROMPT)


def lock(redrawn, base, s):
    """The redraw's detail on the original's tone (both at s x)."""
    sig = LOW * s
    r = np.asarray(redrawn, np.float32); b = np.asarray(base, np.float32)
    low = lambda a: np.stack([ndimage.gaussian_filter(a[..., c], sig) for c in range(3)], -1)
    return np.clip(r - low(r) + low(b), 0, 255)


def drift(redrawn, orig):
    """How far the redraw strays from the original, at the original's size."""
    small = np.asarray(redrawn.resize(orig.size, Image.BOX), np.float32)
    return float(np.sqrt(((small - np.asarray(orig, np.float32)) ** 2).mean()))


def tile(orig, base, s, x, y):
    crop = orig.crop((x, y, x + T, y + T))
    shown = crop.resize((T * 4, T * 4), Image.NEAREST)
    best = None
    for _ in range(3):
        got = gemini(shown).resize((T * s, T * s), Image.LANCZOS)
        e = drift(got, crop)
        if best is None or e < best[1]: best = (got, e)
        if e < 12: break
    got, e = best
    b = base.crop((x * s, y * s, (x + T) * s, (y + T) * s))
    return x, y, lock(got, b, s), e


def main():
    src, out, s = sys.argv[1], sys.argv[2], int(sys.argv[3])
    orig = Image.open(src).convert("RGB")
    W, H = orig.size
    x0, y0, x1, y1 = map(int, sys.argv[4:8]) if len(sys.argv) >= 8 else (0, 0, W, H)
    x1, y1 = min(x1, W), min(y1, H)
    base = orig.resize((W * s, H * s), Image.LANCZOS)
    acc = np.asarray(base, np.float32).copy()
    jobs = [(x, y) for y in starts(y0, y1) for x in starts(x0, x1)]
    print(f"{len(jobs)} tiles", flush=True)
    # Feather: weight rises over the overlap from each tile's inner edges.
    ramp = np.minimum(1, (np.arange(T * s) + 0.5) / (O * s))
    edge = np.minimum(ramp, ramp[::-1])
    total = np.zeros((H * s, W * s), np.float32)
    summed = np.zeros_like(acc)
    with ThreadPoolExecutor(6) as pool:
        for x, y, px, e in pool.map(lambda j: tile(orig, base, s, *j), jobs):
            wy = edge.copy(); wx = edge.copy()
            if y == y0: wy[: O * s] = 1
            if y + T >= y1: wy[-O * s:] = 1
            if x == x0: wx[: O * s] = 1
            if x + T >= x1: wx[-O * s:] = 1
            w = np.outer(wy, wx)
            sl = (slice(y * s, (y + T) * s), slice(x * s, (x + T) * s))
            summed[sl] += px * w[..., None]; total[sl] += w
            print(f"tile {x},{y}: drift {e:.1f}", flush=True)
    has = total > 0
    acc[has] = summed[has] / total[has][..., None]
    Image.fromarray(acc.astype(np.uint8)).save(out)


if __name__ == "__main__":
    main()
