"""Redraw the signs among the stages' trackside objects (class `scenery` in
`cmr-hd extract`: start and finish banners, sponsor billboards, rally
plates, road and message boards) with Nano Banana 2 at 4x.

A vision model picks the signs out of everything else (trees, buildings,
spectators), says how each must turn to read normally and transcribes its
writing; each is redrawn upright with that exact writing in the prompt
(left to itself, the image model misreads names: SNR became SMX), best of
three by drift, and turned back. `pick.py` then keeps the
ones whose writing is real, and `cmr-hd import` holds them to the
original. The verdicts are kept in <out>/signs.tsv (asked once).
usage: signs.py <extracted> <out> [path prefix ...]
Writes <out>/<path>.png (black is transparent) and <out>/log.tsv.
Key: GEMINI_API_KEY, never printed."""
import os, sys, threading
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

S = 4
QUESTION = ("This is a small texture from a 1998 rally video game's trackside scenery, shown enlarged. Is it a sign: "
  "a start or finish banner, a sponsor billboard or banner, a rally name plate, a road sign or a message board, with "
  "writing or a logo on it? If it is not (a tree, a building, a person, a vehicle, a plain surface), answer NO. If it "
  "is, answer SIGN and the number of degrees it must be turned clockwise so its writing reads normally: SIGN 0, "
  "SIGN 90, SIGN 180 or SIGN 270. Reply with only that.")
PROMPT = ("This is a trackside sign from a 1998 rally video game (a start or finish banner, a sponsor billboard, a "
  "rally plate, a road or message board), shown enlarged with visible pixels. Redraw it from scratch as a crisp, "
  "clean high-resolution graphic, as if newly printed or painted: exactly the same design, layout, colours, logos "
  "and material (cloth banner, painted wood, metal plate), and the same text with the same wording, spelling and "
  "lettering style, in exactly the same place and size. All writing must be sharp, legible and correctly spelled; "
  "where a word is too small to read, keep it as simple shapes rather than making letters up. Do not add, remove or "
  "move anything. Keep pure black areas pure black (#000000).")
TRANSCRIBE = ("This is a sign from a 1998 rally video game, shown enlarged with visible pixels. Transcribe exactly the "
  "writing on it, line by line, as it is printed (same spelling and case). It is a sponsor, rally or place name, a "
  "road sign or a message: use what you know of real 1998 rally sponsors and places to read blurry letters, but do "
  "not guess what you cannot read. Reply with only the writing, lines separated by ' / ', or NONE.")
TURN = {0: None, 90: Image.Transpose.ROTATE_270, 180: Image.Transpose.ROTATE_180, 270: Image.Transpose.ROTATE_90}
BACK = {0: None, 90: Image.Transpose.ROTATE_90, 180: Image.Transpose.ROTATE_180, 270: Image.Transpose.ROTATE_270}


def on(img, colour):
    bg = Image.new("RGB", img.size, colour); bg.paste(img, mask=img.split()[3]); return bg


def verdict(img):
    """None for not a sign, else the clockwise turn (degrees) to read it."""
    shown = on(img, (128, 128, 128)); k = max(1, 768 // max(shown.size))
    answer = redraw_api.ask(shown.resize((shown.width * k, shown.height * k), Image.NEAREST), QUESTION).upper()
    if not answer.startswith("SIGN"): return None
    digits = "".join(c for c in answer if c.isdigit())
    return int(digits) if digits and int(digits) in TURN else 0


def redraw(img, text):
    """img (RGBA, upright) redrawn at S x with its writing `text`, best of
    three by drift."""
    prompt = PROMPT if text.upper() == "NONE" else PROMPT + f" The writing on it reads exactly: {text}."
    rgb = on(img, (0, 0, 0)); w, h = rgb.size
    pad, ratio, (ox, oy) = redraw_api.fit_ratio(rgb, (0, 0, 0))
    pw, ph = pad.size; k = max(1, 1024 // max(pw, ph))
    best = None
    for _ in range(3):
        got = redraw_api.redraw(pad.resize((pw * k, ph * k), Image.NEAREST), ratio, prompt)
        got = got.resize((pw * S, ph * S), Image.LANCZOS).crop((ox * S, oy * S, (ox + w) * S, (oy + h) * S))
        e = float(np.sqrt(((np.asarray(got.resize((w, h), Image.BOX), np.float32) - np.asarray(rgb, np.float32)) ** 2).mean()))
        if best is None or e < best[1]: best = (got, e)
        if e < 20: break
    return best


def keyed_alpha(img):
    a = np.asarray(img.convert("RGB"), dtype=np.int32)
    out = img.convert("RGBA"); out.putalpha(Image.fromarray(np.where(a.max(axis=2) < 24, 0, 255).astype(np.uint8)))
    return out


def main():
    ext, out = sys.argv[1], sys.argv[2]
    prefixes = tuple(sys.argv[3:]) or ("",)
    os.makedirs(out, exist_ok=True)
    seen, rels = set(), []
    for line in open(os.path.join(ext, "manifest.tsv")):
        if line.startswith("#"): continue
        f = line.rstrip("\n").split("\t")
        if f[1] == "scenery" and f[0].startswith(prefixes) and f[5] not in seen and min(int(f[3]), int(f[4])) >= 8:
            seen.add(f[5]); rels.append(f[0])
    cache_path = os.path.join(out, "signs.tsv")
    cache = dict(l.rstrip("\n").split("\t")[:2] for l in open(cache_path)) if os.path.exists(cache_path) else {}
    lock = threading.Lock()

    texts = {}
    if os.path.exists(cache_path):
        for l in open(cache_path):
            f = l.rstrip("\n").split("\t")
            if len(f) > 2: texts[f[0]] = f[2]

    def classify(rel):
        if rel in cache: return
        img = Image.open(os.path.join(ext, rel + ".png")).convert("RGBA")
        v = verdict(img)
        text = ""
        if v is not None:
            up = on(img.transpose(TURN[v]) if TURN[v] else img, (128, 128, 128)); k = max(1, 768 // max(up.size))
            text = redraw_api.ask(up.resize((up.width * k, up.height * k), Image.LANCZOS), TRANSCRIBE).replace("\n", " / ")
        with lock:
            cache[rel] = "no" if v is None else str(v); texts[rel] = text
            with open(cache_path, "a") as f: f.write(f"{rel}\t{cache[rel]}\t{text}\n")

    with ThreadPoolExecutor(8) as pool: list(pool.map(classify, rels))
    signs = [r for r in rels if cache.get(r, "no") != "no"]
    print(f"{len(rels)} textures, {len(signs)} signs", flush=True)
    log = open(os.path.join(out, "log.tsv"), "a")

    def job(rel):
        dst = os.path.join(out, rel + ".png")
        if os.path.exists(dst): return
        deg = int(cache[rel])
        img = Image.open(os.path.join(ext, rel + ".png")).convert("RGBA")
        try:
            got, e = redraw(img.transpose(TURN[deg]) if TURN[deg] else img, texts.get(rel) or "NONE")
            got = got.transpose(BACK[deg]) if BACK[deg] else got
            os.makedirs(os.path.dirname(dst), exist_ok=True); keyed_alpha(got).save(dst)
            line = f"{rel}\t{e:.1f}\tsign {deg}\t{texts.get(rel, '')}"
        except Exception as ex:
            line = f"{rel}\tFAILED\t{str(ex)[:200]}"
        with lock: log.write(line + "\n"); log.flush(); print(line, flush=True)

    with ThreadPoolExecutor(6) as pool: list(pool.map(job, signs))


if __name__ == "__main__":
    main()
