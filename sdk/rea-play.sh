# rea-play.sh — shared POSIX sh library for the launchers' linux/<x>-play.
#
# A game script sources this file, then sets its configuration (REA_*
# variables, see README.md), overrides the rea_hook_* functions it needs,
# calls rea_init, and finally rea_main "$@". Sourcing only defines functions
# and drops any REA_* configuration inherited from the environment: the
# player-facing interface is each game's own variables (CMR3_PREFIX, ...),
# which the game script maps onto REA_* itself. Nothing runs Wine,
# gamescope or the game until rea_main.
#
# Everything here is POSIX sh (dash-safe) and works under `set -u`. Library
# state uses the REA_ prefix, scratch variables the _rea_ prefix. Player
# values are word-split with globbing off, so a `*` never expands.
#
# Test seams, honoured only when REA_TEST=1 is set after sourcing (a stray
# environment variable can never change a game):
#   REA_SYSFS            replaces /sys            (GPU choice)
#   REA_ICD_DIRS         replaces the Vulkan ICD dirs (space-separated)
#   REA_NV_LIB_CHECK     command run instead of the libGLX_nvidia lookup
#   REA_GAMESCOPE_CHECK  command run instead of `command -v gamescope`
#   REA_LAUNCH_CMD       command run instead of the whole supervised launch
#   REA_PROC             replaces /proc for the game-process check

REA_SDK_VERSION=0.3.3

# --- configuration -------------------------------------------------------------
# Every configuration variable the library reads. Sourcing unsets them, so
# only what the game script assigns after sourcing counts. REA_FUNCTIONS_ONLY
# and REA_PLAY_HERE (the test convention, see README.md "Tests") are read by
# the game script before it sources this file, and dropped here too.
REA_CONFIG_VARS='REA_APP REA_MODE REA_EXE REA_EXE_NAME REA_EXE_HINT REA_EXE_SEARCH
REA_HERE REA_NAME REA_PREFIX REA_STATE REA_STATE_BASE REA_RUN REA_WAIT
REA_ALWAYS_WAIT REA_CFG REA_CFG_NAME REA_CFG_APPDATA REA_PLAY_ARGS
REA_SETTINGS_ARGS REA_LEGACY_LOCK REA_INTERRUPT_WINESERVER REA_GAMESCOPE
REA_GAMESCOPE_OUTPUT REA_GAMESCOPE_EXTRA REA_RES_KEY REA_DEFAULT_SIZE
REA_ORIGINAL_SIZE REA_FULLSCREEN_SIZE REA_SCREEN REA_SCALER_KEY
REA_DEFAULT_SCALER REA_WINDOWED_KEY REA_WINDOWED_VALUES REA_SDL_ENV
REA_SDL_SETTINGS REA_GPU_ICD_PIN REA_GPU REA_DLL_OVERRIDES REA_WINEDBG_OFF
REA_GAME_PROCS REA_GAME_SETTLE REA_WINEBUS_MARKER REA_WINEBUS_VALUES
REA_DIRECT_NOTE REA_TEST REA_SYSFS REA_ICD_DIRS REA_NV_LIB_CHECK
REA_GAMESCOPE_CHECK REA_LAUNCH_CMD REA_PROC REA_INITED REA_ORIGINAL_WORDS
REA_FUNCTIONS_ONLY REA_PLAY_HERE REA_HELPER_KEYS REA_FLAG_SWITCHES REA_ONE_RUN'
# shellcheck disable=SC2086
unset $REA_CONFIG_VARS 2>/dev/null || true

# Defaults for what the game script left unset (rea_init calls this).
rea_defaults() {
  : "${REA_APP:=}"                  # state folder name, e.g. cmr3-launcher
  : "${REA_MODE:=wine}"             # wine | native
  : "${REA_EXE:=}"                  # the launcher program (found when empty)
  : "${REA_EXE_NAME:=}"             # its file name, e.g. CMR3Launcher.exe
  : "${REA_EXE_HINT:=REA_EXE}"      # the variable a player sets, for messages
  : "${REA_EXE_SEARCH:=}"           # extra candidates; relative = from REA_HERE
  : "${REA_PREFIX:=}"               # Wine prefix (default set in rea_init)
  : "${REA_STATE_BASE:=$HOME/.config}"  # SCR: ${XDG_CONFIG_HOME:-$HOME/.config}
  : "${REA_RUN:=}"                  # wrapper, e.g. "distrobox enter gaming --"
  : "${REA_WAIT:=}"                 # set by an AppImage: wait for the game
  : "${REA_ALWAYS_WAIT:=1}"         # 1: every launch waits; 0: only REA_WAIT
  : "${REA_CFG:=}"                  # launcher.cfg (discovered when empty)
  : "${REA_CFG_NAME:=launcher.cfg}"
  : "${REA_CFG_APPDATA:=}"          # AppData\Local\<this> inside the prefix
  : "${REA_PLAY_ARGS=--no-window}"            # launcher flags for a game run
  : "${REA_SETTINGS_ARGS=--settings --linux}" # flags for the window
  : "${REA_LEGACY_LOCK:=signal}"    # signal | keep | off (pid-only locks)
  : "${REA_INTERRUPT_WINESERVER:=1}"  # interrupted: wineserver -k as well
  # Game size and gamescope.
  : "${REA_GAMESCOPE:=auto}"        # auto | off
  : "${REA_GAMESCOPE_OUTPUT:=1}"    # 1: -W/-H as well as -w/-h
  : "${REA_GAMESCOPE_EXTRA:=}"      # extra gamescope flags (word-split)
  : "${REA_RES_KEY:=resolution}"
  : "${REA_DEFAULT_SIZE:=1920x1080}"
  : "${REA_ORIGINAL_SIZE:=}"        # "original" means this (empty: default)
  : "${REA_ORIGINAL_WORDS:=original}"  # the words that mean it (OR's Rust: "original off")
  : "${REA_FULLSCREEN_SIZE:=}"      # "screen": fullscreen = the screen size
  : "${REA_SCREEN:=}"               # WIDTHxHEIGHT instead of asking xrandr
  : "${REA_SCALER_KEY=scaler}"      # empty: always REA_DEFAULT_SCALER
  : "${REA_DEFAULT_SCALER:=linear}"
  : "${REA_WINDOWED_KEY:=}"         # e.g. display_mode (empty: never skip)
  : "${REA_WINDOWED_VALUES:=windowed borderless}"
  # Environment.
  : "${REA_SDL_ENV=SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1}"
  : "${REA_SDL_SETTINGS:=1}"        # the SDL defaults for the window too
  : "${REA_GPU_ICD_PIN:=0}"         # 1: pin NVIDIA's Vulkan ICD (SRR rule)
  : "${REA_DLL_OVERRIDES:=}"        # the game's WINEDLLOVERRIDES, before the caller's
  : "${REA_WINEDBG_OFF:=0}"         # 1: add winedbg=d (a fault ends the game)
  # The game's own processes, '|'-separated (Windows names cannot hold '|'):
  # when set, the inner script waits for them after the launcher returns.
  : "${REA_GAME_PROCS:=}"
  : "${REA_GAME_SETTLE:=3}"
  : "${REA_WINEBUS_MARKER:=.rea-winebus}"
  : "${REA_WINEBUS_VALUES:=DisableHidraw|DisableInput|Enable SDL|Map Controllers}"
  : "${REA_DIRECT_NOTE:=starting without a gamescope of our own}"
  # One-run flags: launcher.cfg keys the helper itself reads, which a
  # `--play --<key> value` run overrides for that run (rea_one_run).
  : "${REA_HELPER_KEYS=}"           # e.g. "scaler resolution display_mode postfx dxvk"
  : "${REA_FLAG_SWITCHES=--no-window --settings --linux --check}"  # flags without a value
}

