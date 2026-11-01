"""Redraw the stages' sky and distant scenery (the bitmaps inside each
`*.hzn`, class `sky` in `cmr-hd extract`) with Nano Banana 2 at 4x.

A horizon holds bands (the sky over distant hills, stored on their side)
that join into one panorama, and a ground texture that tiles. The bands are
turned upright, joined in the order their edges match and redrawn in one
request, so the joins between them stay continuous; the ground is redrawn
alone. Everything keeps the original's tone and lighting (detail finer
than LOW original pixels comes from the redraw) and fades back to the
original within EDGE pixels of its borders, where the game joins or repeats
the textures. Writes <out>/<path>.png (RGB) and <out>/log.tsv.
usage: sky.py <extracted> <out> [path prefix ...]
Key: GEMINI_API_KEY, never printed."""
import os, sys, threading
from collections import defaultdict
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image, ImageFilter
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

S = 4            # output scale
LOW = 2.0        # tone and lighting kept from the original below this (original px)
EDGE = 4         # fade back to the original within this many original px of a border
FLAT = 0.5       # an original with less fine detail than this is one colour: kept as it is
DRIFT = 50       # a redraw that strays further from the original is dropped
PROMPTS = {
 "band": "This is a panorama of the sky and the distant landscape on the horizon from a 1998 rally video game, an "
  "old low-resolution, dithered 8-bit photograph shown enlarged. Redraw it as the same photograph taken with a modern "
  "high-resolution camera: the same sky, the same clouds in the same places and shapes, the same distant hills, "
  "mountains, trees and fields in exactly the same positions and sizes, the same colours, haze and lighting. It is far "
  "away, so keep it soft and slightly hazy, not sharp; just remove the dithering, colour banding and blockiness and add "
  "natural fine detail. Do not add, remove or move anything; no new objects, no text, no borders, no frame.",
 "ground": "This is a small seamless ground texture (grass, dirt, gravel or snow seen from far away) from a 1998 rally "
  "video game, an old low-resolution dithered 8-bit image shown enlarged. Redraw it as the same texture photographed "
  "at high resolution: the same colours, pattern, scale and patches in the same places, soft and natural, without "
  "dithering or blockiness. Do not add objects, borders or text.",
}


def blueness(a):
    return float((a[..., 2] - a[..., 0]).mean())


