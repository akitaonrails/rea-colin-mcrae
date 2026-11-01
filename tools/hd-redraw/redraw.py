"""Redraw whole images with Nano Banana 2 at 4K (3x for images wider than
640, so twins stay within the game's 8192 limit), best of three by drift
from the original. Writes <out>/<path>.png (alpha for keyed images: black
is transparent) and <out>/log.tsv (path, drift, kind, seconds).
usage: redraw.py <extracted> <out> [path prefix ...]
Car textures (class `car`) are redrawn as texture atlases; name them with a
prefix (e.g. Game/Objects/Cars/Sub). Without prefixes: the full-screen pictures, the main menu's pieces and the
race HUD's larger images. The menu ring frames (ring.py) and large
detailed pictures (tiles.py) have their own scripts and are left out.
Key: GEMINI_API_KEY, never printed."""
import os, sys, threading, time
from concurrent.futures import ThreadPoolExecutor
import numpy as np
from PIL import Image
from scipy import ndimage

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import redraw_api

PROMPTS = {
 "picture": "This is a 640x480 screen from a 1998 PC rally video game. Remaster it as a clean, sharp, highly detailed high-resolution image, as if the artist had originally painted and rendered it at 4K. Keep EXACTLY the same composition, framing, positions, proportions, colours, lighting and every piece of text with the same typeface, wording and placement. Do not add, remove, move or restyle anything; no new objects, no new text, no borders. Text must be crisp, like a high-resolution vector font.",
 "texture": "This is a texture atlas of a 1998 rally car from a video game: flat pieces of the car's livery, "
  "panels, decals, numbers, windows or wheels, laid out side by side and wrapped onto a 3D model, shown enlarged with "
  "visible pixels. Redraw it as the same texture made at high resolution: every piece, edge, decal, stripe, logo, "
  "letter and number in EXACTLY the same place, size, angle and colour (the 3D model depends on it), with clean sharp "
  "detail and smooth gradients instead of pixels. Writing (sponsor names, numbers, number plates) must be crisp, "
  "legible and correctly spelled: the real names the original shows; where a mark is too small to read, draw it as "
  "simple shapes, never made-up letters. Do not add, remove, move or restyle anything; keep pure black areas pure "
  "black (#000000).",
 "art": "This is a small user-interface graphic from a 1998 PC rally video game, shown enlarged. Redraw it from scratch as crisp high-resolution vector artwork (as if drawn in Adobe Illustrator): perfectly smooth straight lines, clean diagonals and round curves, no pixel staircases, no blur, no noise. Keep EXACTLY the same design, layout, positions, proportions, line widths, flat colours and text (same letterforms). Do not add, remove or move anything. The pure black background must stay pure flat black (#000000).",
}
# A try this close to the original (RGB RMSE at the original's size) is kept.
GOOD = {"picture": 28, "art": 40, "texture": 20}
DEFAULT = ("ini/English/", "ini/loading", "ini/loaded", "ini/cdtemplate", "New_Fe/Images/Backdrop/",
           "New_Fe/Images/Load/", "New_Fe/Images/MainOpts/English/", "Game/Images/osdnew/high/",
           "Game/Images/osdnew/revs/", "Game/Images/osdnew/ARRWS")
OWN_SCRIPT = ("/Anim/", "Backdrop/Main.bmp", "Game/Welcome/")
# Pieces the game joins on screen (the rev bar's segments): the model keeps
# their staircases or breaks them; `cmr-hd upscale` (depixel) joins them.
JOINED = ("/2wheel/rev0",)


def angular(a):
    """How much a square image changes going round its centre."""
    h, w = a.shape; R = min(h, w) / 2
    r, t = np.meshgrid(np.linspace(0.15 * R, 0.7 * R, 24), np.linspace(0, 2 * np.pi, 180, endpoint=False))
    v = ndimage.map_coordinates(a, [h / 2 + r * np.sin(t) - 0.5, w / 2 + r * np.cos(t) - 0.5], order=1)
    return float(np.abs(np.diff(v, axis=0)).mean())


