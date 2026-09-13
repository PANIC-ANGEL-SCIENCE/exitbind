#!/bin/sh
set -u

if [ "$#" -lt 2 ]; then
  echo "usage: quiet-command.sh LABEL COMMAND [ARG ...]" >&2
  exit 2
fi

label=$1
shift
case "$label" in
  ''|*[!A-Za-z0-9._-]*)
    echo "quiet-command label must be portable" >&2
    exit 2
    ;;
esac

log=$(mktemp "${TMPDIR:-/tmp}/soulmate-command.XXXXXX") || exit 1
trap 'rm -f "$log"' 0

if "$@" >"$log" 2>&1; then
  printf 'ok: %s\n' "$label"
else
  status=$?
  cat "$log" >&2
  printf '\nfailed (%s): %s\n' "$status" "$label" >&2
  exit "$status"
fi