# A test seam's value, only under REA_TEST=1.
rea_seam() {
  [ "${REA_TEST:-}" = 1 ] || return 0
  eval "printf '%s' \"\${$1:-}\""
}

# Word splitting of player values never globs: these bracket each split,
# keeping a caller's own `set -f`.
rea_noglob() { case $- in *f*) _rea_glob_off=1 ;; *) _rea_glob_off= ; set -f ;; esac; }
rea_glob() { [ -n "${_rea_glob_off:-}" ] || set +f; }

# rea_run COMMAND...: through REA_RUN (word-split without globbing).
rea_run() {
  rea_noglob
  # shellcheck disable=SC2086
  set -- $REA_RUN "$@"
  rea_glob
  "$@"
}

REA_WINEBUS_KEY='HKLM\System\CurrentControlSet\Services\winebus'

# --- hooks the game script may override ------------------------------------

rea_hook_before_settings() { :; }   # e.g. prefix bootstrap (SRR)
rea_hook_before_start() { :; }      # prefix prep, rea_game_env (postfx)
rea_hook_command() { return 127; }  # extra commands; 127 = unknown
rea_hook_usage() { :; }             # extra usage lines

# --- paths -------------------------------------------------------------------

rea_init() {
  rea_defaults
  REA_HERE=${REA_HERE:-$(dirname "$(readlink -f "$0")")}
  REA_NAME=${REA_NAME:-$(basename "$0")}
  [ -n "$REA_APP" ] || { echo "rea-play: REA_APP is not set" >&2; return 1; }
  REA_STATE=${REA_STATE:-$REA_STATE_BASE/$REA_APP}
  REA_LOCK=$REA_STATE/running.pid
  REA_INNER=$REA_STATE/inner.sh
  REA_GAME_ENV_FILE=$REA_STATE/game.env
  mkdir -p "$REA_STATE"
  if [ "$REA_MODE" = wine ]; then
    REA_PREFIX=${REA_PREFIX:-$HOME/.local/share/$REA_APP/prefix}
    mkdir -p "$REA_PREFIX"
  fi
  [ -n "$REA_EXE" ] || rea_find_exe
  [ -n "$REA_CFG" ] || rea_find_cfg
  REA_GAME_ENV=
  REA_INITED=1
  return 0
}

