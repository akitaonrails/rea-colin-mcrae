#!/bin/sh
# rea-launcher-sdk packaging/appimage-wine.sh: builds a launcher's Linux
# AppImage from its unpacked package folder, with a portable Wine (default)
# or none (--native, for launchers built for Linux).
#
#   appimage-wine.sh [options] <package-folder> <output-folder>
#
# Options (or the REA_* environment variable in brackets):
#   --app ID            short id, e.g. cmr3; files go in usr/lib/<ID> [REA_APP]
#   --exe FILE          launcher in the package root, e.g. CMR3Launcher.exe [REA_EXE]
#   --play FILE         helper in <package>/linux (default <ID>-play) [REA_PLAY]
#   --env-prefix NAME   helper variable prefix (default ID upper-cased):
#                       AppRun sets NAME_LAUNCHER and NAME_WAIT [REA_ENV_PREFIX]
#   --name NAME         output NAME-x86_64.AppImage (default: exe stem) [REA_NAME]
#   --icon FILE         PNG icon [REA_ICON]
#   --desktop FILE      ready .desktop entry; its Icon= must be the icon's stem [REA_DESKTOP]
#   --title TEXT        or generate the entry: Name= ... [REA_TITLE]
#   --comment TEXT      ... and Comment= [REA_COMMENT]
#   --pins FILE         pins for wine, appimagetool, appimage-runtime
#                       (default: appimage/tools.pins beside this script) [REA_PINS]
#   --cache DIR         download cache (pins.sh PIN_CACHE) [REA_CACHE]
#   --payload PATH      package file or folder copied to usr/lib/<ID>/PATH; repeatable
#   --optional-payload PATH   the same, skipped when missing; repeatable
#   --doc PATH          extra package file or folder for usr/share/doc/<ID>/PATH; repeatable
#   --env-path VAR=PATH AppRun also exports VAR="$LIB/PATH"; repeatable
#   --play-lib FILE     play library copied beside the helper (default:
#                       ../rea-play.sh from this script, the vendored
#                       sdk/rea-play.sh; else ../shell/rea-play.sh, an SDK
#                       checkout). A helper that names rea-play.sh fails the
#                       build when none is found. [REA_PLAY_LIB]
#   --native            no Wine: native launcher, lib/ on LD_LIBRARY_PATH [REA_MODE=native]
#
# The package must hold the exe, linux/<play>, README.md, THIRD_PARTY.md,
# LICENSE-MIT and LICENSE-APACHE (CHANGELOG.md and linux/README.md are
# shipped when present); they all go into usr/share/doc/<ID>.
set -eu

