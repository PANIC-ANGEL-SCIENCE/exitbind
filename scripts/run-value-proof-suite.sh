#!/bin/sh
set -u

binary=${EXITBIND_BIN:-${SOULMATE_BIN:-exitbind}}
exec "$binary" benchmark --json "$@"
