#!/bin/sh
# Tests cmr-play's own-session lock, stop, the game's script and the
# settings exit codes, as the play library runs them with the game's
# settings; only harmless sleeps and fake programs run:
# sh linux/tests/supervision.sh
set -u
unset VK_INSTANCE_LAYERS VK_LAYER_PATH CMR_WAIT GAMESCOPE_WAYLAND_DISPLAY WINEDLLOVERRIDES
HERE=$(dirname "$(readlink -f "$0")")
T=$(mktemp -d)
export T HOME=$T CMR_PREFIX=$T/prefix
STALE_PID= STALE_START= OTHER_PID= OTHER_START=
stop_helper() {
  [ -n "$1" ] || return 0
  stats=$(rea_proc_stat "$1") || return 0
  if [ "${stats#* }" = "$2" ]; then kill "$1" 2>/dev/null || true; fi
  wait "$1" 2>/dev/null || true
}
cleanup() {
  stop_helper "$STALE_PID" "$STALE_START"
  stop_helper "$OTHER_PID" "$OTHER_START"
  rm -rf "$T"
}
trap cleanup EXIT HUP INT TERM
mkdir -p "$CMR_PREFIX/drive_c/users/me/AppData/Local/CMRLauncher"
REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../cmr-play"
LOCK=$REA_LOCK STATE=$REA_STATE
fails=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; fails=$((fails + 1)); }

# The wrapper records the commands it would run (wineserver -k included).
cat > "$T/fake-run" <<'FAKE'
#!/bin/sh
case "$*" in *"command -v gamescope"*) exit 0 ;; *"reg add"*|*"wineserver -w"*) exit 0 ;; esac
case "$*" in *"wineserver -k"*) echo "$*" >> "$T/wineserver" ; exit 0 ;; esac
echo "$*" > "$T/ran"
printf '<%s>\n' "$@" > "$T/args"
FAKE
chmod +x "$T/fake-run"
REA_RUN=$T/fake-run REA_GPU=

# A live pid with the wrong start time stands for a stale, reused lock.
sleep 30 & STALE_PID=$!
stats=$(rea_proc_stat "$STALE_PID"); STALE_START=${stats#* }
printf '%s %s\n' "$STALE_PID" "$((STALE_START + 1))" > "$LOCK"
if rea_running; then fail "stale lock reported running"; else ok "stale/reused pid refused"; fi
rea_stop
if kill -0 "$STALE_PID" 2>/dev/null; then ok "stop did not signal a reused pid"; else fail "stale helper was signalled"; fi
if grep -q "WINEPREFIX=$CMR_PREFIX wineserver -k" "$T/wineserver"; then ok "stop ends the prefix's wineserver"; else fail "stop: no wineserver -k"; fi
stop_helper "$STALE_PID" "$STALE_START"; STALE_PID= STALE_START=

# A Rally.exe of someone else's is not our launch.
ln -s "$(command -v sleep)" "$T/Rally.exe"
"$T/Rally.exe" 30 & OTHER_PID=$!
stats=$(rea_proc_stat "$OTHER_PID"); OTHER_START=${stats#* }
rm -f "$LOCK"
if rea_running; then fail "unrelated Rally.exe blocked the launch"; else ok "unrelated Rally.exe does not block"; fi
rea_stop
if kill -0 "$OTHER_PID" 2>/dev/null; then ok "unrelated Rally.exe not signalled"; else fail "unrelated helper was signalled"; fi
stop_helper "$OTHER_PID" "$OTHER_START"; OTHER_PID= OTHER_START=

REA_CFG=$CMR_PREFIX/drive_c/users/me/AppData/Local/CMRLauncher/launcher.cfg
printf 'resolution = 1920x1080\n' > "$REA_CFG"
REA_EXE=$T/CMRLauncher.exe; : > "$REA_EXE"

# Background launch (no CMR_WAIT): it returns at once, and the lock
# names our own session until it ends.
cat > "$T/slow-run" <<'FAKE'
#!/bin/sh
case "$*" in *inner.sh*) exec sleep 30 ;; esac
exit 0
FAKE
chmod +x "$T/slow-run"
REA_RUN=$T/slow-run
rea_start > /dev/null
i=0; while ! rea_running && [ "$i" -lt 20 ]; do sleep 0.05; i=$((i + 1)); done
if rea_running; then ok "a background launch is running"; else fail "background launch not running"; fi
rea_read_lock; SESSION=$REA_LOCK_PID
# A second Play while the game runs starts nothing (one instance).
rm -f "$T/ran"
out=$(REA_RUN=$T/fake-run rea_main --play)
if [ "$out" = "already running" ] && [ ! -f "$T/ran" ]; then ok "a second --play while running starts nothing"; else fail "second --play: $out $(cat "$T/ran" 2>/dev/null)"; fi
rea_stop
sleep 0.2
if kill -0 "$SESSION" 2>/dev/null; then fail "stop left the session"; else ok "stop ends our session"; fi
REA_RUN=$T/fake-run

REA_WAIT=1 rea_start > /dev/null
if [ -f "$LOCK" ]; then fail "a finished launch left its lock"; else ok "a finished launch removes its lock"; fi

# Spaces and an apostrophe stay one argument, never script text.
mkdir -p "$T/launcher it's here"
REA_EXE=$T/"launcher it's here"/CMRLauncher.exe
: > "$REA_EXE"
REA_WAIT=1 rea_start > /dev/null
if grep -qx "<$REA_EXE>" "$T/args" && [ "$(tail -1 "$T/args")" = "<--no-window>" ]; then
  ok "apostrophe+space launcher is one argument"
else
  fail "launcher argument: $(tr '\n' ' ' < "$T/args")"
fi
case "$(cat "$STATE/inner.sh")" in
  *"$REA_EXE"*) fail "launcher path in the generated script" ;;
  *'wine "$@"'*) ok "the game's script takes the launcher as an argument" ;;
  *) fail "inner.sh does not run wine \"\$@\"" ;;