# The launcher next to the script, one folder up, or REA_EXE_SEARCH (each
# relative entry taken from REA_HERE, the script's folder).
rea_find_exe() {
  rea_noglob
  # shellcheck disable=SC2086
  set -- $REA_EXE_SEARCH
  rea_glob
  [ -n "$REA_EXE_NAME" ] && set -- "$REA_EXE_NAME" "../$REA_EXE_NAME" "$@"
  for _rea_p in "$@"; do
    case $_rea_p in /*) ;; *) _rea_p=$REA_HERE/$_rea_p ;; esac
    if rea_exe_ok "$_rea_p"; then
      REA_EXE=$(readlink -f "$_rea_p")
      return 0
    fi
  done
  return 0
}

rea_exe_ok() {
  if [ "$REA_MODE" = wine ]; then [ -f "$1" ]; else [ -f "$1" ] && [ -x "$1" ]; fi
}

rea_require_exe() {
  [ -n "$REA_EXE" ] && rea_exe_ok "$REA_EXE" && return 0
  echo "${REA_EXE_NAME:-the launcher} not found next to $REA_NAME (or set $REA_EXE_HINT)" >&2
  return 1
}

# launcher.cfg is the one the launcher itself reads: next to its exe (a
# portable install wins, as in every launcher's settings_path), else in its
# home: the prefix user's AppData\Local\<REA_CFG_APPDATA> (Wine) or the
# state folder (native).
rea_find_cfg() {
  if [ -n "$REA_EXE" ] && [ -f "$(dirname "$REA_EXE")/$REA_CFG_NAME" ]; then
    REA_CFG=$(readlink -f "$(dirname "$REA_EXE")/$REA_CFG_NAME")
    return 0
  fi
  if [ "$REA_MODE" = wine ] && [ -n "$REA_CFG_APPDATA" ]; then
    REA_CFG=
    for _rea_d in "$REA_PREFIX"/drive_c/users/*/AppData/Local/"$REA_CFG_APPDATA"; do
      case "$_rea_d" in */users/Public/*|*'*'*) continue ;; esac
      [ -d "$_rea_d" ] && REA_CFG=$_rea_d/$REA_CFG_NAME
    done
    [ -n "$REA_CFG" ] ||
      REA_CFG=$REA_PREFIX/drive_c/users/${USER:-$(id -un)}/AppData/Local/$REA_CFG_APPDATA/$REA_CFG_NAME
  else
    REA_CFG=$REA_STATE/$REA_CFG_NAME
  fi
}

# rea_get_key KEY [FILE]: the value of the last active `KEY = value` line
# (later lines win, as the launchers' parser applies them in order). The key
# and value are trimmed (spaces, tabs, CR); # lines are comments. The key is
# compared as text, never used as a pattern.
rea_get_key() {
  # A one-run value (rea_one_run) wins over launcher.cfg; an explicit FILE
  # is always read as it is.
  if [ -z "${2:-}" ] && [ -n "${REA_ONE_RUN:-}" ]; then
    _rea_v=$(printf '%s\n' "$REA_ONE_RUN" | awk -v key="$1" '
      { i = index($0, "="); if (substr($0, 1, i - 1) == key) { v = substr($0, i + 1); f = 1 } }
      END { if (f) { print v; exit 0 } exit 1 }') && { printf '%s\n' "$_rea_v"; return 0; }
  fi
  _rea_f=${2:-$REA_CFG}
  [ -n "$_rea_f" ] && [ -f "$_rea_f" ] || return 0
  awk -v key="$1" '
    { sub(/\r$/, "") }
    /^[ \t]*#/ { next }
    {
      i = index($0, "="); if (i == 0) next
      k = substr($0, 1, i - 1); v = substr($0, i + 1)
      gsub(/^[ \t]+|[ \t]+$/, "", k)
      if (k != key) next
      gsub(/^[ \t\r]+|[ \t\r]+$/, "", v)
      val = v; found = 1
    }
    END { if (found) print val }' "$_rea_f"
}

# rea_get_value KEY [FILE]: rea_get_key's value as the launchers compare it,
# lowercased (they accept any case: `postfx = Color`, `--display-mode
# Windowed`). Use it for every comparison; keep rea_get_key for paths.
rea_get_value() { rea_get_key "$@" | tr 'A-Z' 'a-z'; }

# rea_is_on / rea_is_off KEY [FILE]: the launchers' boolean spellings
# (on|true|yes|1, off|false|no|0, any case). Unset or anything else is
# neither, so a caller keeps its default.
rea_is_on() {
  case $(rea_get_value "$@") in on|true|yes|1) return 0 ;; esac
  return 1
}
rea_is_off() {
  case $(rea_get_value "$@") in off|false|no|0) return 0 ;; esac
  return 1
}


# rea_one_run ARGS...: reads `--<key> value` and `--<key>=value` among the
# launcher arguments of one run, for each key in REA_HELPER_KEYS (the flag
# spells the key with - for _: display_mode is --display-mode), into
# REA_ONE_RUN, which rea_get_key prefers over launcher.cfg for this run.
# The arguments still go to the launcher unchanged. Flags in
# REA_FLAG_SWITCHES take no value; any other --flag takes the next word.
# The last one wins, as in the launchers' parser. A value with a newline
# is ignored.
rea_one_run() {
  REA_ONE_RUN=
  [ -n "$REA_HELPER_KEYS" ] || return 0
  while [ "$#" -gt 0 ]; do
    _rea_a=$1; shift
    case $_rea_a in
      --*=*) _rea_flag=${_rea_a%%=*} _rea_val=${_rea_a#*=} ;;
      --*)
        _rea_flag=$_rea_a _rea_val=
        _rea_sw=
        rea_noglob
        for _rea_x in $REA_FLAG_SWITCHES; do [ "$_rea_x" = "$_rea_a" ] && _rea_sw=1; done
        rea_glob
        [ -n "$_rea_sw" ] && continue
        [ "$#" -gt 0 ] || continue
        _rea_val=$1; shift ;;
      *) continue ;;
    esac
    _rea_key=$(printf '%s' "${_rea_flag#--}" | tr - _)
    case $_rea_val in *'
'*) continue ;; esac
    rea_noglob
    for _rea_k in $REA_HELPER_KEYS; do
      [ "$_rea_k" = "$_rea_key" ] && REA_ONE_RUN="$REA_ONE_RUN$_rea_key=$_rea_val
"
    done
    rea_glob
  done
  REA_ONE_RUN=${REA_ONE_RUN%'
'}
  return 0
}

# --- sizes ---------------------------------------------------------------------

# rea_parse_size TEXT: prints WIDTHxHEIGHT for the spellings the launchers
# accept (1920X1080, " 1920 x 1080 ", a CRLF line, leading zeros), or fails
# for anything else: 0 in either side, half a size, extra parts, letters,
# more than five digits.
rea_parse_size() {
  _rea_s=$(printf '%s' "$1" | tr -d ' \t\r' | tr 'X' 'x')
  case $_rea_s in
    *[!0-9x]*|x*|*x|*x*x*|'') return 1 ;;
    *x*) ;;
    *) return 1 ;;
  esac
  _rea_w=${_rea_s%x*} _rea_h=${_rea_s#*x}
  # Leading zeros would read as octal elsewhere; 0 is no size at all.
  _rea_w=${_rea_w#"${_rea_w%%[!0]*}"} _rea_h=${_rea_h#"${_rea_h%%[!0]*}"}
  [ -n "$_rea_w" ] && [ -n "$_rea_h" ] || return 1
  [ "${#_rea_w}" -le 5 ] && [ "${#_rea_h}" -le 5 ] || return 1
  printf '%sx%s\n' "$_rea_w" "$_rea_h"
}

# The screen's size: REA_SCREEN, else the current mode xrandr reports.
rea_screen_size() {
  if [ -n "$REA_SCREEN" ]; then
    rea_parse_size "$REA_SCREEN"
  else
    rea_parse_size "$(xrandr --current 2>/dev/null | awk '/\*/ { print $1; exit }')"
  fi
}

