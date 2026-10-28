# Runtime notes

Observations from running the user's own game. Each entry: date, setup, what
was seen. Static facts live in `docs/recomp/` and `.work/api-surface-notes.md`.

## 2026-11-02: first Proton runs (GE-Proton 11-1, `proton run` outside Steam)

Setup: a hard-linked copy of the user's installed game with `Rally.exe`
replaced by the official EU v1.5 executable (`0eb6ea96…`); fresh prefix;
registry values from `THIN_APP_PLAN.md` R2 imported with `regedit /S`.

- Run with the default Windows version: the game loads `QMIXER.dll` from its
  folder, then shows its own "Wrong operating system" message box (the Win9x
  platform gate, `FUN_0049dcb0`) and exits.
- With `HKCU\Software\Wine\AppDefaults\Rally.exe\Version = win98`: no message
  box. Wine loads `wined3d`; a window titled "Colin McRae Rally" (1261x1390,
  not fullscreen) appeared on the real display. The 32-bit Vulkan loader was
  missing (wined3d falls back to OpenGL). Without the NVIDIA PRIME variables,
  OpenGL context queries failed with `GL_OUT_OF_MEMORY`; with them they
  didn't.
- On a private Xvfb display every captured frame was black. With extra
  tracing, Wine reported it could not find FreeType. Running Proton by hand
  outside Steam's runtime container is not a usable test harness. Next: run
  through the runtime container (`umu-launcher`, or Steam's
  `SteamLinuxRuntime` entry point).
- A CD-type drive letter (`dosdevices\r:` → a folder with `setup\game` →
  the game's `Game`, `HKLM\Software\Wine\Drives "r:"="cdrom"`) was added for
  the CD probe; not yet confirmed because of the black frames.