esac

# The game's script runs the launcher through wine, then waits for
# Rally.exe (the launcher returns once the game is up) and ends the
# prefix's wineserver.
mkdir -p "$T/bin"
printf '#!/bin/sh\necho "wine $*" >> "$T/inner-calls"\n' > "$T/bin/wine"
printf '#!/bin/sh\necho "pgrep $*" >> "$T/inner-calls"\nexit 1\n' > "$T/bin/pgrep"
printf '#!/bin/sh\necho "wineserver $*" >> "$T/inner-calls"\n' > "$T/bin/wineserver"
printf '#!/bin/sh\nexit 0\n' > "$T/bin/sleep"
chmod +x "$T/bin/"*
: > "$T/game.env"
PATH=$T/bin:$PATH REA_GAME_ENV_FILE=$T/game.env REA_GAME_PROCS=Rally.exe \
  sh "$STATE/inner.sh" "$REA_EXE" --no-window
calls=$(tr '\n' ';' < "$T/inner-calls")
case "$calls" in
  "wine $REA_EXE --no-window;"*"Rally.exe"*"wineserver -k;") ok "the game's script runs the launcher, waits for Rally.exe, ends Wine" ;;
  *) fail "inner calls: $calls" ;;
esac

# The settings window: Play (0) and no window (4) go on to the game;
# closing it (3) or a failure does not.
cat > "$T/fake-run" <<'FAKE'
#!/bin/sh
case "$*" in *"--settings --linux"*) echo "$*" > "$T/settings"; exit "$(cat "$T/rc")" ;; esac
exit 0
FAKE
for case in 0:0 4:0 3:1 1:1; do
  echo "${case%:*}" > "$T/rc"
  if rea_settings > /dev/null; then got=0; else got=1; fi
  if [ "$got" = "${case#*:}" ]; then ok "settings exit ${case%:*} -> $got"; else fail "settings exit ${case%:*} gave $got"; fi
done
case "$(cat "$T/settings")" in
  *SDL_*) fail "the window got the game's SDL settings" ;;
  *"wine $REA_EXE --settings --linux"*) ok "the window is the launcher's own" ;;
  *) fail "settings command: $(cat "$T/settings")" ;;
esac

# A lock from the previous cmr-play (pid only) with a live pid counts as
# running; stop ends that pid as the old script did.
sleep 30 & OLD=$!
echo "$OLD" > "$LOCK"
if rea_running; then ok "an old-format lock with a live pid is running"; else fail "old-format lock ignored"; fi
rea_stop
wait "$OLD" 2>/dev/null
if kill -0 "$OLD" 2>/dev/null; then fail "stop left the old-format session"; kill "$OLD"; else ok "stop ends an old-format session"; fi
echo 999999999 > "$LOCK"
if rea_running; then fail "a dead old-format pid counts as running"; else ok "a dead old-format pid does not"; fi
rm -f "$LOCK"

# Interrupted while waiting (the AppImage closed): the session is ended
# and waited for before cmr-play exits.
cat > "$T/slow-run" <<'FAKE'
#!/bin/sh
case "$*" in *inner.sh*) ;; *) exit 0 ;; esac
trap 'sleep 0.3; touch "$T/ended"; exit 0' TERM
sleep 30 & wait
FAKE
REA_RUN=$T/slow-run
( REA_WAIT=1 rea_start > /dev/null ) & WAITER=$!
i=0; while ! rea_running && [ "$i" -lt 40 ]; do sleep 0.05; i=$((i + 1)); done
kill -TERM "$WAITER"
wait "$WAITER"
if [ -f "$T/ended" ]; then ok "an interrupted launch waits for its session"; else fail "interrupted launch exited before its session"; fi
if [ -f "$LOCK" ]; then fail "an interrupted launch left its lock"; else ok "an interrupted launch removes its lock"; fi
REA_RUN=$T/fake-run

# A started game whose /proc record cannot be read is not a failure.
if ( rea_proc_stat() { return 1; }; rea_start > /dev/null ); then ok "launch without a record still succeeds"; else fail "launch without a record failed"; fi

# The command line, end to end through the script itself.
unset REA_FUNCTIONS_ONLY REA_PLAY_HERE
PLAYDIR=$T/kit/linux; mkdir -p "$PLAYDIR" "$T/kit/sdk"
cp "$HERE/../cmr-play" "$PLAYDIR/"; cp "$HERE/../../sdk/rea-play.sh" "$T/kit/sdk/"
: > "$T/kit/CMRLauncher.exe"
if [ "$(sh "$PLAYDIR/cmr-play" status)" = idle ]; then ok "status says idle"; else fail "status"; fi
if [ "$(CMR_RUN=$T/fake-run sh "$PLAYDIR/cmr-play" stop)" = stopped ]; then ok "stop says stopped"; else fail "stop"; fi
sh "$PLAYDIR/cmr-play" bogus > /dev/null 2>&1
if [ $? = 2 ]; then ok "an unknown command exits 2"; else fail "unknown command"; fi
rm -f "$T/kit/CMRLauncher.exe"
out=$(CMR_RUN=$T/fake-run sh "$PLAYDIR/cmr-play" --play 2>&1); rc=$?
case "$rc:$out" in 1:*"CMRLauncher.exe not found next to cmr-play (or set CMR_LAUNCHER)"*) ok "a missing launcher is named" ;; *) fail "missing launcher: $rc $out" ;; esac

# rea_launch reset the EXIT trap of the shell it ran in.
trap cleanup EXIT HUP INT TERM
[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
