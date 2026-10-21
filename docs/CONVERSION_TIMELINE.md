# Conversion timeline

Wall-clock tracking of the full Colin McRae Rally reconstruction effort, for
later analysis of how long the conversion takes.

## Anchor

- **Conversion start (T0):** `2026-10-20T14:18:12Z` — first working session
  (codex, session `01a159ab-75aa-7670-943d-bdbe0144c0a6`). The first commit
  (`eba3b40`, evidence baseline) landed six minutes later at
  `2026-10-20T14:24:57Z`.
- All elapsed figures below are wall-clock from T0 (includes idle gaps between
  sessions; it is not active-work time).

## Method

Append one row per **milestone commit** (a commit that completes or materially
advances a `docs/DECOMP_MAP.md` item). Record the commit's UTC time, elapsed
from T0, the milestone slug from the `DECOMP_MAP` progress-tail, and the items
touched. Per-commit granularity for the full history is recoverable with
`git log --format='%h %cI %s'`.

## Milestones

| UTC timestamp | Elapsed from T0 | Milestone slug | Commit | DECOMP items | Notes |
| --- | --- | --- | --- | --- | --- |
| 2026-10-20T14:18:12Z | 0h00m | (session start) | — | — | Conversion begins (codex). |
| 2026-10-20T14:24:57Z | 0h07m | reconstruction-evidence-baseline | `eba3b40` | — | First commit; evidence/roadmap scaffolding. |
| 2026-10-21T04:21:29Z | 14h03m | add-portable-headless-root-application | `cb92783` | I02, I03 | Headless root-frame application; end of codex session (113 commits total covering subsystems A–H recovery and I01–I06). |
| 2026-10-21T04:33Z | 14h15m | implement-sdl3-software-frame-presentation-host | `5eb7d3c` | I03, I07 | SDL3 host window/input/presentation + indexed→RGBA8 expansion; audio host and live input mapping remain. |
| 2026-10-21T04:42Z | 14h24m | map-sdl-input-into-recovered-frontend-input | `610122b` | I07 | Edge-triggered SDL keyboard/gamepad → recovered `FrontendInput` mapper, contract-tested against `FrontendState`; live interactive loop and audio host remain. |
| 2026-10-21T04:50Z | 14h32m | compose-pure-interactive-root-menu-controller | `927504f` | I07 | Pure `RootInteraction` controller + `steady_root_render_input` bridge (snap to recovered endpoints, minimal root/exit transition driver); SDL loop wiring and audio host remain. |
| 2026-10-21T04:58Z | 14h40m | wire-sdl-interactive-root-menu-loop | `e0db2e6` | I03, I07 | SDL interactive loop + `--windowed-root-frame` wiring (live input → controller → re-presented frames); only the audio host remains for I07, displayed run gated. |
| 2026-10-21T05:05Z | 14h47m | recover-qmixer-wave-load-and-play-path | _(latest)_ | H02 | REA recovery of the QSound init (22050 Hz / 30 channels) + wave load (`QSWaveMixOpenWave`) + play (`QSWaveMixPlayEx`) path; docs-only recovery milestone (no code yet). |

> A commit cannot embed its own final hash, so the most recent row is marked
> _(latest)_; the next milestone update pins its hash from `git log`.
