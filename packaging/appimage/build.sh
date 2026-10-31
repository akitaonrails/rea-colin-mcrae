#!/bin/sh
# Builds the Linux AppImage: the launcher, cmr-play and a portable Wine.
#
#   packaging/appimage/build.sh <package-folder> <output-folder>
#
# <package-folder> is an unpacked CMRLauncher zip (CMRLauncher.exe,
# cmrhooks.dll, linux/cmr-play). Downloads (checked by SHA-256) are kept in
# $CMR_CACHE (default ~/.cache/cmr-appimage).
set -eu
PKG=$(readlink -f "$1"); OUT=$(readlink -f "$2")
HERE=$(dirname "$(readlink -f "$0")")
CACHE=${CMR_CACHE:-$HOME/.cache/cmr-appimage}

# Wine (LGPL-2.1+), Kron4ek's portable build of WineHQ's sources, in WoW64
# mode so 32-bit games need no 32-bit system libraries.
WINE_VERSION=11.20
WINE_URL=https://github.com/Kron4ek/Wine-Builds/releases/download/$WINE_VERSION/wine-$WINE_VERSION-amd64-wow64.tar.xz
WINE_SHA=482a493c755dcea776199b86560e0fb0f969bb0275c67d88120b0b35b0df34b9
TOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
TOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0

fetch() { # url sha file
  mkdir -p "$CACHE"
  if [ ! -f "$CACHE/$3" ]; then
    curl -sfL "$1" -o "$CACHE/$3.part"
    mv "$CACHE/$3.part" "$CACHE/$3"
  fi
  echo "$2  $CACHE/$3" | sha256sum -c --quiet || { echo "checksum mismatch: $3"; rm -f "$CACHE/$3"; exit 1; }
}
fetch "$WINE_URL" "$WINE_SHA" wine.tar.xz
fetch "$TOOL_URL" "$TOOL_SHA" appimagetool
chmod +x "$CACHE/appimagetool"

APPDIR=$(mktemp -d)/CMRLauncher.AppDir
trap 'rm -rf "$(dirname "$APPDIR")"' EXIT
mkdir -p "$APPDIR/usr/lib/cmr" "$APPDIR/usr/lib/wine"
tar -xJf "$CACHE/wine.tar.xz" -C "$APPDIR/usr/lib/wine" --strip-components=1
# Development files aren't needed to run.
rm -rf "$APPDIR/usr/lib/wine/include" "$APPDIR/usr/lib/wine/share/man"
for tool in function_grep.pl widl winebuild winecpp winedump wineg++ winegcc winemaker wmc wrc msidb; do
  rm -f "$APPDIR/usr/lib/wine/bin/$tool"
done
cp "$PKG/CMRLauncher.exe" "$PKG/cmrhooks.dll" "$PKG/linux/cmr-play" "$APPDIR/usr/lib/cmr/"
if [ -d "$PKG/hd" ]; then cp -r "$PKG/hd" "$APPDIR/usr/lib/cmr/hd"; fi
mkdir -p "$APPDIR/usr/share/doc/wine"
cp "$HERE"/wine/* "$APPDIR/usr/share/doc/wine/"
cp "$HERE/AppRun" "$APPDIR/AppRun"
cp "$HERE/cmr-launcher.desktop" "$HERE/cmr-launcher.png" "$APPDIR/"
chmod +x "$APPDIR/AppRun" "$APPDIR/usr/lib/cmr/cmr-play"
mkdir -p "$OUT"
ARCH=x86_64 "$CACHE/appimagetool" --appimage-extract-and-run --no-appstream \
  "$APPDIR" "$OUT/CMRLauncher-x86_64.AppImage"
echo "built $OUT/CMRLauncher-x86_64.AppImage (Wine $WINE_VERSION)"
