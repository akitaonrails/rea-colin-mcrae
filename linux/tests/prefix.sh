#!/bin/sh
# Tests the Wine settings cmr-play gives the game: sh linux/tests/prefix.sh
set -u
HERE=$(dirname "$(readlink -f "$0")")
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
export HOME=$T CMR_PREFIX=$T/prefix
REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../cmr-play"
fails=0
check() {
  if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: '$2', want '$3'"; fails=$((fails + 1)); fi
}

# The game needs no DLL overrides of its own (the hook library is loaded
# in memory); the AppImage's and a player's pass through unchanged.
unset WINEDLLOVERRIDES
check "no overrides of our own" "$(rea_dll_overrides)" ""
WINEDLLOVERRIDES="mscoree,mshtml="
check "the AppImage's overrides kept" "$(rea_dll_overrides)" "mscoree,mshtml="
unset WINEDLLOVERRIDES

check "the prefix is CMR_PREFIX" "$REA_PREFIX" "$T/prefix"
check "the state folder" "$REA_STATE" "$T/.config/cmr-launcher"
check "the game's process" "$REA_GAME_PROCS" "Rally.exe"
# Without CMR_PREFIX: the prefix cmr-play always used.
out=$(unset CMR_PREFIX; REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../cmr-play"; echo "$REA_PREFIX")
check "the default prefix" "$out" "$T/.local/share/cmr-launcher/prefix"
# launcher.cfg in the prefix user's AppData\Local\CMRLauncher.
mkdir -p "$T/p2/drive_c/users/me/AppData/Local/CMRLauncher" "$T/p2/drive_c/users/Public/AppData/Local/CMRLauncher"
out=$(CMR_PREFIX=$T/p2 REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../cmr-play"; echo "$REA_CFG")
check "launcher.cfg in the prefix" "$out" "$T/p2/drive_c/users/me/AppData/Local/CMRLauncher/launcher.cfg"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
