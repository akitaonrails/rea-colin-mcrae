"""Redraw a main-menu ring frame (New_Fe/Images/MainOpts/<language>/Anim:
words printed around a ring, tops towards the centre, reading
counter-clockwise) at 4x. The ring is unwrapped (polar) so every word is
straight, upright text; the frame's words are stacked on one green sheet
and redrawn together with Nano Banana 2 (one word at a time, the model
loses the typeface), unmixed from the green, snapped back onto each
word's own ink and wrapped back onto the ring. Dots are drawn as discs.
usage: ring.py <frame.png> <out.png> [--dry <strip.png>]
--dry only shows the unwrapped strip and the words found (no API call).
Key: GEMINI_API_KEY, never printed. RING_DEBUG=<prefix> saves the sheet
and every try."""
import os, sys
import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

CX, CY = 202.0, 201.5   # the ring's centre (fitted on the seven English frames)
R_IN, R_OUT = 164.0, 206.0
S = 4                   # output scale
DOT = 30                # narrower components are dots
N = int(2 * np.pi * (R_IN + R_OUT) / 2)  # strip length at 1x


def unwrap(img, t0):
    """The ring band as a strip at S x: x runs counter-clockwise from angle
    t0, y outwards; nearest samples of the original (a smooth unwrap made
    the model draw soft, ghosted letters)."""
    a = np.asarray(img)
    w, h = N * S, int((R_OUT - R_IN) * S)
    u, v = np.meshgrid((np.arange(w) + 0.5) / S, (np.arange(h) + 0.5) / S)
    t = t0 + u / N * 2 * np.pi
    r = R_IN + v
    x = CX + r * np.sin(t)
    y = CY + r * np.cos(t)
    xi = np.clip(np.floor(x).astype(int), 0, img.width - 1)
    yi = np.clip(np.floor(y).astype(int), 0, img.height - 1)
    return Image.fromarray(a[yi, xi])


def wrap(strip, t0, size):
    """The strip (S x) back onto the ring at S x, bilinear."""
    a = np.asarray(strip, dtype=np.float32)
    if a.ndim == 3 and a.shape[2] == 4 and not isinstance(strip, np.ndarray):
        a = a.copy(); a[..., :3] *= a[..., 3:4] / 255  # premultiply
    W, H = size[0] * S, size[1] * S
    X, Y = np.meshgrid((np.arange(W) + 0.5) / S, (np.arange(H) + 0.5) / S)
    dx, dy = X - CX, Y - CY
    t = (np.arctan2(dx, dy) - t0) % (2 * np.pi)
    u = t / (2 * np.pi) * N * S - 0.5
    v = (np.hypot(dx, dy) - R_IN) * S - 0.5
    out = np.zeros((H, W, a.shape[2]), np.float32)
    for c in range(a.shape[2]):
        out[..., c] = ndimage.map_coordinates(a[..., c], [v, u], order=1, mode="constant", cval=0)
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8))


def seam(img):
    """The angle with the least ink: the strip starts there."""
    a = np.asarray(img)[..., 3] > 0
    ys, xs = np.nonzero(a)
    t = np.arctan2(xs + 0.5 - CX, ys + 0.5 - CY) % (2 * np.pi)
    hist, edges = np.histogram(t, bins=360, range=(0, 2 * np.pi))
    run = np.convolve(np.r_[hist, hist[:20]], np.ones(20), "valid")[:360]
    return edges[int(np.argmin(run)) + 10]


def words(strip):
    """Word boxes in the strip (letters joined along the line; dots apart)."""
    a = np.asarray(strip)[..., 3] > 0
    joined = ndimage.binary_dilation(a, structure=np.ones((S * 4, S * 3 + 1)))
    labels, _ = ndimage.label(joined)
    out = []
    for i, sl in enumerate(ndimage.find_objects(labels), 1):
        if sl[1].stop - sl[1].start < DOT * S:  # a dot
            continue
        out.append((sl, labels == i))
    return out


GREEN = np.array([0, 255, 0], np.float32)
PROMPT = ("These are the words of a menu in a 1998 PC rally video game, one per row, shown enlarged with jagged pixels. "
  "Redraw them from scratch as crisp high-resolution vector lettering, as if typeset and drawn in Adobe Illustrator: perfectly "
  "smooth straight strokes, clean round curves, no pixel staircases, no blur, no noise. Keep exactly the same letters and "
  "spelling, the same bold condensed typeface, the same sizes, positions and slant, white letters with the same dark grey drop "
  "shadow offset the same way, and the same small dots. Do not add, remove or move anything. The background must stay pure "
  "flat green (#00FF00).")


def on_green(rgba):
    bg = Image.new("RGBA", rgba.size, (0, 255, 0, 255)); bg.alpha_composite(rgba); return bg.convert("RGB")


