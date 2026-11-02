#!/bin/sh
# rea-launcher-sdk packaging/smoke-package.sh: checks a built package
# against the launcher's allowlist. Every listed file must be present and
# non-empty; every file (or link) not listed fails the check, so game
# assets and stray build output can never ship.
#
#   smoke-package.sh <package-folder | package.zip> <allowlist>
#
# Allowlist: one package-relative path per line, # comments. Optional
# words after the path:
#   x         must be executable
#   empty     may be empty
#   optional  may be missing
# A path with * is a pattern (a * also crosses /): it allows every file it
# matches and must match at least one unless optional. A .zip must hold
# exactly one top-level folder, which is checked.
set -eu
die() { echo "smoke-package: $*" >&2; exit 1; }
[ $# -eq 2 ] || { echo "usage: $0 <package-folder | package.zip> <allowlist>" >&2; exit 2; }
[ -f "$2" ] || die "no allowlist $2"
LIST=$2

T=$(mktemp -d "${TMPDIR:-/tmp}/rea-smoke.XXXXXX")
trap 'rm -rf "$T"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

if [ -f "$1" ]; then
  case $1 in *.zip) ;; *) die "not a folder or .zip: $1" ;; esac
  command -v unzip >/dev/null 2>&1 || die "unzip is needed for $1"
  unzip -q "$1" -d "$T/zip" || die "cannot unzip $1"
  [ "$(ls -A "$T/zip" | wc -l)" -eq 1 ] || die "the zip must hold exactly one top-level folder"
  set -- "$T"/zip/*
  [ -d "$1" ] && [ ! -L "$1" ] || die "the zip must hold exactly one top-level folder"
fi
[ -d "$1" ] || die "no package folder $1"
PKG=$(cd "$1" && pwd -P)

# Everything but folders: links and specials must be listed too.
(cd "$PKG" && find . ! -type d -print) | sed 's|^\./||' | LC_ALL=C sort > "$T/actual"
: > "$T/allowed"
fails=0
fail() { echo "smoke-package: $*" >&2; fails=$((fails + 1)); }

# Read the allowlist (not in a pipe: the counters must survive).
awk '!/^[ \t]*(#|$)/' "$LIST" > "$T/list"
while read -r path flags; do
  case $path in /* | ../* | */../* | */..) fail "allowlist path escapes the package: $path"; continue ;; esac
  exe='' empty='' optional=''
  for flag in $flags; do
    case $flag in
      x) exe=1 ;;
      empty) empty=1 ;;
      optional) optional=1 ;;
      *) fail "unknown allowlist flag '$flag' for $path" ;;
    esac
  done
  case $path in
    *'*'*)
      matched=0
      while IFS= read -r file; do
        # shellcheck disable=SC2254
        case $file in $path) matched=1; printf '%s\n' "$file" >> "$T/allowed" ;; esac
      done < "$T/actual"
      [ "$matched" = 1 ] || [ -n "$optional" ] || fail "nothing matches $path"
      ;;
    *)
      printf '%s\n' "$path" >> "$T/allowed"
      if [ ! -e "$PKG/$path" ] && [ ! -L "$PKG/$path" ]; then
        [ -n "$optional" ] || fail "missing: $path"
        continue
      fi
      if [ -L "$PKG/$path" ] || [ ! -f "$PKG/$path" ]; then fail "not a regular file: $path"; continue; fi
      [ -n "$empty" ] || [ -s "$PKG/$path" ] || fail "empty: $path"
      [ -z "$exe" ] || [ -x "$PKG/$path" ] || fail "not executable: $path"
      ;;
  esac
done < "$T/list"

LC_ALL=C sort -u "$T/allowed" > "$T/allowed.sorted"
LC_ALL=C comm -23 "$T/actual" "$T/allowed.sorted" > "$T/extra"
if [ -s "$T/extra" ]; then
  echo "smoke-package: files not in the allowlist (game data and unlisted files are forbidden):" >&2
  sed 's/^/  /' "$T/extra" >&2
  fails=$((fails + 1))
fi
[ "$fails" = 0 ] || die "$PKG failed ($fails problem(s))"
echo "package matches $LIST: $PKG"