def keep_original(img, siblings):
    """For a car texture: whether it is a spinning wheel (a sharper wheel of
    the same size is in the same model, and this is it smeared round its
    centre) or a plain panel next to a detailed one. Redrawn, the first
    shows still spokes at speed and the model engraves details into the
    second; both keep the original."""
    a = np.asarray(img.convert("L"), np.float32)
    if a.shape[0] != a.shape[1]: return False
    for sib in siblings:
        b = np.asarray(sib.convert("L"), np.float32)
        if b.shape != a.shape: continue
        ratio = angular(a) / max(angular(b), 1e-3)
        corr = float(np.corrcoef(ndimage.gaussian_filter(a, 3).ravel(), ndimage.gaussian_filter(b, 3).ravel())[0, 1])
        if (ratio < 0.7 and corr > 0.95) or (ratio < 0.45 and corr > 0.6): return True
    return False


def flat_rgb(img):
    rgb = Image.new("RGB", img.size); rgb.paste(img, mask=img.split()[3]); return rgb


def keyed_alpha(img):
    a = np.asarray(img.convert("RGB"), dtype=np.int32)
    alpha = np.where(a.max(axis=2) < 24, 0, 255).astype(np.uint8)
    out = img.convert("RGBA"); out.putalpha(Image.fromarray(alpha)); return out


def once(rgb, kind, scale):
    """One redraw at scale, padded to a ratio the model takes; small art is
    shown enlarged (sharp pixels) so the model sees its shapes."""
    w, h = rgb.size
    pad, ratio, (ox, oy) = redraw_api.fit_ratio(rgb, (0, 0, 0))
    pw, ph = pad.size
    k = max(1, 1024 // max(pw, ph))
    got = redraw_api.redraw(pad.resize((pw * k, ph * k), Image.NEAREST), ratio, PROMPTS[kind])
    return got.resize((pw * scale, ph * scale), Image.LANCZOS).crop((ox * scale, oy * scale, (ox + w) * scale, (oy + h) * scale))


def drift(big, rgb):
    small = np.asarray(big.resize(rgb.size, Image.BOX), dtype=float)
    return float(np.sqrt(((small - np.asarray(rgb, dtype=float)) ** 2).mean()))


def best_of(rgb, kind, scale, tries=3):
    best = None
    for _ in range(tries):
        got = once(rgb, kind, scale); e = drift(got, rgb)
        if best is None or e < best[1]: best = (got, e)
        if e <= GOOD[kind]: break
    return best


def main():
    ext, out = sys.argv[1], sys.argv[2]
    prefixes = tuple(sys.argv[3:]) or DEFAULT
    os.makedirs(out, exist_ok=True)
    lock = threading.Lock()
    log = open(os.path.join(out, "log.tsv"), "a")

    def job(rel, cls):
        dst = os.path.join(out, rel + ".png")
        if os.path.exists(dst): return
        orig = Image.open(os.path.join(ext, rel + ".png")).convert("RGBA")
        kind = {"picture": "picture", "car": "texture"}.get(cls, "art")
        t = time.time()
        try:
            img, e = best_of(flat_rgb(orig), kind, 3 if orig.width > 640 else 4)
            if cls in ("interface", "hud", "car"): img = keyed_alpha(img)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            img.save(dst)
            line = f"{rel}\t{e:.1f}\t{kind}\t{time.time() - t:.0f}s"
        except Exception as ex:
            line = f"{rel}\tFAILED\t{str(ex)[:200]}\t"
        with lock:
            log.write(line + "\n"); log.flush(); print(line, flush=True)

    models = {}
    for line in open(os.path.join(ext, "manifest.tsv")):
        f = line.rstrip("\n").split("\t")
        if len(f) > 1 and f[1] == "car": models.setdefault(f[0].rsplit("/", 1)[0], []).append(f[0])
    seen, jobs = set(), []
    for line in open(os.path.join(ext, "manifest.tsv")):
        if line.startswith("#"): continue
        rel, cls, _, w, h, name = line.rstrip("\n").split("\t")[:6]
        if name in seen or not rel.startswith(prefixes) or any(p in rel for p in OWN_SCRIPT + JOINED): continue
        img = Image.open(os.path.join(ext, rel + ".png"))
        colours = len(img.getcolors(1 << 20) or [0] * 999)
        if min(int(w), int(h)) < 12 or colours <= 2: continue
        if cls == "car":
            sibs = [Image.open(os.path.join(ext, r + ".png")) for r in models[rel.rsplit("/", 1)[0]] if r != rel]
            if keep_original(img, sibs):
                print(rel, "kept the original (a spinning wheel or a plain panel)", flush=True); continue
        seen.add(name); jobs.append((rel, cls))
    print(len(jobs), "images", flush=True)
    with ThreadPoolExecutor(6) as pool:
        list(pool.map(lambda j: job(*j), jobs))


if __name__ == "__main__":
    main()
