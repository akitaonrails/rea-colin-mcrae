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
| 2026-10-21T05:05Z | 14h47m | recover-qmixer-wave-load-and-play-path | `82eef5b` | H02 | REA recovery of the QSound init (22050 Hz / 30 channels) + wave load (`QSWaveMixOpenWave`) + play (`QSWaveMixPlayEx`) path; docs-only recovery milestone (no code yet). |
| 2026-10-21T05:07Z | 14h49m | recover-qmixer-mixer-control-surface | `8426edb` | H03 | REA recovery of per-channel volume/frequency/3D-position controls, stop, and channel-busy query; docs-only recovery milestone. |
| 2026-10-21T14:03Z | 23h45m | define-reconstruction-owned-audio-mixer-contract | `a462ea4` | H04 | `cmr-core::audio_mixer`: backend-agnostic `AudioMixer` trait + deterministic `MixerSessionState` reproducing the recovered init guard / 22050 Hz / 30-channel cap / wave-slot table / PCM `WAVEFORMATEX` / per-channel controls, with a shared `check_audio_mixer_contract` (12 tests). First audio *code* milestone; H04 → partial. |
| 2026-10-21T14:06Z | 23h48m | recover-weather-crowd-ambience-wave-inventory | `0b5d4db` | H02 | Recovered the ambience loader `FUN_0041cd30`: six 11025 Hz/16-bit waves (`crowd`, `abduct`, storm-gated `clap1/clap2/rumble1/rumble2`), encoded as metadata driving the mixer contract in `cmr-core::audio_weather_assets` (4 tests). |
| 2026-10-21T14:09Z | 23h51m | replay-recovered-ambience-load-sequence-through-the-mixer-contract | `655b7c2` | H02 | `load_weather_ambience` replays the recovered `FUN_0041cd30` load order through any `AudioMixer`, reproducing the base+relative path concatenation and per-wave parameters and returning slot ids; 3 tests over a recording mixer (7 total in the module). First step-3 audio reconstruction driver. |
| 2026-10-21T14:12Z | 23h54m | recover-crowd-weather-ambience-trigger-and-volume-model | `454a794` | H02, H03 | Recovered the ambience trigger tick `FUN_0041cfe0` + volume setter `FUN_0047eb70`: shared channel, slot-offset layout, loop/one-shot play flags (`0xffffffff`/`0`), master-volume percentage scale, and the 32000 crowd-volume clamp. Encoded play flags + master scale in `audio_mixer`, clamp + thunder selector in `audio_weather_assets` (23 audio tests). Resolves the prior PlayEx-flags unknown. |
| 2026-10-21T14:14Z | 23h56m | recover-abduct-ambience-trigger-and-shared-channel-arbitration | `7129da4` | H02 | Recovered the abduct trigger `FUN_0041d270` (slot `base+1`, one-shot, volume `0x7fff`), completing the ambience group's shared-channel arbitration: only crowd loops, suppressed while a one-shot abduct/thunder voice holds the channel. Encoded `ABDUCT_VOLUME` + `is_looping_ambience_slot` (10 module tests). |
| 2026-10-21T14:18Z | 24h00m | reconstruct-ambience-trigger-ticks-as-pure-command-emitters | `ad95a3d` | H02, H03 | `cmr-core::audio_ambience::AmbienceController`: both recovered ticks (`FUN_0041d270`, `FUN_0041cfe0`) reconstructed as pure functions emitting the exact `MixerCommand` sequence, with recovered flag arbitration and master-scaled volumes; proximity/curve/selector values are caller inputs. 10 tests. Step-3 reconstruction of the ambience subsystem. Conversion reaches 24h wall-clock from T0. |
| 2026-10-21T14:21Z | 24h03m | catalogue-the-recovered-sound-asset-namespace | `01786ec` | H02 | String-table recovery of the full sound namespace, catalogued in `cmr-core::audio_assets`: crowd/weather waves (incl. `wet_out`/`rain_in`), frontend/replay music, menu UI sounds, and the car/accident/prangs/newsurfs/speech/rallschl directory groups (4 tests). Breadth map for the remaining loaders. |
| 2026-10-21T14:23Z | 24h05m | recover-the-menu-ui-sound-loader | _(latest)_ | H02 | Recovered the menu UI loader `FUN_00468f50`: four 22050 Hz/16-bit mono waves (`FORWARD`/`BACKWARD`/`LEFTRIGH`/`UPDOWN.WAV`) on a dedicated full-volume menu channel. Corrected the catalog (`MENU_WAVE_FILES` vs the distinct `MENU_EVENT_SOUND_NAMES`); 5 tests. Ties audio to the reconstructed frontend. |

> A commit cannot embed its own final hash, so the most recent row is marked
> _(latest)_; the next milestone update pins its hash from `git log`.
