#!/bin/sh
# rea-launcher-sdk packaging/check-lld.sh: checks that a launcher links its
# Windows targets with lld. GNU ld 2.47 leaves import slots outside every
# import descriptor in i686 and x86_64 builds (the program crashes on that
# call, often before main; packaging/check-imports.sh finds it in a built
# file), so `-C link-arg=-fuse-ld=lld` is the rule for every *-windows-gnu
# target.
#
#   check-lld.sh [--config FILE] TARGET...
#
# Looks in [target.TARGET] of FILE (default .cargo/config.toml) for a
# rustflags line with -fuse-ld=lld; CARGO_TARGET_<TARGET>_RUSTFLAGS in the
# environment also counts. Non-Windows targets pass. Exits 1 on a miss.
set -eu
CONFIG=.cargo/config.toml
if [ "${1:-}" = --config ]; then
  [ $# -ge 2 ] || { echo "usage: $0 [--config FILE] TARGET..." >&2; exit 2; }
  CONFIG=$2; shift 2
fi
[ $# -ge 1 ] || { echo "usage: $0 [--config FILE] TARGET..." >&2; exit 2; }

status=0
for target; do
  case $target in *-windows-gnu*) ;; *) echo "ok   $target: not a Windows GNU target"; continue ;; esac
  case $target in *[!A-Za-z0-9_.-]*) echo "check-lld: odd target '$target'" >&2; exit 2 ;; esac
  var=CARGO_TARGET_$(printf '%s' "$target" | tr 'a-z.-' 'A-Z__')_RUSTFLAGS
  flags=''
  eval "flags=\${$var:-}"
  case $flags in *fuse-ld=lld*) echo "ok   $target: lld via $var"; continue ;; esac
  if [ -f "$CONFIG" ] && awk -v t="$target" '
      /^[ \t]*\[/ {
        h = $0; gsub(/[ \t"'\'']/, "", h)
        in_t = (h == "[target." t "]")
        next
      }
      in_t && /^[ \t]*rustflags[ \t]*=/ && /fuse-ld=lld/ { found = 1 }
      END { exit !found }' "$CONFIG"; then
    echo "ok   $target: lld in $CONFIG"
  else
    echo "FAIL $target: no -fuse-ld=lld for it in $CONFIG. GNU ld 2.47 mis-links Windows imports; add:" >&2
    printf '  [target.%s]\n  rustflags = ["-C", "link-arg=-fuse-ld=lld"]\n' "$target" >&2
    status=1
  fi
done
exit "$status"