die() { echo "appimage-wine: $*" >&2; exit 1; }
usage() { sed -n '2,/^set -eu/p' "$0" | sed '$d; s/^# \{0,1\}//' >&2; exit 2; }
# Relative paths inside the package: no absolute paths, no "..".
check_rel() {
  case $1 in
    '' | /* | *[!A-Za-z0-9._/+-]* | .. | ../* | */.. | */../*) die "unsafe path: '$1'" ;;
  esac
}
need_arg() { [ $# -ge 2 ] || die "$1 needs a value"; }

HERE=$(dirname "$(readlink -f "$0")")
APP=${REA_APP:-}; EXE=${REA_EXE:-}; PLAY=${REA_PLAY:-}; PFX=${REA_ENV_PREFIX:-}
NAME=${REA_NAME:-}; ICON=${REA_ICON:-}; DESKTOP=${REA_DESKTOP:-}
TITLE=${REA_TITLE:-}; COMMENT=${REA_COMMENT:-}
PINS=${REA_PINS:-$HERE/appimage/tools.pins}; MODE=${REA_MODE:-wine}
PLAY_LIB=${REA_PLAY_LIB:-}
PAYLOAD='' OPTIONAL='' DOCS='' ENVS=''
[ -z "${REA_CACHE:-}" ] || PIN_CACHE=$REA_CACHE

while [ $# -gt 0 ]; do
  case $1 in
    --app) need_arg "$@"; APP=$2; shift 2 ;;
    --exe) need_arg "$@"; EXE=$2; shift 2 ;;
    --play) need_arg "$@"; PLAY=$2; shift 2 ;;
    --env-prefix) need_arg "$@"; PFX=$2; shift 2 ;;
    --name) need_arg "$@"; NAME=$2; shift 2 ;;
    --icon) need_arg "$@"; ICON=$2; shift 2 ;;
    --desktop) need_arg "$@"; DESKTOP=$2; shift 2 ;;
    --title) need_arg "$@"; TITLE=$2; shift 2 ;;
    --comment) need_arg "$@"; COMMENT=$2; shift 2 ;;
    --pins) need_arg "$@"; PINS=$2; shift 2 ;;
    --cache) need_arg "$@"; PIN_CACHE=$2; export PIN_CACHE; shift 2 ;;
    --payload) need_arg "$@"; check_rel "$2"; PAYLOAD="$PAYLOAD $2"; shift 2 ;;
    --optional-payload) need_arg "$@"; check_rel "$2"; OPTIONAL="$OPTIONAL $2"; shift 2 ;;
    --doc) need_arg "$@"; check_rel "$2"; DOCS="$DOCS $2"; shift 2 ;;
    --env-path) need_arg "$@"; ENVS="$ENVS $2"; shift 2 ;;
    --play-lib) need_arg "$@"; PLAY_LIB=$2; shift 2 ;;
    --native) MODE=native; shift ;;
    -h | --help) usage ;;
    --) shift; break ;;
    -*) die "unknown option $1 (see --help)" ;;
    *) break ;;
  esac
done
[ $# -eq 2 ] || usage

# Identifiers end up in AppRun and paths: keep them plain.
case $APP in '' | *[!a-z0-9-]*) die "--app must be lower-case letters, digits, '-' (got '$APP')" ;; esac
[ -n "$EXE" ] || die "--exe is required"
case $EXE in */* | *[!A-Za-z0-9._+-]*) die "unsafe --exe '$EXE'" ;; esac
PLAY=${PLAY:-$APP-play}
case $PLAY in */* | *[!A-Za-z0-9._+-]*) die "unsafe --play '$PLAY'" ;; esac
PFX=${PFX:-$(printf '%s' "$APP" | tr 'a-z-' 'A-Z_')}
case $PFX in '' | [!A-Z]* | *[!A-Z0-9_]*) die "--env-prefix must match [A-Z][A-Z0-9_]* (got '$PFX')" ;; esac
NAME=${NAME:-${EXE%.exe}}
case $NAME in */* | *[!A-Za-z0-9._+-]*) die "unsafe --name '$NAME'" ;; esac
case $MODE in wine | native) ;; *) die "REA_MODE must be wine or native" ;; esac
for kv in $ENVS; do
  case ${kv%%=*} in '' | [!A-Z]* | *[!A-Z0-9_]*) die "bad --env-path variable in '$kv'" ;; esac
  case $kv in *=*) check_rel "${kv#*=}" ;; *) die "--env-path wants VAR=PATH (got '$kv')" ;; esac
done
[ -n "$ICON" ] && [ -f "$ICON" ] || die "--icon must name a PNG file"
case $ICON in *.png) ;; *) die "--icon must be a .png" ;; esac
ICON_NAME=$(basename "$ICON" .png)
case $ICON_NAME in *[!A-Za-z0-9._-]*) die "unsafe icon name '$ICON_NAME'" ;; esac
if [ -n "$DESKTOP" ]; then
  [ -f "$DESKTOP" ] || die "no desktop file $DESKTOP"
  grep -qx "Icon=$ICON_NAME" "$DESKTOP" || die "$DESKTOP needs Icon=$ICON_NAME"
else
  [ -n "$TITLE" ] || die "give --desktop FILE or --title TEXT"
  case "$TITLE$COMMENT" in *'
'*) die "--title/--comment must be one line" ;; esac
fi
[ -f "$PINS" ] || die "no pin file $PINS"
# An explicit library must exist; the vendored default is used when present.
if [ -n "$PLAY_LIB" ]; then
  [ -f "$PLAY_LIB" ] || die "no play library $PLAY_LIB"
