#!/bin/sh
# rea-launcher-sdk packaging/release-notes.sh: prints one version's
# CHANGELOG section, for its GitHub release.
#
#   release-notes.sh <version or tag> [changelog]
#
# "v0.2.0" and "0.2.0" both find any of these headings:
#   ## [0.2.0] - 2026-11-02     ## 0.2.0 - 2026-11-02
#   ## v0.2.0 - 2026-11-02      ## 0.2.0
# The section ends at the next "## " heading or the link references.
# Fails when the section is missing or empty, so a release never goes out
# without notes. Plain POSIX awk (mawk, gawk, busybox).
set -eu
[ $# -ge 1 ] && [ $# -le 2 ] || { echo "usage: $0 <version or tag> [changelog]" >&2; exit 2; }
version=${1#v}
changelog=${2:-CHANGELOG.md}
[ -n "$version" ] || { echo "release-notes: empty version" >&2; exit 2; }
[ -f "$changelog" ] || { echo "release-notes: no $changelog" >&2; exit 1; }
notes=$(awk -v v="$version" '
  function heading_version(line,   h) {
    h = line
    sub(/^##[ \t]+/, "", h); sub(/[ \t].*$/, "", h)
    gsub(/\[/, "", h); gsub(/\]/, "", h); sub(/^v/, "", h)
    return h
  }
  /^## / {
    if (take) exit
    if (heading_version($0) == v) take = 1
    next
  }
  take && /^\[[^ \t]*\]: / { exit }
  take { n++; line[n] = $0 }
  END {
    first = 1; while (first <= n && line[first] ~ /^[ \t]*$/) first++
    last = n; while (last >= first && line[last] ~ /^[ \t]*$/) last--
    for (i = first; i <= last; i++) print line[i]
  }
' "$changelog")
[ -n "$notes" ] || { echo "release-notes: no CHANGELOG section for $version in $changelog" >&2; exit 1; }
printf '%s\n' "$notes"