# Sets REA_W and REA_H from the cfg's resolution: WIDTHxHEIGHT, a word in
# REA_ORIGINAL_WORDS (REA_ORIGINAL_SIZE), fullscreen/full/desktop (the
# screen, when REA_FULLSCREEN_SIZE=screen); anything else is
# REA_DEFAULT_SIZE.
rea_game_size() {
  _rea_r=$(rea_get_value "$REA_RES_KEY" | tr -d ' \t\r')
  _rea_size=
  rea_noglob
  # shellcheck disable=SC2086
  set -- $REA_ORIGINAL_WORDS
  rea_glob
  _rea_orig=
  for _rea_word in "$@"; do
    [ -n "$_rea_r" ] && [ "$_rea_r" = "$(printf '%s' "$_rea_word" | tr 'A-Z' 'a-z')" ] && _rea_orig=1
  done
  case $_rea_orig:$_rea_r in
    1:*) _rea_size=$(rea_parse_size "$REA_ORIGINAL_SIZE") ;;
    :fullscreen|:full|:desktop)
      [ "$REA_FULLSCREEN_SIZE" = screen ] && _rea_size=$(rea_screen_size) ;;
    *) _rea_size=$(rea_parse_size "$_rea_r") ;;
  esac
  [ -n "$_rea_size" ] || _rea_size=$(rea_parse_size "$REA_DEFAULT_SIZE") || _rea_size=1920x1080
  REA_W=${_rea_size%x*} REA_H=${_rea_size#*x}
}

# gamescope's -F filter from the cfg's scaler; unknown values (gamescope
# would refuse to start) are the default.
rea_scaler() {
  _rea_f=
  [ -n "$REA_SCALER_KEY" ] && _rea_f=$(rea_get_value "$REA_SCALER_KEY")
  case $_rea_f in
    linear|nearest|fsr|nis|pixel) ;;
    bilinear) _rea_f=linear ;;
    *) _rea_f=$REA_DEFAULT_SCALER ;;
  esac
  REA_F=$_rea_f
}

# --- GPU -------------------------------------------------------------------------

rea_has_nv_lib() {
  _rea_seam=$(rea_seam REA_NV_LIB_CHECK)
  if [ -n "$_rea_seam" ]; then
    $_rea_seam
  else
    rea_run sh -c 'ldconfig -p 2>/dev/null | grep -q libGLX_nvidia.so.0'
  fi
}

rea_nvidia_present() {
  [ -d "$1/module/nvidia" ] && return 0
  for _rea_v in "$1"/bus/pci/devices/*/vendor; do
    [ "$(cat "$_rea_v" 2>/dev/null)" = 0x10de ] && return 0
  done
  return 1
}

# The GPU for the game, as environment assignments (nothing: the default
# GPU). A choice the caller already made always wins.
# - REA_GPU_ICD_PIN=1 (SRR): with an NVIDIA device present and its Vulkan
#   ICD beside another vendor's, pin NVIDIA's ICD (VK_DRIVER_FILES and the
#   deprecated VK_ICD_FILENAMES) so DXVK cannot land on the iGPU. A leftover
#   NVIDIA json without an NVIDIA device is never pinned.
# - Otherwise NVIDIA with its driver and OpenGL library: render offload.
# - Several Mesa GPUs, the screen's one integrated (Intel, or less video
#   memory than another): DRI_PRIME=1.
rea_gpu_env() {
  _rea_sys=$(rea_seam REA_SYSFS); _rea_sys=${_rea_sys:-/sys}
  if [ "$REA_GPU_ICD_PIN" = 1 ]; then
    [ -n "${VK_DRIVER_FILES:-}${VK_ICD_FILENAMES:-}${DRI_PRIME:-}${__NV_PRIME_RENDER_OFFLOAD:-}" ] && return 0
    _rea_nv= _rea_others=
    _rea_dirs=$(rea_seam REA_ICD_DIRS)
    rea_noglob
    # shellcheck disable=SC2086
    set -- ${_rea_dirs:-/etc/vulkan/icd.d /usr/share/vulkan/icd.d}
    rea_glob
    for _rea_dir in "$@"; do
      for _rea_j in "$_rea_dir"/*.json; do
        [ -e "$_rea_j" ] || continue
        case "${_rea_j##*/}" in
          *nvidia*) _rea_nv=${_rea_nv:+$_rea_nv:}$_rea_j ;;
          *) _rea_others=1 ;;
        esac
      done
    done
    if [ -n "$_rea_nv" ] && [ -n "$_rea_others" ] && rea_nvidia_present "$_rea_sys"; then
      echo "VK_DRIVER_FILES=$_rea_nv VK_ICD_FILENAMES=$_rea_nv"
      return 0
    fi
  else
    [ -n "${DRI_PRIME:-}${__NV_PRIME_RENDER_OFFLOAD:-}" ] && return 0
  fi
  _rea_n=0 _rea_nvdrv= _rea_boot= _rea_boot_vram=0 _rea_other_vram=0
  for _rea_d in "$_rea_sys"/class/drm/card*; do
    case "${_rea_d##*/}" in *-*) continue ;; esac  # connectors (card0-DP-1)
    [ -r "$_rea_d/device/vendor" ] || continue
    _rea_n=$((_rea_n + 1))
    _rea_vram=$(cat "$_rea_d/device/mem_info_vram_total" 2>/dev/null) || _rea_vram=0
    case $_rea_vram in ''|*[!0-9]*) _rea_vram=0 ;; esac
    [ "$(basename "$(readlink "$_rea_d/device/driver" 2>/dev/null)" 2>/dev/null)" = nvidia ] && _rea_nvdrv=1
    if [ "$(cat "$_rea_d/device/boot_vga" 2>/dev/null)" = 1 ]; then
      _rea_boot=$(cat "$_rea_d/device/vendor"); _rea_boot_vram=$_rea_vram
    elif [ "$_rea_vram" -gt "$_rea_other_vram" ]; then
      _rea_other_vram=$_rea_vram
    fi
  done
  if [ "$REA_GPU_ICD_PIN" != 1 ] && [ -n "$_rea_nvdrv" ] && rea_has_nv_lib; then
    echo "__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only"
  elif [ "$_rea_n" -ge 2 ] && { [ "$_rea_boot" = 0x8086 ] || [ "$_rea_other_vram" -gt "$_rea_boot_vram" ]; }; then
    echo "DRI_PRIME=1"
  fi
  return 0
}

