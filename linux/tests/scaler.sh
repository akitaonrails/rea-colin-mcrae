#!/bin/sh
# Tests that cmr-play hands launcher.cfg's scaler to gamescope (-F), for
# every scaler, and the race size (-w/-h); one-run flags win for that run
# only: sh linux/tests/scaler.sh
set -u
unset GAMESCOPE_WAYLAND_DISPLAY DRI_PRIME __NV_PRIME_RENDER_OFFLOAD WINEDLLOVERRIDES
HERE=$(dirname "$(readlink -f "$0")")
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
export HOME=$T CMR_PREFIX=$T/prefix CMR_WAIT=1 CMR_LAUNCHER=$T/CMRLauncher.exe
CFGDIR=$CMR_PREFIX/drive_c/users/me/AppData/Local/CMRLauncher
mkdir -p "$CFGDIR"; : > "$T/CMRLauncher.exe"
# The wrapper (a program: the launch runs it through setsid): "gamescope
# is installed", and records the command it runs.
cat > "$T/fake-run" <<FAKE
#!/bin/sh
case "\$*" in *"command -v gamescope"*) exit 0 ;; *wineserver*|*"reg add"*) exit 0 ;; esac
echo "\$*" > "$T/ran"
FAKE
chmod +x "$T/fake-run"
REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../cmr-play"
REA_RUN=$T/fake-run REA_GPU=
CFG=$CFGDIR/launcher.cfg
REA_CFG=$CFG
fails=0
ran() { cat "$T/ran"; }
expect() { # name pattern
  case "$(ran)" in $2) echo "ok   $1" ;; *) echo "FAIL $1: $(ran)"; fails=$((fails + 1)) ;; esac
}
for scaler in nearest pixel fsr linear; do
  printf 'resolution = 1920x1080\nscaler = %s\n' "$scaler" > "$CFG"
  rea_start > /dev/null
  expect "$scaler" "*gamescope -w 1920 -h 1080 -f -S fit -F $scaler --*"
done
printf 'resolution = 1920x1080\n' > "$CFG"
rea_start > /dev/null
expect "default is linear" '*-F linear --*'
# The game's own settings reach its session.
for want in "REA_GAME_PROCS=Rally.exe" "WINEPREFIX=$CMR_PREFIX" "$CMR_LAUNCHER --no-window"; do
  case "$(ran)" in *"$want"*) echo "ok   session has $want" ;; *) echo "FAIL no $want: $(ran)"; fails=$((fails + 1)) ;; esac
done
case "$(ran)" in *SDL_*|*WINEDLLOVERRIDES*) echo "FAIL the game got settings it never had: $(ran)"; fails=$((fails + 1)) ;; *) echo "ok   no SDL or DLL overrides" ;; esac
# Unset, original and off: the game's own 640x480.
for r in "" "resolution = original" "resolution = off" "resolution = Original"; do
  printf '%s\n' "$r" > "$CFG"
  rea_start > /dev/null
  expect "'$r' is 640x480" '*gamescope -w 640 -h 480 -f -S fit -F linear --*'
done
printf 'resolution = 2560X1440\r\n' > "$CFG"
rea_start > /dev/null
expect "2560X1440 with CRLF" '*-w 2560 -h 1440 -f*'
printf 'resolution = $(touch pwned)x1\n' > "$CFG"
rea_start > /dev/null
expect "a hostile value falls back to 640x480" '*-w 640 -h 480 -f*'
[ -e pwned ] && { echo "FAIL a value ran"; fails=$((fails + 1)); }
# Flags for one run beat launcher.cfg, reach the launcher too, and leave
# the file as it was.
printf 'resolution = 1920x1080\nscaler = linear\n' > "$CFG"
before=$(cat "$CFG")
rea_main --play --scaler fsr --resolution 1280x960 > /dev/null
expect "one-run flags override launcher.cfg" \
  '*gamescope -w 1280 -h 960 -f -S fit -F fsr --*--no-window --scaler fsr --resolution 1280x960*'
[ "$(cat "$CFG")" = "$before" ] && echo "ok   launcher.cfg untouched" || { echo "FAIL cfg changed"; fails=$((fails + 1)); }
rea_main --play > /dev/null
expect "a plain --play reads launcher.cfg again" '*-w 1920 -h 1080 -f -S fit -F linear --*'
rea_main --play --scaler=nearest > /dev/null
expect "--scaler=nearest for one run" '*-F nearest --*'
# A look without a resolution sizes gamescope as the launcher sizes the
# race (form::with_look): Steam Deck 1280x800, Enhanced the screen (on its
# long side first), Original the game's 640x480.
printf 'look = steam_deck\n' > "$CFG"
rea_start > /dev/null
expect "file look steam_deck: 1280x800" '*gamescope -w 1280 -h 800 -f*'
printf 'look = Enhanced\n' > "$CFG"
REA_SCREEN=2560x1440 rea_start > /dev/null
expect "file look enhanced: the screen" '*gamescope -w 2560 -h 1440 -f*'
REA_SCREEN=2160x3840 rea_start > /dev/null
expect "a portrait screen gives a landscape size" '*gamescope -w 3840 -h 2160 -f*'
printf 'look = original\n' > "$CFG"
rea_start > /dev/null
expect "file look original: 640x480" '*gamescope -w 640 -h 480 -f*'
printf 'look = steam_deck\nresolution = 1920x1080\n' > "$CFG"
rea_start > /dev/null
expect "the file's own resolution wins over its look" '*gamescope -w 1920 -h 1080 -f*'
rea_main --play --look steam_deck > /dev/null
expect "--look wins over the file's resolution, and reaches the launcher" \
  '*gamescope -w 1280 -h 800 -f*--no-window --look steam_deck*'
REA_SCREEN=1600x2560 rea_main --play --look enhanced > /dev/null
expect "--look enhanced on a portrait screen: landscape" '*gamescope -w 2560 -h 1600 -f*--look enhanced*'
rea_main --play --look=original > /dev/null
expect "--look=original" '*gamescope -w 640 -h 480 -f*'
rea_main --play --look steam_deck --resolution 2560x1440 > /dev/null
expect "--resolution wins over --look" '*gamescope -w 2560 -h 1440 -f*'
printf 'resolution = 1920x1080\n' > "$CFG"
rea_main --play --look bogus > /dev/null
expect "an unknown --look leaves the file's size" '*gamescope -w 1920 -h 1080 -f*'
# Inside gamescope already (Steam Deck Game Mode): no second one.
GAMESCOPE_WAYLAND_DISPLAY=gamescope-0 rea_start > /dev/null
case "$(ran)" in *gamescope\ -w*) echo "FAIL nested gamescope"; fails=$((fails + 1)) ;; *inner.sh*) echo "ok   no nested gamescope" ;; *) echo "FAIL nested: $(ran)"; fails=$((fails + 1)) ;; esac
[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
