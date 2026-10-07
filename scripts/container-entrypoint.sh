#!/bin/sh
set -eu
umask 077
exec /usr/local/bin/web-ts "$@"