# REA_GPU once per run (a caller or test may preset it, even empty).
rea_gpu_init() {
  [ -n "${REA_GPU+set}" ] && return 0
  REA_GPU=$(rea_gpu_env)
  [ -n "$REA_GPU" ] && echo "GPU: $REA_GPU"
  return 0
}

# --- environment ---------------------------------------------------------------

# Prints the REA_SDL_ENV assignments, each NAME=DEFAULT replaced by the
# caller's own non-empty value. SDL_JOYSTICK_DEVICE is never set: SDL finds
# every pad itself and naming one adds it a second time.
rea_sdl_env() {
  rea_noglob
  # shellcheck disable=SC2086
  set -- $REA_SDL_ENV
  rea_glob
  for _rea_kv in "$@"; do
    _rea_k=${_rea_kv%%=*}
    case $_rea_k in
      ''|[0-9]*|*[!A-Za-z0-9_]*|SDL_JOYSTICK_DEVICE) continue ;;
    esac
    eval "_rea_cur=\${$_rea_k:-}"
    printf '%s=%s\n' "$_rea_k" "${_rea_cur:-${_rea_kv#*=}}"
  done
}

# The game's DLL overrides before the caller's (an AppImage's, a player's),
# so both apply.
rea_dll_overrides() {
  _rea_o=$REA_DLL_OVERRIDES
  [ "$REA_WINEDBG_OFF" = 1 ] && _rea_o=${_rea_o:+$_rea_o;}winedbg=d
  printf '%s\n' "$_rea_o${WINEDLLOVERRIDES:+${_rea_o:+;}$WINEDLLOVERRIDES}"
}

# rea_game_env NAME VALUE: an environment variable for the game only, set
# inside the inner script, so gamescope (itself a Vulkan program) never
# loads e.g. a vkBasalt layer. The value is data, never shell source.
rea_game_env() {
  case $1 in ''|[0-9]*|*[!A-Za-z0-9_]*) echo "rea_game_env: bad name '$1'" >&2; return 1 ;; esac
  case $2 in *'
'*) echo "rea_game_env: $1 holds a newline" >&2; return 1 ;; esac
  REA_GAME_ENV="$REA_GAME_ENV$1=$2
"
}

# --- Wine prefix -----------------------------------------------------------------

rea_wine() {
  rea_noglob
  # shellcheck disable=SC2086
  set -- WINEPREFIX="$REA_PREFIX" ${REA_GPU:-} wine "$@"
  rea_glob
  rea_run env "$@"
}
rea_wineserver() { rea_run env WINEPREFIX="$REA_PREFIX" wineserver "$@"; }
# A prefix's wineserver lingers a few seconds, tied to the display it
# started on; a game started meanwhile would join it outside gamescope.
rea_wineserver_wait() { rea_wineserver -w 2>/dev/null; return 0; }

# rea_once MARKER COMMAND...: runs COMMAND once per prefix (per state folder
# in native mode); MARKER is a file name made after it succeeds.
rea_once() {
  _rea_m=${REA_PREFIX:-$REA_STATE}/$1
  shift
  [ -f "$_rea_m" ] && return 0
  "$@" || return
  : > "$_rea_m"
}

# No crash dialog (winetricks' nocrashdialog), checked in user.reg, so it
# runs once; the prefix shuts down afterwards (see rea_wineserver_wait).
rea_wine_no_crash_dialog() {
  grep -qs '"ShowCrashDialog"=dword:00000000' "$REA_PREFIX/user.reg" && return 0
  rea_run env WINEPREFIX="$REA_PREFIX" wine reg add 'HKCU\Software\Wine\WineDbg' \
    /v ShowCrashDialog /t REG_DWORD /d 0 /f >/dev/null 2>&1
  rea_wineserver_wait
}

# Wine's controller bridge (winebus): each REA_WINEBUS_VALUES name set to 1
# only when absent (a player's value is never overwritten), under the names
# winebus.sys reads (DisableHidraw, no space). Once per prefix
# (REA_WINEBUS_MARKER), then the prefix shuts down so the bus re-reads them.
rea_seed_winebus() {
  rea_once "$REA_WINEBUS_MARKER" rea_seed_winebus_now
}
rea_seed_winebus_now() {
  echo "configuring Wine's controller bridge (once)"
  _rea_ifs=$IFS; IFS='|'
  rea_noglob
  # shellcheck disable=SC2086
  set -- $REA_WINEBUS_VALUES
  rea_glob
  IFS=$_rea_ifs
  for _rea_name in "$@"; do
    rea_wine reg query "$REA_WINEBUS_KEY" /v "$_rea_name" >/dev/null 2>&1 ||
      rea_wine reg add "$REA_WINEBUS_KEY" /v "$_rea_name" /t REG_DWORD /d 1 /f >/dev/null 2>&1
  done
  rea_wineserver_wait
}

