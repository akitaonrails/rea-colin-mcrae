#!/bin/sh
# rea-launcher-sdk packaging/check-imports.sh: checks that every import of a
# built Windows program is resolvable: each import address table (IAT) slot
# must belong to an import descriptor, or the loader never fills it and the
# program jumps into data on that call (GNU ld 2.47 does this to i686 and
# x86_64 builds with raw-dylib imports; link with lld). Generalised from
# Sega Rally Revo's tests/imports.rs.
#
#   check-imports.sh FILE.exe|FILE.dll ...
#
# PE32 and PE32+. Prints the orphaned imports by name and exits 1 when any
# file has some; exits 2 when a file isn't a PE image. POSIX sh + od + dd.
set -eu
[ $# -ge 1 ] || { echo "usage: $0 FILE.exe|FILE.dll ..." >&2; exit 2; }

# Little-endian reads from $F at a file offset (byte order independent of
# the host: od prints single bytes).
u8s() { od -An -v -tu1 -j "$1" -N "$2" "$F" 2>/dev/null; }
u16() { u8s "$1" 2 | awk '{ print $1 + 256 * $2 }'; }
u32() { u8s "$1" 4 | awk '{ print $1 + 256 * ($2 + 256 * ($3 + 256 * $4)) }'; }
# A thunk: prints "0" for the terminator, "O" for an import by ordinal,
# else the hint/name RVA.
thunk() {
  u8s "$1" "$SLOT" | awk -v w="$SLOT" '{
    for (i = 1; i <= NF; i++) b[i] = $i
    low = b[1] + 256 * (b[2] + 256 * (b[3] + 256 * b[4]))
    if (w == 8) { high = b[5] + 256 * (b[6] + 256 * (b[7] + 256 * b[8])); ord = b[8] >= 128 }
    else { high = 0; ord = b[4] >= 128 }
    if (low == 0 && high == 0) print 0; else if (ord) print "O"; else print low
  }'
}
# File offset of an RVA, from the section table ("rva size raw" lines).
offset() {
  awk -v r="$1" '{ if (r >= $1 && r < $1 + $2) { print r - $1 + $3; found = 1; exit } }
    END { if (!found) print -1 }' "$SECTIONS"
}
cstring() { dd if="$F" bs=1 skip="$1" count=128 2>/dev/null | tr '\000' '\n' | head -n 1; }

T=$(mktemp -d "${TMPDIR:-/tmp}/rea-imports.XXXXXX")
trap 'rm -rf "$T"' EXIT
SECTIONS=$T/sections
status=0

for F; do
  [ -f "$F" ] || { echo "check-imports: no file $F" >&2; exit 2; }
  if [ "$(u8s 0 2 | tr -d ' \n')" != 7790 ]; then echo "check-imports: $F: not an MZ image" >&2; exit 2; fi
  pe=$(u32 60)
  if [ "$(u8s "$pe" 4 | tr -d ' \n')" != 806900 ]; then echo "check-imports: $F: no PE signature" >&2; exit 2; fi
  opt=$((pe + 24))
  case $(u16 "$opt") in
    267) SLOT=4 dirs=$((opt + 96)) ;;   # 0x10b PE32
    523) SLOT=8 dirs=$((opt + 112)) ;;  # 0x20b PE32+
    *) echo "check-imports: $F: unknown optional header" >&2; exit 2 ;;
  esac
  nsec=$(u16 $((pe + 6)))
  table=$((opt + $(u16 $((pe + 20)))))
  : > "$SECTIONS"
  i=0
  while [ "$i" -lt "$nsec" ]; do
    s=$((table + 40 * i))
    vsize=$(u32 $((s + 8))) rva=$(u32 $((s + 12))) rsize=$(u32 $((s + 16))) raw=$(u32 $((s + 20)))
    [ "$rsize" -gt "$vsize" ] && vsize=$rsize
    echo "$rva $vsize $raw" >> "$SECTIONS"
    i=$((i + 1))
  done
  import_rva=$(u32 $((dirs + 8)))
  iat_rva=$(u32 $((dirs + 96))) iat_size=$(u32 $((dirs + 100)))
  if [ "$import_rva" = 0 ] || [ "$iat_size" = 0 ]; then
    echo "ok   $F: no imports"
    continue
  fi

  # Slots covered by the descriptors: each FirstThunk run up to its zero.
  : > "$T/covered"
  d=$(offset "$import_rva")
  [ "$d" -ge 0 ] || { echo "check-imports: $F: import directory outside every section" >&2; exit 2; }
  n=0
  while [ "$(u8s "$d" 20 | tr -d ' 0\n')" != "" ] && [ "$n" -lt 4096 ]; do
    slot=$(u32 $((d + 16)))
    k=0
    while [ "$k" -lt 65536 ]; do
      echo "$slot" >> "$T/covered"
      o=$(offset "$slot")
      [ "$o" -ge 0 ] || break
      [ "$(thunk "$o")" != 0 ] || break
      slot=$((slot + SLOT)); k=$((k + 1))
    done
    d=$((d + 20)); n=$((n + 1))
  done

  # Every IAT slot no descriptor covers, named when it imports by name.
  : > "$T/orphans"
  slot=$iat_rva
  while [ "$slot" -lt $((iat_rva + iat_size)) ]; do
    if ! grep -qx "$slot" "$T/covered"; then
      o=$(offset "$slot")
      t=$([ "$o" -ge 0 ] && thunk "$o" || echo 0)
      case $t in
        0 | O) ;;
        *) h=$(offset "$t"); if [ "$h" -ge 0 ]; then cstring $((h + 2)) >> "$T/orphans"; else echo "(rva $t)" >> "$T/orphans"; fi ;;
      esac
    fi
    slot=$((slot + SLOT))
  done
  if [ -s "$T/orphans" ]; then
    echo "FAIL $F: $(wc -l < "$T/orphans" | tr -d ' ') import(s) outside every import descriptor (link with lld: -C link-arg=-fuse-ld=lld):" >&2
    sed 's/^/  /' "$T/orphans" >&2
    status=1
  else
    echo "ok   $F: every import belongs to a descriptor ($(sort -u "$T/covered" | wc -l | tr -d ' ') slots)"
  fi
done
exit "$status"
