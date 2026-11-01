"""Picks, for every image, the first candidate redraw whose writing is real
and makes sense (`redraw_api.writing_ok`), from candidate folders in order
(each made by redraw.py, sheet.py...). Images with no candidate that
passes are left out (the original stays). Shown enlarged to the checker so
small writing is readable.
usage: pick.py <out> <candidate folder> [candidate folder ...]
Writes <out>/<path>.png and <out>/pick.tsv. Key: GEMINI_API_KEY."""
import os, shutil, sys
from concurrent.futures import ThreadPoolExecutor
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api


def main():
    out, cands = sys.argv[1], sys.argv[2:]
    rels = sorted({os.path.relpath(os.path.join(r, f), c)[:-4] for c in cands for r, _, fs in os.walk(c)
                   for f in fs if f.lower().endswith(".bmp.png") and "/rejected/" not in os.path.join(r, f)})

    def one(rel):
        for c in cands:
            path = os.path.join(c, rel + ".png")
            if not os.path.exists(path): continue
            im = Image.open(path).convert("RGBA")
            shown = Image.new("RGB", im.size, (128, 128, 128)); shown.paste(im, mask=im.split()[3])
            k = max(1, 1024 // max(shown.size))
            if redraw_api.writing_ok(shown.resize((shown.width * k, shown.height * k), Image.LANCZOS)):
                dst = os.path.join(out, rel + ".png"); os.makedirs(os.path.dirname(dst), exist_ok=True)
                shutil.copy(path, dst)
                return rel, c
        return rel, None

    os.makedirs(out, exist_ok=True)
    with ThreadPoolExecutor(8) as pool, open(os.path.join(out, "pick.tsv"), "a") as log:
        for rel, c in pool.map(one, rels):
            log.write(f"{rel}\t{c or 'none: the original stays'}\n"); print(rel, c or "NONE", flush=True)


if __name__ == "__main__":
    main()