# --- the game's own processes ----------------------------------------------------

# True while a process named in REA_GAME_PROCS is alive. Zombies do not
# count: a crashed session can leave one, which would block the wait loop
# for good. Shared verbatim with the inner script (self-contained).
REA_GAME_ALIVE_FN='rea_game_alive() {
  [ -n "${REA_GAME_PROCS:-}" ] || return 1
  _ga_ifs=$IFS; IFS="|"; set -f
  set -- $REA_GAME_PROCS
  set +f; IFS=$_ga_ifs
  for _ga_name in "$@"; do
    for _ga_pid in $(pgrep -x "$_ga_name"); do
      _ga_proc=/proc
      [ "${REA_TEST:-}" = 1 ] && _ga_proc=${REA_PROC:-/proc}
      _ga_state=$(sed "s/.*) //" "$_ga_proc/$_ga_pid/stat" 2>/dev/null | cut -d" " -f1)
      [ -n "$_ga_state" ] && [ "$_ga_state" != Z ] && return 0
    done
  done
  return 1
}'
eval "$REA_GAME_ALIVE_FN"

# --- the inner script ------------------------------------------------------------

# The script the session runs (inside gamescope when there is one). Its
# arguments are the launcher and the launcher's flags: none of them is ever
# part of its text, so a path with a quote, a space or $() cannot break or
# inject into it. Its settings arrive as environment variables:
#   REA_INNER_MODE      wine | native
#   REA_GAME_ENV_FILE   NAME=VALUE lines for the game only (data, not sourced)
#   REA_GAME_PROCS      the game's process names to wait for (Wine)
#   REA_GAME_SETTLE     seconds before that wait starts
#   REA_TEST            the library's own REA_TEST (empty outside tests)
rea_write_inner() {
  {
    echo '#!/bin/sh'
    echo '# Written by rea-play.sh; the launcher is "$1", its flags follow.'
    printf '%s\n' "$REA_GAME_ALIVE_FN"
    cat <<'INNER'
if [ -s "${REA_GAME_ENV_FILE:-}" ]; then
  while IFS= read -r _kv; do
    case "${_kv%%=*}" in ''|[0-9]*|*[!A-Za-z0-9_]*) continue ;; esac
    case "$_kv" in *=*) export "$_kv" ;; esac
  done < "$REA_GAME_ENV_FILE"
fi
if [ "${REA_INNER_MODE:-wine}" = native ]; then
  exec "$@"
fi
trap 'wineserver -k; exit' INT TERM HUP
wine "$@"
if [ -n "${REA_GAME_PROCS:-}" ]; then
  # The launcher can return before the game it started (an isolated Wine
  # desktop hands back early): wait for the game itself.
  sleep "${REA_GAME_SETTLE:-3}"
  while rea_game_alive; do sleep 1; done
fi
wineserver -k
INNER
  } > "$REA_INNER"
  chmod +x "$REA_INNER"
}

# --- session supervision ---------------------------------------------------------

# Session id and start time of a pid, from /proc. Fields are read after the
# last ") ", so a command name with spaces or parentheses cannot shift them.
rea_proc_stat() {
  [ -r "/proc/$1/stat" ] || return 1
  _rea_stat=$(cat "/proc/$1/stat" 2>/dev/null) || return 1
  _rea_rest=${_rea_stat##*) }
  rea_noglob
  # shellcheck disable=SC2086
  set -- $_rea_rest
  rea_glob
  [ "$#" -ge 20 ] || return 1
  printf '%s %s\n' "$4" "${20}"
}

rea_read_lock() {
  [ -r "$REA_LOCK" ] || return 1
  IFS=' ' read -r REA_LOCK_PID REA_LOCK_START < "$REA_LOCK" || return 1
  case "$REA_LOCK_PID" in ''|*[!0-9]*) return 1 ;; esac
  case "$REA_LOCK_START" in ''|*[!0-9]*) return 1 ;; esac
}

# A lock alone proves nothing: its pid must still lead the session we
# started, with the start time recorded then. A stale lock whose pid was
# reused, or the game started some other way, never counts.
rea_session_valid() {
  rea_read_lock || return 1
  _rea_st=$(rea_proc_stat "$REA_LOCK_PID") || return 1
  [ "$_rea_st" = "$REA_LOCK_PID $REA_LOCK_START" ]
}

# A pid-only lock from an older script: a live pid counts as running (an
# upgrade mid-game must not start a second session). REA_LEGACY_LOCK=off
# ignores such locks.
rea_legacy_lock() {
  [ "$REA_LEGACY_LOCK" != off ] || return 1
  [ -r "$REA_LOCK" ] || return 1
  IFS=' ' read -r _rea_old _rea_rest < "$REA_LOCK" || return 1
  [ -z "$_rea_rest" ] || return 1
  case "$_rea_old" in ''|*[!0-9]*) return 1 ;; esac
  kill -0 "$_rea_old" 2>/dev/null || return 1
  REA_LEGACY_PID=$_rea_old
}

rea_running() { rea_session_valid || rea_legacy_lock; }

rea_waits() { [ "$REA_ALWAYS_WAIT" = 1 ] || [ -n "$REA_WAIT" ]; }

# Interrupted while waiting (the AppImage closing, Ctrl-C): end the session
# and wait for it, so the AppImage stays mounted until its Wine is gone and
# a restart never finds the old game.
rea_launch_interrupted() {
  rea_restore_traps
  if rea_session_valid; then
    kill -TERM "-$REA_LOCK_PID" 2>/dev/null || true
  fi
  # Under a wrapper (distrobox) the game may live outside the session.
  if [ "$REA_MODE" = wine ] && [ "$REA_INTERRUPT_WINESERVER" = 1 ]; then
    rea_wineserver -k 2>/dev/null
  fi
  wait "$REA_PID" 2>/dev/null
  rm -f "$REA_LOCK"
  exit 143   # the caller's own EXIT trap, restored above, runs now
}

