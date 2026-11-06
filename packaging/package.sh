#!/bin/sh
# packaging/package.sh [--no-appimage] <package-folder>
# Builds the zip folder (e.g. dist/CMRLauncher-v0.7.0/), its .zip beside it,
# and the AppImage (portable Wine, sdk/packaging/appimage-wine.sh) in the
# same parent folder. CI calls it from launcher-ci.yml's package-command
# (sdk/docs/playbooks/ci-and-releases.md).
#
# The zip plays on Windows (CMRLauncher.exe) and on Linux through
# linux/cmr-play; the HD art pack (packaging/hd-pack.txt, checked by
# SHA-256) is unpacked into hd/.
#
# Environment: REA_RELEASE_DIR holds CMRLauncher.exe and cmrhooks.dll (CI
# sets it; default target/i686-pc-windows-gnu/release); REA_CACHE caches
# pinned downloads; CMR_PACKAGE_PINS replaces the folder of pin files
# (packaging/, tests only). Everything the folder holds must be listed in
# packaging/package.allow.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
appimage=1
if [ "${1:-}" = --no-appimage ]; then appimage='' && shift; fi
[ $# -eq 1 ] || { echo "usage: $0 [--no-appimage] <package-folder>" >&2; exit 2; }
pkg=$1
name=$(basename "$pkg")
case $name in '' | *[!A-Za-z0-9._-]*) echo "package: bad package name '$name'" >&2; exit 2 ;; esac
dist=$(dirname "$pkg")
mkdir -p "$dist"
dist=$(cd "$dist" && pwd -P)
pkg=$dist/$name
release=${REA_RELEASE_DIR:-$ROOT/target/i686-pc-windows-gnu/release}
for f in CMRLauncher.exe cmrhooks.dll; do
  [ -f "$release/$f" ] || { echo "package: no $release/$f (cargo build --release --target i686-pc-windows-gnu --workspace)" >&2; exit 1; }
done

. "$ROOT/sdk/packaging/pins.sh"
PIN_FILE=
PIN_DIR=${CMR_PACKAGE_PINS:-$ROOT/packaging}
PIN_CACHE=${REA_CACHE:-${XDG_CACHE_HOME:-$HOME/.cache}/cmr-launcher}/pins
export PIN_FILE PIN_DIR PIN_CACHE
work=$(mktemp -d "${TMPDIR:-/tmp}/cmr-package.XXXXXX")
trap 'rm -rf "$work"' EXIT

rm -rf "$pkg" "$dist/$name.zip"
mkdir -p "$pkg/linux/tests" "$pkg/licenses" "$pkg/hd" "$pkg/dgvoodoo"
cp "$release/CMRLauncher.exe" "$release/cmrhooks.dll" "$pkg/"
cp "$ROOT/README.md" "$ROOT/THIRD_PARTY.md" "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$pkg/"
[ -f "$ROOT/CHANGELOG.md" ] && cp "$ROOT/CHANGELOG.md" "$pkg/"
cp "$ROOT/linux/cmr-play" "$ROOT/linux/README.md" "$pkg/linux/"
cp "$ROOT"/linux/tests/*.sh "$pkg/linux/tests/"
# The helper finds the SDK play library beside itself.
cp "$ROOT/sdk/rea-play.sh" "$pkg/linux/rea-play.sh"
chmod +x "$pkg/linux/cmr-play"
# The settings window's fonts (Barlow, SIL OFL), compiled into the exe.
cp "$ROOT"/packaging/licenses/* "$pkg/licenses/"

# The HD art pack: its pack.txt and the x2/x4 sets the launcher installs
# into its runtime folder.
zip=$(pin_fetch hd-pack "$work") || { echo "package: no source for the HD art pack matched its pin" >&2; exit 1; }
unzip -q "$zip" -d "$pkg/hd"
[ -f "$pkg/hd/pack.txt" ] || { echo "package: the HD art pack has no pack.txt" >&2; exit 1; }

# dgVoodoo2 2.87.6 (MS/x86/DDraw.dll and MS/x86/D3DImm.dll): installed into
# the runtime folder on native Windows so DirectDraw and Direct3D Immediate
# Mode work on Windows 10 and 11 out of the box.
dgv=$(pin_fetch dgvoodoo "$work") || { echo "package: no source for dgVoodoo2 matched its pin" >&2; exit 1; }
mkdir -p "$work/dgvoodoo"
unzip -q "$dgv" MS/x86/DDraw.dll MS/x86/D3DImm.dll -d "$work/dgvoodoo"
cp "$work/dgvoodoo/MS/x86/DDraw.dll" "$work/dgvoodoo/MS/x86/D3DImm.dll" "$pkg/dgvoodoo/"

(cd "$dist" && zip -qr "$name.zip" "$name")
echo "built $dist/$name.zip"

if [ -n "$appimage" ]; then
  sh "$ROOT/sdk/packaging/appimage-wine.sh" --app cmr --exe CMRLauncher.exe \
    --icon "$ROOT/packaging/appimage/cmr-launcher.png" \
    --desktop "$ROOT/packaging/appimage/cmr-launcher.desktop" \
    --payload cmrhooks.dll --payload hd --doc licenses \
    ${REA_CACHE:+--cache "$REA_CACHE/appimage"} \
    --name "$name" "$pkg" "$dist"
fi
