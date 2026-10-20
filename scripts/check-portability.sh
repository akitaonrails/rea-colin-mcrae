#!/usr/bin/env bash
set -euo pipefail

# Keep developer-specific source locations out of version control. The importer
# must receive its source root explicitly; tracked examples use placeholders.
developer_path_pattern='/(home|Users|mnt)/[^[:space:]<>]+/(Emulators|Games|ROMS?(_FINAL)?)/'

if git grep -nIE "$developer_path_pattern" -- ':!scripts/check-portability.sh'; then
  echo "error: tracked files contain a developer-specific game source path" >&2
  echo 'use --source "$CMR_ORIGINAL_DIR" and a /path/to/... example instead' >&2
  exit 1
fi

echo "portability check passed: no developer-specific game source paths are tracked"
