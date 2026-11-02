# Playbook: use community fixes, packs and tools

How a launcher pins, ships, downloads, installs and configures what the
community made (SilentPatch, OutRun2006Tweaks, Xidi, DXVK, dgVoodoo2, HD
texture packs). The goal: **the player never downloads, unpacks, copies or
configures anything by hand.**

## 1. Decide how it reaches the player

Read the component's licence first (record it in `docs/RESEARCH.md`).

| Situation | How | Example |
|---|---|---|
| Licence allows redistribution | **Bundle**: fetched at package time, inside the zip/AppImage | DXVK, Xidi, SilentPatch, Tweaks |
| No licence, or redistribution unclear, or very large | **Download on request**: a button in the window fetches it from the author's own release | OutRun's HD texture pack |
| Licence-less but the owner decided to bundle | Bundle, with the owner's decision recorded in `THIRD_PARTY.md` | CMR3's HD UI |
| Must be built from source | Build in CI from a pinned commit, publish as our release asset | Screamer's dosbox-staging, CMR3's vkBasalt |

Choose in this order: **bundle** (always preferred — check the licence
carefully before falling back), then **download on request** with one
button, never a manual step. Never ask the player to fetch something
themselves when the launcher could.

**Microsoft redistributables** (d3dx9, DirectPlay, d3dcompiler, VC++
runtimes): try Wine's builtins first and verify in the game. If the native
DLL is really needed, check its redistribution licence (the VC++ runtime
allows bundling; DirectX redistributables have their own terms) and either
bundle it or download it on the player's request — never a winetricks
command the player has to run, and never a silent download.

## 2. Pin it

Every file is pinned by URL and SHA-256 in a pin file (`packaging/*.pins`, or one `packaging/<name>.txt` per pin — `pins.sh` reads both)
(`sdk/packaging/pins.sh` format: `name url sha256 [fallback-url…]`).

- **Mirror** what the licence allows into the launcher's own GitHub release
  (the mirror workflow, the SDK's reusable `mirror.yml`) and list it as a
  fallback URL, so a build still works when the upstream file moves or
  disappears: add the word `mirror` to the pin and its `pin_mirror_url` as
  the last fallback, then run the mirror workflow
  (`sdk/packaging/README.md`, "Mirroring").
- **Licence-less, but the owner decided to host it** (table above): record
  the decision in `THIRD_PARTY.md` first (who, when, why, the author's
  terms), then mark the pin `hosted` instead of `mirror` and call the mirror
  workflow with `include-hosted: true`. Without that input the workflow
  skips `hosted` pins, so nothing licence-less is republished by accident.
- **Upstream builds that change often** (Tweaks): repack each build as a
  release of ours and pin that (OutRun's `update-tweaks.sh`).
- **Watch upstream** with the SDK's `upstream-watch.yml`: it opens an issue
  when a pinned project publishes something new, so updating stays a
  deliberate, tested change.

## 3. Install it into the runtime

- Through `rea_launcher::ledger`: the launcher only replaces files it owns
  or untouched game copies, removes only its own, and restores the game's
  own file when the player switches the component off.
- Wine needs native DLLs said out loud: build `WINEDLLOVERRIDES` with
  `rea_launcher::wine::dll_overrides_for`, merged with the player's own.
- Large read-only packs are **pointed to, not copied** when the component
  can read a folder (OutRun's HD pack via Tweaks' texture folder).

## 4. Configure it

Generate the component's config from `launcher.cfg` (INI editor that keeps
the player's other keys; only the launcher's own keys are rewritten each
run). The player picks a look or an option in the window; they never edit
the component's files.

## 5. Download on request (packs, optional extras)

Use `rea_launcher::download` with the platform fetcher
(`rea_launcher_win32::http` on Windows) and the window's download row
(`rea_launcher_ui`):

- Pin: URL, SHA-256, size, unpacked size.
- Check free space first; resume an interrupted download; verify the hash
  while streaming; unpack to a temporary folder (no path escapes) and rename
  into place; mark completion with the hash; remove older versions.
- Show progress and a Cancel button; the game stays playable meanwhile and
  the option turns on by itself when the download finishes.
- Never download without the player pressing the button (no network access
  otherwise).

## 6. Credit it

`THIRD_PARTY.md`: component, version, licence, source URL, how it ships
(bundled / downloaded / built). README credits section names every
community project the launcher stands on.