def excess(a):
    return a[..., 1] - (a[..., 0] + a[..., 2]) / 2


def unmix(rgb, free):
    """Neutral lettering over the redraw's own green: alpha and colour.
    The green (never quite the asked-for one, and uneven) is estimated
    from the `free` pixels, far from any ink, smoothed over the sheet."""
    a = np.asarray(rgb, np.float32)
    w = ndimage.gaussian_filter(free.astype(np.float32), 24)
    bg = np.stack([ndimage.gaussian_filter(a[..., c] * free, 24) for c in range(3)], -1) / np.maximum(w, 1e-6)[..., None]
    alpha = np.clip(1 - excess(a) / np.maximum(excess(bg), 1), 0, 1)
    colour = np.clip((a - (1 - alpha[..., None]) * bg) / np.maximum(alpha[..., None], 1e-3), 0, 255)
    return colour, alpha


def redraw_sheet(sheet):
    """The sheet redrawn at the same size."""
    pad, ratio, (ox, oy) = redraw_api.fit_ratio(sheet, (0, 255, 0))
    pw, ph = pad.size
    # Shown at least 2x (nearest): the model then keeps the letterforms.
    k = max(2, 1536 // max(pw, ph))
    got = redraw_api.redraw(pad.resize((pw * k, ph * k), Image.NEAREST), ratio, PROMPT)
    return got.resize((pw, ph), Image.LANCZOS).crop((ox, oy, ox + sheet.width, oy + sheet.height))


def shape_err(alpha, want):
    """How far the redraw's shape is from the original's, at 1x."""
    k = lambda m: m.reshape(m.shape[0] // S, S, m.shape[1] // S, S).mean(axis=(1, 3))
    h, w = (alpha.shape[0] // S) * S, (alpha.shape[1] // S) * S
    return float(np.sqrt(((k(alpha[:h, :w]) - k(want[:h, :w])) ** 2).mean()))


GAP = 8 * S


def make_sheet(strip, found, columns=2):
    """The frame's words stacked on green (upright, as unwrapped) in
    columns, and where each sits: (strip box, sheet x, sheet y)."""
    a = np.asarray(strip)
    boxes, own = [], []
    for sl, label in found:
        y0, y1 = max(0, sl[0].start - GAP), min(a.shape[0], sl[0].stop + GAP)
        x0, x1 = max(0, sl[1].start - GAP), min(a.shape[1], sl[1].stop + GAP)
        boxes.append((y0, y1, x0, x1))
        piece = np.asarray(strip.crop((x0, y0, x1, y1))).copy()
        piece[..., 3] *= ndimage.binary_dilation(label[y0:y1, x0:x1], iterations=S).astype(np.uint8)
        own.append(Image.fromarray(piece))
    rows = -(-len(boxes) // columns)
    cols = [boxes[c * rows:(c + 1) * rows] for c in range(columns)]
    cw = [max((x1 - x0 for _, _, x0, x1 in col), default=0) for col in cols]
    H = max(sum(y1 - y0 for y0, y1, _, _ in col) for col in cols)
    sheet = Image.new("RGBA", (sum(cw) + GAP * (columns - 1), H), (0, 0, 0, 0))
    places, cx, n = [], 0, 0
    for col, w in zip(cols, cw):
        y = 0
        for b in col:
            y0, y1, x0, x1 = b
            x = cx + (w - (x1 - x0)) // 2
            sheet.alpha_composite(own[n], (x, y)); n += 1
            places.append((b, x, y)); y += y1 - y0
        cx += w + GAP
    return on_green(sheet), places


def moments(alpha):
    tot = alpha.sum()
    ys, xs = np.mgrid[0:alpha.shape[0], 0:alpha.shape[1]]
    cy, cx = (alpha * ys).sum() / tot, (alpha * xs).sum() / tot
    sy = np.sqrt((alpha * (ys - cy) ** 2).sum() / tot); sx = np.sqrt((alpha * (xs - cx) ** 2).sum() / tot)
    return cy, cx, sy, sx


def redraw_strip(strip, found):
    """The frame's words redrawn in one go (best of three by likeness),
    each snapped back onto its own ink; dots as discs. An RGBA strip,
    premultiplied."""
    a = np.asarray(strip)
    sheet, places = make_sheet(strip, found)
    ink = np.asarray(sheet, np.float32)
    free = ~ndimage.binary_dilation(excess(ink) < 200, iterations=6 * S)
    want = np.clip(1 - excess(ink) / 255, 0, 1)
    best = None
    for _ in range(3):
        got = redraw_sheet(sheet); e = shape_err(unmix(got, free)[1], want)
        if os.environ.get("RING_DEBUG"): got.save(os.environ["RING_DEBUG"] + f"-try{e:.3f}.png")
        if best is None or e < best[1]: best = (got, e)
    got, e = best
    if os.environ.get("RING_DEBUG"):
        sheet.save(os.environ["RING_DEBUG"] + "-in.png"); got.save(os.environ["RING_DEBUG"] + "-out.png")
    colour, alpha = unmix(got, free)
    canvas = np.zeros(a.shape, np.float32)
    for (sl, label), ((y0, y1, x0, x1), sx0, sy0) in zip(found, places):
        h, w = y1 - y0, x1 - x0
        al = alpha[sy0:sy0 + h, sx0:sx0 + w]
        co = colour[sy0:sy0 + h, sx0:sx0 + w]
        keep = ndimage.binary_dilation(label[y0:y1, x0:x1], iterations=S * 2)
        want = (a[y0:y1, x0:x1, 3] > 0) * keep * 1.0
        al = al * ndimage.binary_dilation(want > 0, iterations=S * 3)
        # Snap: map the redraw's ink onto the original's (centre and spread).
        hy, hx, hsy, hsx = moments(al); wy, wx, wsy, wsx = moments(want)
        ky, kx = hsy / wsy, hsx / wsx
        offset = (hy - wy * ky, hx - wx * kx)
        src = np.dstack([co * al[..., None], al * 255])
        out = np.stack([ndimage.affine_transform(src[..., c], [ky, kx], offset=offset, order=1) for c in range(4)], -1)
        out = out * keep[..., None]
        c = canvas[y0:y1, x0:x1]
        o = out[..., 3:4] / 255
        c[..., :3] = c[..., :3] * (1 - o) + out[..., :3]
        c[..., 3:4] = np.maximum(c[..., 3:4], out[..., 3:4])
    draw_dots(strip, canvas)
    return canvas, f"{e:.3f}"


def draw_dots(strip, canvas):
    """The dots between words: their dark and light parts as soft discs."""
    a = np.asarray(strip)
    ink = a[..., 3] > 0
    labels, _ = ndimage.label(ndimage.binary_dilation(ink, structure=np.ones((S * 4, S * 3 + 1))))
    yy, xx = np.mgrid[0:a.shape[0], 0:a.shape[1]]
    for i, sl in enumerate(ndimage.find_objects(labels), 1):
        part = (labels[sl] == i) & ink[sl]
        if sl[1].stop - sl[1].start >= DOT * S:
            continue
        lum = a[sl][..., :3].mean(axis=2)
        for sel in (part & (lum <= 128), part & (lum > 128)):
            if sel.sum() == 0: continue
            cy, cx = ndimage.center_of_mass(sel)
            r = (sel.sum() / np.pi) ** 0.5
            colour = np.median(a[sl][sel][:, :3], axis=0)
            Y, X = yy[sl], xx[sl]
            cov = np.clip(r - np.hypot(X - (sl[1].start + cx), Y - (sl[0].start + cy)) + 0.5, 0, 1)
            c = canvas[sl]
            c[..., :3] = c[..., :3] * (1 - cov[..., None]) + colour * cov[..., None]  # premultiplied
            c[..., 3] = np.maximum(c[..., 3], cov * 255)


def unpremultiply(img):
    """Straight colour where the (wrapped, premultiplied) strip has alpha."""
    a = np.asarray(img, np.float32)
    al = a[..., 3:4] / 255
    rgb = np.where(al > 0, a[..., :3] / np.maximum(al, 1e-6), 0)
    return Image.fromarray(np.clip(np.concatenate([rgb, a[..., 3:4]], -1), 0, 255).astype(np.uint8))


def main():
    src, out = sys.argv[1], sys.argv[2]
    img = Image.open(src).convert("RGBA")
    t0 = seam(img)
    strip = unwrap(img, t0)
    found = words(strip)
    print(f"seam {np.degrees(t0):.0f} deg, {len(found)} words")
    if "--dry" in sys.argv:
        show = Image.new("RGBA", strip.size, (0, 0, 80, 255)); show.alpha_composite(strip)
        d = ImageDraw.Draw(show)
        for sl, _ in found:
            d.rectangle([sl[1].start, sl[0].start, sl[1].stop - 1, sl[0].stop - 1], outline=(255, 0, 0, 255))
        show.save(sys.argv[sys.argv.index("--dry") + 1])
        back = wrap(strip, t0, img.size)
        bg = Image.new("RGBA", back.size, (0, 0, 80, 255)); bg.alpha_composite(back.convert("RGBA"))
        bg.save(out)
        return
    canvas, notes = redraw_strip(strip, found)
    ring = unpremultiply(wrap(canvas, t0, img.size))
    ring.save(out)
    print(os.path.basename(src), notes, flush=True)


if __name__ == "__main__":
    main()
