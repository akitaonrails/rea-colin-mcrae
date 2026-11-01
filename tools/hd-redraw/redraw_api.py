"""The Nano Banana 2 call shared by the redraw scripts. The key comes from
GEMINI_API_KEY and is never printed or written."""
import base64, io, json, os, time, urllib.error, urllib.request
import numpy as np
from PIL import Image

MODEL = "gemini-3.1-flash-image"
# The aspect ratios the model accepts.
RATIOS = {"1:1": 1, "4:3": 4/3, "3:4": 3/4, "3:2": 1.5, "2:3": 2/3, "16:9": 16/9, "9:16": 9/16,
          "5:4": 1.25, "4:5": 0.8, "21:9": 21/9, "4:1": 4, "1:4": 0.25, "8:1": 8, "1:8": 0.125}


def call(img, ratio, prompt, size="4K"):
    """One image-to-image request; the reply's image, RGB."""
    buf = io.BytesIO(); img.save(buf, "PNG")
    body = {"contents": [{"parts": [{"text": prompt},
              {"inline_data": {"mime_type": "image/png", "data": base64.b64encode(buf.getvalue()).decode()}}]}],
            "generationConfig": {"responseModalities": ["IMAGE"], "imageConfig": {"aspectRatio": ratio, "imageSize": size}}}
    req = urllib.request.Request(f"https://generativelanguage.googleapis.com/v1beta/models/{MODEL}:generateContent",
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json", "x-goog-api-key": os.environ["GEMINI_API_KEY"]})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            d = json.load(r)
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"API error {e.code}: {e.read()[:300].decode('utf-8', 'replace')}")
    for c in d.get("candidates", []):
        for p in c.get("content", {}).get("parts", []):
            if "inlineData" in p:
                return Image.open(io.BytesIO(base64.b64decode(p["inlineData"]["data"]))).convert("RGB")
    raise RuntimeError("no image in reply: " + json.dumps(d)[:200])


def redraw(img, ratio, prompt):
    """`call`, retried on rate limits, server errors and empty replies."""
    for attempt in range(4):
        try:
            return call(img, ratio, prompt)
        except RuntimeError as e:
            if any(c in str(e) for c in ("429", "500", "503", "no image")):
                time.sleep(10 * (attempt + 1)); continue
            raise
    raise RuntimeError("gave up after retries")


def fit_ratio(img, fill):
    """img padded (centred, with `fill`) to the nearest ratio the model
    takes: the padded image, the ratio's name and img's offset in it."""
    w, h = img.size
    ratio = min(RATIOS, key=lambda k: abs(np.log(RATIOS[k] / (w / h))))
    r = RATIOS[ratio]
    pw, ph = (max(w, round(h * r)), h) if r >= w / h else (w, max(h, round(w / r)))
    pad = Image.new("RGB", (pw, ph), fill); ox, oy = (pw - w) // 2, (ph - h) // 2; pad.paste(img, (ox, oy))
    return pad, ratio, (ox, oy)
