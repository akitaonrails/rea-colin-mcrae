# hd-redraw

Scripts that redraw the game's menus, loading screens, larger HUD
images and the stages' skies with Google's Nano Banana 2
(`gemini-3.1-flash-image`) for the HD art pack. What each one is for,
what works and what doesn't, and the whole pipeline with `cmr-hd`:
[docs/UPSCALING.md](../../docs/UPSCALING.md).

| Script | Does |
| --- | --- |
| `redraw.py` | whole images, one request each, best of three |
| `filter.py` | moves `redraw.py` results that drift or lose their transparency aside |
| `align.py` | aligns `redraw.py` results to their originals |
| `ring.py` | the main menu's ring of words, frame by frame |
| `tiles.py` | large pictures in tiles, tone kept from the original |
| `sky.py` | every stage's sky and distant scenery (`*.hzn`) |
| `sheet.py` | a family of small sprites (the race's car icons) on one sheet |
| `signs.py` | the trackside signs: picked out, transcribed, redrawn upright |
| `pick.py` | per image, the first candidate redraw whose writing is real |
| `fixtext.py` | puts the original back where a redraw's writing is wrong |
| `redraw_api.py` | the shared request, retries and aspect-ratio padding |

```sh
pip install -r requirements.txt
export GEMINI_API_KEY=...   # your own key; the scripts never print it
```

Inputs come from `cmr-hd extract`; write outputs under `.work/`. Never
commit them: they are made from the game's art.

They stay in Python (owner decision, see `AGENTS.md`); they make the pack
and are not shipped.