def upright(img):
    """The image turned so its sky (the bluest edge) is at the top, and
    the rotation that undoes it."""
    a = np.asarray(img, np.float32)
    k = max(4, min(a.shape[:2]) // 8)
    edges = {"top": a[:k], "bottom": a[-k:], "left": a[:, :k], "right": a[:, -k:]}
    side = max(edges, key=lambda e: blueness(edges[e]))
    turn = {"top": None, "left": Image.Transpose.ROTATE_270, "right": Image.Transpose.ROTATE_90,
            "bottom": Image.Transpose.ROTATE_180}[side]
    back = {None: None, Image.Transpose.ROTATE_270: Image.Transpose.ROTATE_90,
            Image.Transpose.ROTATE_90: Image.Transpose.ROTATE_270,
            Image.Transpose.ROTATE_180: Image.Transpose.ROTATE_180}[turn]
    return (img.transpose(turn) if turn else img), back


def chain(bands):
    """The order the bands join in (each one's right edge against the next
    one's left), starting from the band whose left edge matches least."""
    a = [np.asarray(b, np.float32) for b in bands]
    cost = lambda i, j: float(np.abs(a[i][:, -1] - a[j][:, 0]).mean())
    n = len(a)
    if n == 1:
        return [0]
    start = max(range(n), key=lambda j: min(cost(i, j) for i in range(n) if i != j))
    order, left = [start], set(range(n)) - {start}
    while left:
        nxt = min(left, key=lambda j: cost(order[-1], j)); order.append(nxt); left.remove(nxt)
    return order


def redraw(img, kind):
    """img redrawn at S x (same framing), best of two by drift."""
    soft = img.filter(ImageFilter.GaussianBlur(0.6))  # the dithering, not the shapes
    pad, ratio, (ox, oy) = redraw_api.fit_ratio(soft, (0, 0, 0))
    pw, ph = pad.size
    k = max(1, 1536 // max(pw, ph))
    best = None
    for _ in range(2):
        got = redraw_api.redraw(pad.resize((pw * k, ph * k), Image.LANCZOS), ratio, PROMPTS[kind])
        got = got.resize((pw * S, ph * S), Image.LANCZOS).crop((ox * S, oy * S, (ox + img.width) * S, (oy + img.height) * S))
        small = np.asarray(got.resize(img.size, Image.BOX), np.float32)
        e = float(np.sqrt(((small - np.asarray(img, np.float32)) ** 2).mean()))
        if best is None or e < best[1]: best = (got, e)
    return best


def finish(got, img):
    """The redraw's fine detail on the original's tone, faded back to the
    original at the borders."""
    base = np.asarray(img.resize((img.width * S, img.height * S), Image.LANCZOS), np.float32)
    r = np.asarray(got, np.float32)
    low = lambda a: np.stack([ndimage.gaussian_filter(a[..., c], LOW * S) for c in range(3)], -1)
    out = r - low(r) + low(base)
    h, w = out.shape[:2]
    d = np.minimum.outer(np.minimum(np.arange(h), np.arange(h)[::-1]), np.minimum(np.arange(w), np.arange(w)[::-1]))
    t = np.clip((d + 0.5) / (EDGE * S), 0, 1)[..., None]
    return Image.fromarray(np.clip(out * t + base * (1 - t), 0, 255).astype(np.uint8))


def worth_it(img, drift):
    """Whether a redraw goes in: the original has detail to redraw (on a
    flat texture the model only invents speckles and waves) and the redraw
    stays close to it (the model smeared one stage's sky)."""
    a = np.asarray(img.convert("L"), np.float32)
    return float(np.abs(a - ndimage.gaussian_filter(a, 2)).mean()) >= FLAT and drift <= DRIFT


def horizon(ext, rels):
    """Every bitmap of one horizon file, redrawn: {path: (image, drift)}."""
    imgs = {r: Image.open(os.path.join(ext, r + ".png")).convert("RGB") for r in rels}
    big = max(i.width * i.height for i in imgs.values())
    bands = [r for r in rels if imgs[r].width * imgs[r].height == big]
    out = {}
    for r in rels:
        if r not in bands:
            got, e = redraw(imgs[r], "ground"); out[r] = (finish(got, imgs[r]), e)
    turned = {r: upright(imgs[r]) for r in bands}
    ups = [turned[r][0] for r in bands]
    if len({u.size for u in ups}) == 1:
        order = [bands[i] for i in chain(ups)]
        w, h = ups[0].size
        pano = Image.new("RGB", (w * len(order), h))
        for n, r in enumerate(order): pano.paste(turned[r][0], (n * w, 0))
        got, e = redraw(pano, "band")
        done = finish(got, pano)
        for n, r in enumerate(order):
            piece = done.crop((n * w * S, 0, (n + 1) * w * S, h * S))
            back = turned[r][1]
            out[r] = (piece.transpose(back) if back else piece, e)
    else:  # bands of different shapes: one at a time
        for r in bands:
            up, back = turned[r]
            got, e = redraw(up, "band"); piece = finish(got, up)
            out[r] = (piece.transpose(back) if back else piece, e)
    return out


def main():
    ext, out = sys.argv[1], sys.argv[2]
    prefixes = tuple(sys.argv[3:]) or ("",)
    groups, seen = defaultdict(list), set()
    for line in open(os.path.join(ext, "manifest.tsv")):
        if line.startswith("#"): continue
        rel, cls = line.rstrip("\n").split("\t")[:2]
        if cls != "sky" or not rel.startswith(prefixes): continue
        groups[rel.rsplit("/", 1)[0]].append(rel)
    # Horizons with the very same bitmaps (stages that share a sky) once.
    jobs = []
    for file, rels in sorted(groups.items()):
        key = tuple(sorted(open(os.path.join(ext, r + ".png"), "rb").read() for r in rels))
        if key in seen: continue
        seen.add(key); jobs.append(sorted(rels))
    os.makedirs(out, exist_ok=True)
    log, lock = open(os.path.join(out, "log.tsv"), "a"), threading.Lock()
    print(len(jobs), "horizons", flush=True)

    def job(rels):
        if any(os.path.exists(os.path.join(out, r + ".png")) for r in rels): return
        try:
            for r, (img, e) in horizon(ext, rels).items():
                original = Image.open(os.path.join(ext, r + ".png")).convert("RGB")
                if not worth_it(original, e):
                    with lock: log.write(f"{r}\t{e:.1f}\tkept the original\n"); log.flush(); print(r, "kept the original", flush=True)
                    continue
                dst = os.path.join(out, r + ".png"); os.makedirs(os.path.dirname(dst), exist_ok=True)
                img.save(dst)
                with lock: log.write(f"{r}\t{e:.1f}\n"); log.flush(); print(r, f"{e:.1f}", flush=True)
        except Exception as ex:
            with lock: log.write(f"{rels[0]}\tFAILED\t{str(ex)[:200]}\n"); log.flush(); print(rels[0], "FAILED", ex, flush=True)

    with ThreadPoolExecutor(6) as pool:
        list(pool.map(job, jobs))


if __name__ == "__main__":
    main()
