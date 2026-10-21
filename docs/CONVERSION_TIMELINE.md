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
| 2026-10-21T14:23Z | 24h05m | recover-the-menu-ui-sound-loader | `cd9f047` | H02 | Recovered the menu UI loader `FUN_00468f50`: four 22050 Hz/16-bit mono waves (`FORWARD`/`BACKWARD`/`LEFTRIGH`/`UPDOWN.WAV`) on a dedicated full-volume menu channel. Corrected the catalog (`MENU_WAVE_FILES` vs the distinct `MENU_EVENT_SOUND_NAMES`); 5 tests. Ties audio to the reconstructed frontend. |
| 2026-10-21T14:26Z | 24h08m | recover-and-reconstruct-the-menu-sound-play-helpers | `59b2caa` | H02 | Recovered the menu play helpers `FUN_00467ef0`/`FUN_00467dd0` (full volume, stop, one-shot play of slot `base+offset`; confirm=0/nav=3 carved) and reconstructed them in `cmr-core::audio_menu::play_menu_sound` (4 tests). Maps the frontend navigation sounds to recovered slots. |
| 2026-10-21T14:30Z | 24h12m | recover-the-full-wave-inventory-and-car-engine-model | `6e0bae7` | H02 | String-table recovery of the complete bare wave-filename inventory: car engine model (12 cars × 6 `{RT,FT,RM,FM,RS,FS}` loops + shared), co-driver pacenote calls, surface/collision materials, tutorial numbers, and per-language cheat/secret sounds. Structured car model encoded in `cmr-core::audio_car_assets` (4 tests). |
| 2026-10-21T14:45Z | 24h27m | recover-the-car-loader-body-and-selection-remap | `97a9077` | H02 | Full decompile of the car loader `FUN_004872b0` (via subagent): car-selection nibble→row-index switch remap, 14 waves at 11025 Hz/16-bit/mono in fixed order (`FS,RS,FM,RM,WHINE,FT,RT,CHATTER,POPLOOP1-4,IGNITION,GEAR`), slots `+0..+7` plus a `DAT_00550c08==2` tail. Encoded `CAR_WAVE_RATE_HZ`/`CAR_LOAD_SEQUENCE`/`car_row_index_for_selector` (7 car tests). |
| 2026-10-21T14:52Z | 24h34m | recover-the-engine-rev-band-crossfade-model | `fc7e1ae` | H02, H03 | Engine tick `FUN_00487e00`/whine `FUN_00488ab0` (via subagent): per-layer looping channels (`base+slot+player*8`), three stereo rev-band pairs (Slow 0/1, Mid 2/3, Top 5/6), band index `((RPM-lo)*count)/(hi-lo)` with out-of-window muting and overlap crossfade, throttle-gated Top. Reconstructed in `cmr-core::audio_engine` (6 tests). Corrected the F/R inference (stereo pan, not on/off power). |
| 2026-10-21T15:03Z | 24h45m | recover-the-surface-and-collision-sound-banks | `8dc2228` | H02, H03 | Surface/collision sound banks (via subagent): collision/prang loader `FUN_00469ac0` (5 full-volume channels, 48 `prangs`/`accident` waves, impact `FUN_00469e80`) and surface/skid loader `FUN_004892b0` (`newsurfs`, 4 looping channels, per-object-type `<type>_o/i/sko/ski<n>.wav` variants, roll `FUN_0046a030`). Reconstructed the roll volume ramp (`base/3 + (count-5)*base/0x2d`, full at `0x24`), variant filename shape, and bank constants in `cmr-core::audio_surface` (3 tests). The impact sub-wave/volume bit-expression is deliberately left to the evidence ledger (its literal transcription degenerates). |
| 2026-10-21T15:15Z | 24h57m | recover-the-co-driver-pacenote-speech-subsystem | _(latest)_ | H02, H03 | Co-driver pacenote (speech) subsystem (via subagent): loader `FUN_00444160` (40-wave bank at 11025 Hz/16-bit/mono, slot==load index, `<root>`+`<language dir>`+`codriver\`+filename, language `cfg[0x18]&7` over english/french/german/spanish/italian/helium), submit `FUN_00444380` + sequencer `FUN_00444450` (one component at a time, one-shot, single shared channel gated by `IsChannelDone`, `INTO` linking queued notes, 8-entry ring + depth-2 per-track FIFO). Reconstructed the inventory, format/channel constants, language gate, `codriver\` path, and the observed per-word field-extraction arithmetic in `cmr-core::audio_pacenote` (6 tests). Higher packed-word field semantics and the FIFO `9*(n-1)` accumulation left to the evidence ledger. |

> A commit cannot embed its own final hash, so the most recent row is marked
> _(latest)_; the next milestone update pins its hash from `git log`.
