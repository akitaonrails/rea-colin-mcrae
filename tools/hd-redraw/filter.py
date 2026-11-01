"""Moves redraw.py results that drift too far, or whose transparent background
isn't, to <redrawn>/rejected. usage: filter.py <extracted> <redrawn>"""
import os, sys, shutil
import numpy as np
from PIL import Image
EXT, RED = sys.argv[1], sys.argv[2]
LIMIT = {"picture": 30, "art": 75}
kept = rej = 0
for line in open(os.path.join(RED, "log.tsv")):
    rel, err, kind = line.rstrip("\n").split("\t")[:3]
    if err == "FAILED": continue
    path = os.path.join(RED, rel + ".png")
    if not os.path.exists(path): continue
    why = None
    if float(err) > LIMIT.get(kind, 60): why = f"drift {err}"
    red = Image.open(path)
    if red.mode == "RGBA":
        o = np.asarray(Image.open(os.path.join(EXT, rel + ".png")).convert("RGBA").resize(red.size, Image.NEAREST))[..., 3] > 0
        r = np.asarray(red)[..., 3] > 0
        if (~o).sum() and (r & ~o).sum() / (~o).sum() > 0.15: why = why or "background not transparent"
        if o.sum() and (~r & o).sum() / o.sum() > 0.55: why = why or "art missing"
    if why:
        dst = os.path.join(RED, "rejected", rel + ".png"); os.makedirs(os.path.dirname(dst), exist_ok=True)
        shutil.move(path, dst); rej += 1; print("rejected", rel, why)
    else:
        kept += 1
print(kept, "kept,", rej, "rejected")
