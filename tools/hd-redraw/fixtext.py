"""Where a redraw's writing is wrong, put the original back. For candidates
that `pick.py --against` rejected (a car livery whose drivers' signatures or
tiny stickers became made-up words): a vision model marks the boxes where
the redraw's writing differs from the original's; there the redraw gives
way to the original enlarged smoothly (feathered), so the rest stays
redrawn. The result is checked again and kept only if it passes.
usage: fixtext.py <extracted> <out> <candidate folder> [...]
Writes <out>/<path>.png and <out>/fix.tsv. Key: GEMINI_API_KEY."""
import json, os, re, sys
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

BOXES = ("Left: an original low-resolution texture from a 1998 rally video game (writing may be mirrored on purpose). "
  "Right: a high-resolution redraw of it, the same layout. Find every place where the right image's writing is wrong: "
  "garbled, misspelled, made-up, or different from the left. Reply with only a JSON list of boxes on the RIGHT image, "
  "each [ymin, xmin, ymax, xmax] scaled 0-1000 to that image; [] if there are none.")


def boxes(original, redraw):
    h = max(original.height, redraw.height)
    pair = Image.new("RGB", (original.width + redraw.width + 16, h), (128, 128, 128))
    pair.paste(original, (0, 0)); pair.paste(redraw, (original.width + 16, 0))
    text = redraw_api.ask(pair, BOXES)
    m = re.search(r"\[.*\]", text, re.S)
    try:
        found = json.loads(m.group(0)) if m else []
    except ValueError:
        return None
    return [b for b in found if isinstance(b, list) and len(b) == 4]


def one(ext, cands, out, rel):
    orig = Image.open(os.path.join(ext, rel + ".png")).convert("RGBA")
    for c in cands:
        path = os.path.join(c, rel + ".png")
        if not os.path.exists(path): continue
        red = Image.open(path).convert("RGBA")
        W, H = red.size
        base = orig.resize((W, H), Image.LANCZOS)
        k = max(1, 1024 // max(W, H))
        o = Image.new("RGB", orig.size, (128, 128, 128)); o.paste(orig, mask=orig.split()[3])
        r = Image.new("RGB", red.size, (128, 128, 128)); r.paste(red, mask=red.split()[3])
        found = boxes(o.resize((W * k, H * k), Image.NEAREST), r.resize((W * k, H * k), Image.LANCZOS))
        if found is None: continue
        mask = np.zeros((H, W), np.float32)
        pad = W // 64
        for y0, x0, y1, x1 in found:
            mask[max(0, int(y0 * H / 1000) - pad):int(y1 * H / 1000) + pad,
                 max(0, int(x0 * W / 1000) - pad):int(x1 * W / 1000) + pad] = 1
        if mask.mean() > 0.35: continue  # most of it: not worth keeping
        mask = np.clip(ndimage.gaussian_filter(mask, W / 256) * 1.5, 0, 1)[..., None]
        fixed = Image.fromarray((np.asarray(red, np.float32) * (1 - mask) + np.asarray(base, np.float32) * mask).astype(np.uint8))
        shown = Image.new("RGB", fixed.size, (128, 128, 128)); shown.paste(fixed, mask=fixed.split()[3])
        if redraw_api.writing_matches(o.resize((W * k, H * k), Image.NEAREST), shown.resize((W * k, H * k), Image.LANCZOS)):
            dst = os.path.join(out, rel + ".png"); os.makedirs(os.path.dirname(dst), exist_ok=True); fixed.save(dst)
            return rel, f"{c}, {len(found)} spots, {mask.mean() * 100:.0f}% original"
    return rel, None


def main():
    ext, out, cands = sys.argv[1], sys.argv[2], sys.argv[3:]
    os.makedirs(out, exist_ok=True)
    rels = [l.rstrip("\n") for l in sys.stdin if l.strip()]
    with ThreadPoolExecutor(8) as pool, open(os.path.join(out, "fix.tsv"), "a") as log:
        for rel, how in pool.map(lambda r: one(ext, cands, out, r), rels):
            log.write(f"{rel}\t{how or 'none: the original stays'}\n"); print(rel, how or "NONE", flush=True)


if __name__ == "__main__":
    main()
