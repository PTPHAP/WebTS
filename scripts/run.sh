#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
umask 077
test -f config.local.toml || { echo '请按docs/DEPLOYMENT.md创建config.local.toml'; exit 1; }
exec ./web-ts serve config.local.toml
