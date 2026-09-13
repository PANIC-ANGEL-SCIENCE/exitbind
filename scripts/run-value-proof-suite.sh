#!/bin/sh
set -u

binary=${EXITBIND_BIN:-${SOULMATE_BIN:-soulmate}}
exec "$binary" benchmark --json "$@"