elif [ -f "$HERE/../rea-play.sh" ]; then
  PLAY_LIB=$HERE/../rea-play.sh
elif [ -f "$HERE/../shell/rea-play.sh" ]; then
  PLAY_LIB=$HERE/../shell/rea-play.sh
fi
for tool in curl tar sha256sum; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is needed"
done

[ -d "$1" ] || die "no package folder $1"
PKG=$(cd "$1" && pwd -P)
mkdir -p "$2"
OUT=$(cd "$2" && pwd -P)
for f in "$EXE" "linux/$PLAY" README.md THIRD_PARTY.md LICENSE-MIT LICENSE-APACHE; do
  [ -f "$PKG/$f" ] || die "package lacks $f: $PKG"
done
for p in $PAYLOAD $DOCS; do
  [ -e "$PKG/$p" ] || die "package lacks $p: $PKG"
done
# Never ship a helper whose library is missing.
if [ -z "$PLAY_LIB" ] && grep -q 'rea-play\.sh' "$PKG/linux/$PLAY"; then
  die "linux/$PLAY sources rea-play.sh but none was found (run scripts/sdk-sync, or pass --play-lib)"
fi

WORK=$(mktemp -d "${TMPDIR:-/tmp}/rea-appimage.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

. "$HERE/pins.sh"
# shellcheck disable=SC2034 # read by pins.sh
PIN_FILE=$PINS PIN_DIR=''
DL=$WORK/downloads
TOOL=$(pin_fetch appimagetool "$DL") || die "cannot fetch appimagetool"
RUNTIME=$(pin_fetch appimage-runtime "$DL") || die "cannot fetch the AppImage runtime"
chmod +x "$TOOL"

APPDIR=$WORK/$NAME.AppDir
LIB=$APPDIR/usr/lib/$APP
DOC=$APPDIR/usr/share/doc/$APP
mkdir -p "$LIB" "$DOC"

if [ "$MODE" = wine ]; then
  WINE=$(pin_fetch wine "$DL") || die "cannot fetch Wine"
  # The shipped source notes must name the Wine we ship: the archive's own
  # top folder (wine-11.20-amd64-wow64/) says which, whatever the pin's
  # url is called (a file:// copy named by its checksum, offline); else the
  # url's file name.
  WINE_VERSION=$( (tar -tJf "$WINE" 2>/dev/null || true) | head -n 1 \
    | sed -n 's#^\./##; s#^wine-\([0-9][0-9.]*[0-9]\)[-/].*#\1#p')
  [ -n "$WINE_VERSION" ] || WINE_VERSION=$(basename "$(pin_url wine)" | sed -n 's/^wine-\([0-9][0-9.]*[0-9]\)-.*/\1/p')
  [ -n "$WINE_VERSION" ] || die "cannot read the Wine version from the Wine archive's top folder or the wine pin's file name"
  grep -q "Wine $WINE_VERSION" "$HERE/appimage/wine/SOURCE.txt" \
    || die "appimage/wine/SOURCE.txt does not describe Wine $WINE_VERSION"
  mkdir -p "$APPDIR/usr/lib/wine" "$APPDIR/usr/share/doc/wine"
  tar -xJf "$WINE" -C "$APPDIR/usr/lib/wine" --strip-components=1
  [ -x "$APPDIR/usr/lib/wine/bin/wine" ] || die "the Wine archive has no bin/wine"
  # Development files aren't needed to run.
  rm -rf "$APPDIR/usr/lib/wine/include" "$APPDIR/usr/lib/wine/share/man"
  for t in function_grep.pl widl winebuild winecpp winedump wineg++ winegcc winemaker wmc wrc msidb; do
    rm -f "$APPDIR/usr/lib/wine/bin/$t"
  done
  cp "$HERE/appimage/wine/COPYING.LIB" "$HERE/appimage/wine/LICENSE" \
    "$HERE/appimage/wine/SOURCE.txt" "$APPDIR/usr/share/doc/wine/"
fi

# The launcher, its helper with the SDK play library beside it, and the
# payload at the same relative paths as in the package.
cp "$PKG/$EXE" "$LIB/$EXE"
cp "$PKG/linux/$PLAY" "$LIB/$PLAY"
if [ -n "$PLAY_LIB" ]; then cp "$PLAY_LIB" "$LIB/rea-play.sh"; fi
copy_in() { # package-relative path, destination root
  [ ! -e "$2/$1" ] || die "$1 is listed twice (or clashes with the launcher)"
  mkdir -p "$(dirname "$2/$1")"
  cp -R "$PKG/$1" "$2/$1"
}
for p in $PAYLOAD; do copy_in "$p" "$LIB"; done
for p in $OPTIONAL; do
  if [ -e "$PKG/$p" ]; then copy_in "$p" "$LIB"; fi
done
# One copy of the helper: AppRun runs the top-level one.
rm -f "$LIB/linux/$PLAY"
chmod +x "$LIB/$PLAY"
if [ "$MODE" = native ]; then chmod +x "$LIB/$EXE"; fi
for kv in $ENVS; do
  [ -e "$LIB/${kv#*=}" ] || die "--env-path $kv: usr/lib/$APP/${kv#*=} is not in the AppImage"
done

# Our licences, the third-party list and the guides, as in the package.
for f in README.md THIRD_PARTY.md LICENSE-MIT LICENSE-APACHE; do cp "$PKG/$f" "$DOC/"; done
if [ -f "$PKG/CHANGELOG.md" ]; then cp "$PKG/CHANGELOG.md" "$DOC/"; fi
if [ -f "$PKG/linux/README.md" ]; then mkdir -p "$DOC/linux" && cp "$PKG/linux/README.md" "$DOC/linux/"; fi
for p in $DOCS; do copy_in "$p" "$DOC"; done

# AppRun from the template: keep this mode's lines, fill the names.
ENV_LINES=
for kv in $ENVS; do
  ENV_LINES="$ENV_LINES${ENV_LINES:+;}export ${kv%%=*}=\"\$LIB/${kv#*=}\""
done
awk -v mode="$MODE" -v app="$APP" -v exe="$EXE" -v play="$PLAY" -v pfx="$PFX" \
  -v envs="$ENV_LINES" '
  /^@WINE@/ { if (mode != "wine") next; sub(/^@WINE@/, "") }
  /^@NATIVE@/ { if (mode != "native") next; sub(/^@NATIVE@/, "") }
  /^@ENV@$/ { n = split(envs, e, ";"); for (i = 1; i <= n; i++) print e[i]; next }
  { gsub(/@APP@/, app); gsub(/@EXE@/, exe); gsub(/@PLAY@/, play); gsub(/@PREFIX@/, pfx); print }
' "$HERE/appimage/AppRun.in" > "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun"
! grep -n '@[A-Z]*@' "$APPDIR/AppRun" >&2 || die "AppRun.in has an unknown placeholder"
sh -n "$APPDIR/AppRun"

cp "$ICON" "$APPDIR/$ICON_NAME.png"
if [ -n "$DESKTOP" ]; then
  cp "$DESKTOP" "$APPDIR/$ICON_NAME.desktop"
else
  {
    echo '[Desktop Entry]'
    echo 'Type=Application'
    echo "Name=$TITLE"
    if [ -n "$COMMENT" ]; then echo "Comment=$COMMENT"; fi
    echo 'Exec=AppRun'
    echo "Icon=$ICON_NAME"
    echo 'Terminal=false'
    echo 'Categories=Game;'
  } > "$APPDIR/$ICON_NAME.desktop"
fi

TARGET=$OUT/$NAME-x86_64.AppImage
rm -f "$TARGET"
ARCH=x86_64 "$TOOL" --appimage-extract-and-run --no-appstream \
  --runtime-file "$RUNTIME" "$APPDIR" "$TARGET"
[ -s "$TARGET" ] || die "appimagetool produced nothing"
echo "built $TARGET (${MODE}${WINE_VERSION:+ $WINE_VERSION})"