# While a launch waits, its EXIT/HUP/INT/TERM traps replace the caller's;
# the caller's are saved first and put back afterwards (also before an
# interrupted launch exits, so the caller's EXIT trap still runs). `trap`
# goes through a file: dash reports nothing for $(trap). Only the top-level
# shell restores: inside a ( subshell ) bash reports the parent's traps,
# which are not active there, and re-arming them (a cleanup `rm -rf`, say)
# would be wrong; a subshell gets the defaults back, as before.
rea_save_traps() {
  _rea_saved_traps=
  sh -c 'echo "$PPID"' > "$REA_STATE/traps.saved" 2>/dev/null
  if [ "$(cat "$REA_STATE/traps.saved" 2>/dev/null)" = "$$" ]; then
    trap > "$REA_STATE/traps.saved"
    _rea_saved_traps=$(cat "$REA_STATE/traps.saved")
  fi
  rm -f "$REA_STATE/traps.saved"
}
rea_restore_traps() {
  trap - EXIT HUP INT TERM
  eval "${_rea_saved_traps:-}"
}

# rea_launch [NAME=VALUE...] COMMAND...: runs the command in its own
# session (so stop addresses exactly this launch) through REA_RUN, with the
# prefix and GPU choice, output in play.log. The lock records the session
# leader's pid and start time. It waits (REA_ALWAYS_WAIT=1 or REA_WAIT) and
# then removes the lock; otherwise it returns at once, and a stale lock is
# recognised by its start time.
rea_launch() {
  _rea_seam=$(rea_seam REA_LAUNCH_CMD)
  if [ -n "$_rea_seam" ]; then
    $_rea_seam "$@"
    return
  fi
  rea_noglob
  # shellcheck disable=SC2086
  if [ "$REA_MODE" = wine ]; then
    set -- $REA_RUN env WINEPREFIX="$REA_PREFIX" ${REA_GPU:-} "$@"
  else
    set -- $REA_RUN env ${REA_GPU:-} "$@"
  fi
  rea_glob
  setsid "$@" > "$REA_STATE/play.log" 2>&1 < /dev/null &
  REA_PID=$!
  # No record possible (it already ended, or no /proc): still a started
  # game, not a failure.
  _rea_st=$(rea_proc_stat "$REA_PID") || {
    if rea_waits; then wait "$REA_PID"; return; fi
    return 0
  }
  printf '%s %s\n' "$REA_PID" "${_rea_st#* }" > "$REA_LOCK"
  rea_waits || return 0
  rea_save_traps
  trap 'rm -f "$REA_LOCK"' EXIT
  trap 'rea_launch_interrupted' HUP INT TERM
  wait "$REA_PID"
  _rea_rc=$?
  rm -f "$REA_LOCK"
  rea_restore_traps
  return "$_rea_rc"
}

# Ends the session we started (revalidated right before the signal: a stale
# or reused pid is never a target) and, in Wine mode, whatever is left in
# our private prefix. A pid-only lock's pid is signalled as the older script
# did (REA_LEGACY_LOCK=signal), or left alone (keep).
rea_stop() {
  if rea_session_valid; then
    kill -TERM "-$REA_LOCK_PID" 2>/dev/null || true
  elif [ "$REA_LEGACY_LOCK" = signal ] && rea_legacy_lock; then
    kill -TERM "$REA_LEGACY_PID" 2>/dev/null || true
  fi
  rm -f "$REA_LOCK"
  [ "$REA_MODE" = wine ] && rea_wineserver -k 2>/dev/null
  return 0
}

rea_status() { if rea_running; then echo running; else echo idle; fi; }

# --- gamescope -------------------------------------------------------------------

rea_has_gamescope() {
  _rea_seam=$(rea_seam REA_GAMESCOPE_CHECK)
  if [ -n "$_rea_seam" ]; then
    $_rea_seam
  else
    rea_run sh -c 'command -v gamescope' >/dev/null 2>&1
  fi
}

# The game's window mode asks for no gamescope (REA_WINDOWED_KEY set and its
# value one of REA_WINDOWED_VALUES): windowed games size themselves.
rea_windowed() {
  [ -n "$REA_WINDOWED_KEY" ] || return 1
  _rea_mode=$(rea_get_value "$REA_WINDOWED_KEY")
  [ -n "$_rea_mode" ] || return 1
  rea_noglob
  # shellcheck disable=SC2086
  set -- $(printf '%s' "$REA_WINDOWED_VALUES" | tr 'A-Z' 'a-z')
  rea_glob
  for _rea_v in "$@"; do
    [ "$_rea_mode" = "$_rea_v" ] && return 0
  done
  return 1
}

# A gamescope of our own: never inside another (a Steam Deck's Game Mode;
# a nested one fails), never when the game opted out or is windowed, and
# only when one is installed.
rea_use_gamescope() {
  [ "$REA_GAMESCOPE" != off ] || return 1
  [ -z "${GAMESCOPE_WAYLAND_DISPLAY:-}" ] || return 1
  rea_windowed && return 1
  rea_has_gamescope
}

# --- the two steps ---------------------------------------------------------------

