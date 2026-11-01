"""Aligns each redraw (redraw.py output) to its original: the scale and shift per axis that
best match the original's shapes, applied at full size. Writes into
<out>, keeping alpha. usage: align.py <extracted> <redrawn> <out>"""
import os, sys
import numpy as np
from PIL import Image
from scipy import ndimage, optimize

EXT, RED, OUT = sys.argv[1:4]

def lum(img):
    a = np.asarray(img.convert("RGBA"), dtype=np.float64)
    rgb = a[..., :3] * (a[..., 3:] / 255.0)
    return rgb.mean(axis=2)

def warp(img, p, order=1):
    sx, sy, dx, dy = p
    # output (y, x) samples input at (y*sy + dy, x*sx + dx)
    return ndimage.affine_transform(img, [sy, sx], offset=[dy, dx], order=order, mode="nearest")

def fit(orig, small):
    o = ndimage.gaussian_filter(orig, 1.0)
    s = ndimage.gaussian_filter(small, 1.0)
    cost = lambda p: ((warp(s, p) - o) ** 2).mean()
    best = min(((cost([1, 1, dx, dy]), [1, 1, dx, dy]) for dx in (-2, -1, 0, 1, 2) for dy in (-2, -1, 0, 1, 2)), key=lambda t: t[0])
    r = optimize.minimize(cost, best[1], method="Powell", options={"xtol": 1e-3, "ftol": 1e-4, "maxiter": 3000})
    return r.x, cost([1, 1, 0, 0]), r.fun

for root, _, files in os.walk(RED):
    if "rejected" in root:
        continue
    for f in files:
        if not f.endswith(".png"):
            continue
        rel = os.path.relpath(os.path.join(root, f), RED)
        orig_img = Image.open(os.path.join(EXT, rel)).convert("RGBA")
        red = Image.open(os.path.join(root, f))
        w, h = orig_img.size
        k = red.width / w
        small = red.resize((w, h), Image.BOX)
        p, before, after = fit(lum(orig_img), lum(small))
        sx, sy, dx, dy = p
        # The same map at full size: output X samples input X*sx + dx*k (+ centre terms).
        big = np.asarray(red.convert("RGBA"), dtype=np.float64)
        ch = [warp(big[..., c], [sx, sy, dx * k + (sx - 1) * (k - 1) / 2, dy * k + (sy - 1) * (k - 1) / 2]) for c in range(4)]
        outimg = Image.fromarray(np.clip(np.stack(ch, -1), 0, 255).astype(np.uint8), "RGBA")
        if red.mode != "RGBA":
            outimg = outimg.convert("RGB")
        dst = os.path.join(OUT, rel); os.makedirs(os.path.dirname(dst), exist_ok=True)
        outimg.save(dst)
        print(f"{rel}\tscale {sx:.3f} {sy:.3f} shift {dx:+.2f} {dy:+.2f}\terror {before:.0f} -> {after:.0f}", flush=True)
