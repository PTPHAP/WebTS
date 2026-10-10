#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
git -c core.autocrlf=false submodule update --init --recursive
if git -C vendor/tsclientlib apply --reverse --check ../../patches/raw-audio.patch 2>/dev/null; then
  echo 'raw-audio patch already applied'
else
  git -C vendor/tsclientlib apply --check ../../patches/raw-audio.patch || {
    echo 'Protocol patch differs from this checkout. Preserve local changes and use a clean checkout with LF endings; this script does not reset your files.' >&2
    exit 1
  }
  git -C vendor/tsclientlib apply ../../patches/raw-audio.patch
fi