# The settings window; its exit code says what to do next: 0 Play, 4 no
# window could open (play with the saved settings), 3 closed, else failed.
# stderr goes to settings.log.
rea_settings() {
  rea_gpu_init
  rea_hook_before_settings
  # `--flag=value` reaches the launcher as `--flag value`, the spelling
  # every launcher's parser reads.
  _rea_n=$#
  while [ "$_rea_n" -gt 0 ]; do
    _rea_a=$1; shift; _rea_n=$((_rea_n - 1))
    case $_rea_a in
      --?*=*) set -- "$@" "${_rea_a%%=*}" "${_rea_a#*=}" ;;
      *) set -- "$@" "$_rea_a" ;;
    esac
  done
  _rea_sdl=
  [ "$REA_SDL_SETTINGS" = 1 ] && _rea_sdl=$(rea_sdl_env)
  rea_noglob
  # shellcheck disable=SC2086
  if [ "$REA_MODE" = wine ]; then
    set -- WINEPREFIX="$REA_PREFIX" ${REA_GPU:-} wine "$REA_EXE" $REA_SETTINGS_ARGS "$@"
  else
    set -- ${REA_GPU:-} "$REA_EXE" $REA_SETTINGS_ARGS "$@"
  fi
  _rea_ifs=$IFS; IFS='
'
  # shellcheck disable=SC2086
  set -- $_rea_sdl "$@"
  IFS=$_rea_ifs
  rea_glob
  rea_run env "$@" 2>"$REA_STATE/settings.log"
  _rea_rc=$?
  [ "$REA_MODE" = wine ] && rea_wineserver_wait
  case $_rea_rc in
    0) return 0 ;;
    4) echo "no settings window could open; playing with the saved settings"; return 0 ;;
    3) echo "settings window closed without Play; nothing started" ;;
    *) echo "settings window failed (exit $_rea_rc); see $REA_STATE/settings.log" ;;
  esac
  return 1
}

# Starts the game with the saved settings: prefix prep, the inner script,
# then the supervised session, in gamescope when that applies. Extra
# arguments go to the launcher after REA_PLAY_ARGS.
rea_start() {
  rea_require_exe || exit 1
  rea_one_run "$@"
  # `--flag=value` reaches the launcher as `--flag value`, the spelling
  # every launcher's parser reads.
  _rea_n=$#
  while [ "$_rea_n" -gt 0 ]; do
    _rea_a=$1; shift; _rea_n=$((_rea_n - 1))
    case $_rea_a in
      --?*=*) set -- "$@" "${_rea_a%%=*}" "${_rea_a#*=}" ;;
      *) set -- "$@" "$_rea_a" ;;
    esac
  done
  rea_gpu_init
  [ -f "$REA_CFG" ] || rea_find_cfg   # the settings window may have made it
  REA_GAME_ENV=
  rea_hook_before_start
  rea_write_inner
  printf '%s' "$REA_GAME_ENV" > "$REA_GAME_ENV_FILE"
  rea_noglob
  # shellcheck disable=SC2086
  set -- sh "$REA_INNER" "$REA_EXE" $REA_PLAY_ARGS "$@"
  rea_glob
  if rea_use_gamescope; then
    rea_game_size
    rea_scaler
    rea_noglob
    # shellcheck disable=SC2086
    if [ "$REA_GAMESCOPE_OUTPUT" = 1 ]; then
      set -- gamescope -W "$REA_W" -H "$REA_H" -w "$REA_W" -h "$REA_H" -f -S fit -F "$REA_F" $REA_GAMESCOPE_EXTRA -- "$@"
    else
      set -- gamescope -w "$REA_W" -h "$REA_H" -f -S fit -F "$REA_F" $REA_GAMESCOPE_EXTRA -- "$@"
    fi
    rea_glob
    echo "starting at ${REA_W}x${REA_H}, scaler $REA_F"
  else
    echo "$REA_DIRECT_NOTE"
  fi
  set -- REA_INNER_MODE="$REA_MODE" REA_GAME_ENV_FILE="$REA_GAME_ENV_FILE" \
    REA_GAME_PROCS="$REA_GAME_PROCS" REA_GAME_SETTLE="$REA_GAME_SETTLE" \
    REA_TEST="${REA_TEST:-}" "$@"
  if [ "$REA_MODE" = wine ]; then
    _rea_o=$(rea_dll_overrides)
    [ -n "$_rea_o" ] && set -- WINEDLLOVERRIDES="$_rea_o" "$@"
  fi
  _rea_sdl=$(rea_sdl_env)
  _rea_ifs=$IFS; IFS='
'
  rea_noglob
  # shellcheck disable=SC2086
  set -- $_rea_sdl "$@"
  rea_glob
  IFS=$_rea_ifs
  rea_launch "$@"
  _rea_rc=$?
  REA_ONE_RUN=
  return "$_rea_rc"
}

# --- commands --------------------------------------------------------------------

rea_usage() {
  echo "usage: $REA_NAME [--settings|--play|stop|status|--help]"
  echo "  (none), --settings  show the settings window, then play"
  echo "  --play, play, start play with the saved settings (no window)"
  echo "  stop                end the game session $REA_NAME started"
  echo "  status              running or idle"
  rea_hook_usage
}

# Settings window, then the game (arguments go to the window).
rea_cmd_default() {
  if rea_running; then echo "already running"; exit 0; fi
  rea_require_exe || exit 1
  rea_settings "$@" || exit 0
  rea_start
}

rea_cmd_play() {
  if rea_running; then echo "already running"; return 0; fi
  rea_start "$@"
}

# The game script's rea_hook_command sees every non-empty command line first
# (127 = not its own), so a game can keep its own meaning for any word, the
# built-in aliases included.
rea_main() {
  [ -n "${REA_INITED:-}" ] || rea_init || exit 1
  if [ "$#" -gt 0 ]; then
    rea_hook_command "$@"
    _rea_rc=$?
    [ "$_rea_rc" = 127 ] || return "$_rea_rc"
  fi
  case "${1:-}" in
    ""|--settings|settings)
      [ "$#" -gt 0 ] && shift
      rea_cmd_default "$@" ;;
    --play|play|start)
      shift
      rea_cmd_play "$@" ;;
    status|--status) rea_status ;;
    stop|--stop)
      rea_stop
      echo stopped ;;
    -h|--help|help) rea_usage ;;
    *) rea_usage >&2; exit 2 ;;
  esac
}
