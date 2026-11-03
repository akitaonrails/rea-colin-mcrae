#!/bin/sh
# rea-launcher-sdk packaging/release-notes.sh: prints one version's
# CHANGELOG section, for its GitHub release.
#
#   release-notes.sh [--since <version or tag>] <version or tag> [changelog]
#
# "v0.2.0" and "0.2.0" both find any of these headings:
#   ## [0.2.0] - 2026-11-02     ## 0.2.0 - 2026-11-02
#   ## v0.2.0 - 2026-11-02      ## 0.2.0
# The section ends at the next "## " heading or the link references.
# --since vX (a fix-forward release: v0.3.1 was tagged but never
# published, v0.3.2 fixes it) prints every section after vX up to the
# version, newest first, each under its own "## " heading line.
# Fails when the version's section is missing or empty, so a release never
# goes out without notes. Plain POSIX awk (mawk, gawk, busybox).
set -eu
usage() { echo "usage: $0 [--since <version or tag>] <version or tag> [changelog]" >&2; exit 2; }
since=''
if [ "${1:-}" = --since ]; then
  [ $# -ge 2 ] || usage
  since=${2#v}; shift 2
  [ -n "$since" ] || usage
fi
[ $# -ge 1 ] && [ $# -le 2 ] || usage
version=${1#v}
changelog=${2:-CHANGELOG.md}
[ -n "$version" ] || { echo "release-notes: empty version" >&2; exit 2; }
[ -f "$changelog" ] || { echo "release-notes: no $changelog" >&2; exit 1; }
# Prints the version's section; with since, also every section after it
# (by version number) up to the version, with headings. Exit 3: the
# version's own section is missing or empty.
notes=$(awk -v v="$version" -v since="$since" '
  function heading_version(line,   h) {
    h = line
    sub(/^##[ \t]+/, "", h); sub(/[ \t].*$/, "", h)
    gsub(/\[/, "", h); gsub(/\]/, "", h); sub(/^v/, "", h)
    return h
  }
  function cmp(a, b,   x, y, i) {
    split(a, x, "."); split(b, y, ".")
    for (i = 1; i <= 3; i++) {
      if (x[i] + 0 < y[i] + 0) return -1
      if (x[i] + 0 > y[i] + 0) return 1
    }
    return 0
  }
  function versionish(h) { return h ~ /^[0-9]+\.[0-9]+\.[0-9]+/ }
  # Flushes the current section (trimmed), with its heading when several are shown.
  function flush(   first, last, i, body) {
    if (!cur) return
    first = 1; while (first <= n && line[first] ~ /^[ \t]*$/) first++
    last = n; while (last >= first && line[last] ~ /^[ \t]*$/) last--
    if (curv == v) own = (last >= first)
    if (since != "" || curv == v) {
      if (shown) print ""
      if (since != "") { print head; if (last >= first) print "" }
      for (i = first; i <= last; i++) print line[i]
      shown = 1
    }
    cur = 0; n = 0
  }
  /^## / {
    flush()
    h = heading_version($0)
    if (h == v) { cur = 1; curv = h; head = $0 }
    else if (since != "" && versionish(h) && versionish(v) && cmp(h, since) > 0 && cmp(h, v) < 0) { cur = 1; curv = h; head = $0 }
    next
  }
  /^\[[^ \t]*\]: / { flush(); exit }
  cur { n++; line[n] = $0 }
  END { flush(); if (!own) exit 3 }
' "$changelog") || {
  echo "release-notes: no CHANGELOG section for $version in $changelog" >&2
  exit 1
}
printf '%s\n' "$notes"
