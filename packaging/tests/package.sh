#!/bin/sh
# packaging/package.sh builds a zip folder that passes packaging/package.allow
# (no AppImage, no network: stand-in exe and DLL, a tiny HD pack from a
# file:// pin): sh packaging/tests/package.sh
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/cmr-package-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; fails=$((fails + 1)); }
command -v zip >/dev/null 2>&1 && command -v unzip >/dev/null 2>&1 || { echo "skip: zip/unzip not installed"; exit 0; }

mkdir -p "$T/release" "$T/pack/x2" "$T/pack/x4" "$T/pins"
printf 'MZ stand-in\n' > "$T/release/CMRLauncher.exe"
printf 'MZ stand-in\n' > "$T/release/cmrhooks.dll"
printf 'cmr-hd pack 1\nx2/0123456789abcdef.png\nx4/0123456789abcdef.webp\nx2/fedcba9876543210.webp\nx4/fedcba9876543210.png\n' > "$T/pack/pack.txt"
for f in x2/0123456789abcdef.png x4/0123456789abcdef.webp x2/fedcba9876543210.webp x4/fedcba9876543210.png; do
  printf 'image\n' > "$T/pack/$f"
done
(cd "$T/pack" && zip -qr "$T/cmr-hd-pack-test.zip" .)
sha=$(sha256sum "$T/cmr-hd-pack-test.zip" | cut -d' ' -f1)
echo "file://$T/cmr-hd-pack-test.zip $sha hosted" > "$T/pins/hd-pack.txt"
mkdir -p "$T/dgv/MS/x86"
printf 'MZ stand-in\n' > "$T/dgv/MS/x86/DDraw.dll"
printf 'MZ stand-in\n' > "$T/dgv/MS/x86/D3DImm.dll"
(cd "$T/dgv" && zip -qr "$T/dgVoodoo2_87_6.zip" .)
dgv_sha=$(sha256sum "$T/dgVoodoo2_87_6.zip" | cut -d' ' -f1)
echo "file://$T/dgVoodoo2_87_6.zip $dgv_sha hosted" > "$T/pins/dgvoodoo.txt"
run() {
  REA_RELEASE_DIR=$1 REA_CACHE=$T/cache CMR_PACKAGE_PINS=$T/pins \
    sh "$ROOT/packaging/package.sh" --no-appimage "$2" > "$T/out" 2>&1
}
if run "$T/release" "$T/dist/CMRLauncher-v0.0.0"; then
  ok "package.sh builds the folder and zip"
else
  fail "package.sh: $(cat "$T/out")"
fi
for p in "$T/dist/CMRLauncher-v0.0.0" "$T/dist/CMRLauncher-v0.0.0.zip"; do
  if sh "$ROOT/sdk/packaging/smoke-package.sh" "$p" "$ROOT/packaging/package.allow" > "$T/out" 2>&1; then
    ok "allowlist passes: $(basename "$p")"
  else
    fail "allowlist: $(basename "$p"): $(cat "$T/out")"
  fi
done
[ -x "$T/dist/CMRLauncher-v0.0.0/linux/cmr-play" ] && ok "cmr-play is executable" || fail "cmr-play not executable"
cmp -s "$ROOT/sdk/rea-play.sh" "$T/dist/CMRLauncher-v0.0.0/linux/rea-play.sh" && ok "the play library ships beside cmr-play" || fail "no rea-play.sh beside cmr-play"
# The packaged helper finds its library beside itself.
if [ "$(HOME=$T sh "$T/dist/CMRLauncher-v0.0.0/linux/cmr-play" status)" = idle ]; then ok "the packaged cmr-play runs"; else fail "packaged cmr-play"; fi
# A pack that doesn't match its pin is refused.
echo "file://$T/cmr-hd-pack-test.zip $(printf '0%.0s' $(seq 64)) hosted" > "$T/pins/hd-pack.txt"
if run "$T/release" "$T/dist/CMRLauncher-bad"; then fail "a pack that doesn't match its pin must fail"; else ok "a pack that doesn't match its pin fails"; fi
echo "file://$T/cmr-hd-pack-test.zip $sha hosted" > "$T/pins/hd-pack.txt"
mkdir -p "$T/nodll"; cp "$T/release/CMRLauncher.exe" "$T/nodll/"
if run "$T/nodll" "$T/dist/x"; then fail "a missing cmrhooks.dll must fail"; else ok "a missing cmrhooks.dll fails"; fi
if run "$T/nowhere" "$T/dist/x"; then fail "a missing exe must fail"; else ok "a missing exe fails"; fi

if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
echo "$fails failed"; exit 1
