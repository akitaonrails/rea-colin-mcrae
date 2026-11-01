"""Picks, for every image, the first candidate redraw whose writing is real
and makes sense (`redraw_api.writing_ok`), from candidate folders in order
(each made by redraw.py, sheet.py...). Images with no candidate that
passes are left out (the original stays). Shown enlarged to the checker so
small writing is readable.
usage: pick.py [--texts <signs.tsv> | --against <extracted>] <out> <candidate folder> [...]
--texts: the transcriptions signs.py made; each sign must read exactly that.
--against: compare each candidate's writing with the original's (car
textures, whose writing is partly mirrored on purpose).
Writes <out>/<path>.png and <out>/pick.tsv. Key: GEMINI_API_KEY."""
import os, shutil, sys
from concurrent.futures import ThreadPoolExecutor
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api


def main():
    args = sys.argv[1:]
    texts, against = {}, None
    if args[0] == "--against":
        against, args = args[1], args[2:]
    elif args[0] == "--texts":
        for l in open(args[1]):
            f = l.rstrip("\n").split("\t")
            if len(f) > 2: texts[f[0]] = f[2]
        args = args[2:]
    out, cands = args[0], args[1:]
    rels = sorted({os.path.relpath(os.path.join(r, f), c)[:-4] for c in cands for r, _, fs in os.walk(c)
                   for f in fs if f.lower().endswith(".bmp.png") and "/rejected/" not in os.path.join(r, f)})

    def one(rel):
        for c in cands:
            path = os.path.join(c, rel + ".png")
            if not os.path.exists(path): continue
            im = Image.open(path).convert("RGBA")
            shown = Image.new("RGB", im.size, (128, 128, 128)); shown.paste(im, mask=im.split()[3])
            k = max(1, 1024 // max(shown.size))
            if against:
                orig = Image.open(os.path.join(against, rel + ".png")).convert("RGBA")
                base = Image.new("RGB", orig.size, (128, 128, 128)); base.paste(orig, mask=orig.split()[3])
                size = (shown.width * k, shown.height * k)
                ok = redraw_api.writing_matches(base.resize(size, Image.NEAREST), shown.resize(size, Image.LANCZOS))
            else:
                ok = redraw_api.writing_ok(shown.resize((shown.width * k, shown.height * k), Image.LANCZOS), texts.get(rel))
            if ok:
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
