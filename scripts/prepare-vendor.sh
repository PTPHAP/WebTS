#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
git submodule update --init --recursive
if git -C vendor/tsclientlib apply --reverse --check ../../patches/raw-audio.patch 2>/dev/null; then
  echo 'raw-audio patch already applied'
else
  git -C vendor/tsclientlib apply --check ../../patches/raw-audio.patch
  git -C vendor/tsclientlib apply ../../patches/raw-audio.patch
fi
