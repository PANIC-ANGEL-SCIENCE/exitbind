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

# ci-local supplies a private per-run path so diagnostic evidence survives.
if [ -n "${QUIET_COMMAND_LOG:-}" ]; then
  log=$QUIET_COMMAND_LOG
else
  log=$(mktemp "${TMPDIR:-/tmp}/soulmate-command.XXXXXX") || exit 1
  trap 'rm -f "$log"' 0
fi

if (unset QUIET_COMMAND_LOG; "$@") >"$log" 2>&1; then
  if [ -n "${QUIET_COMMAND_LOG:-}" ]; then
    printf '0\n' > "$log.exit"
  fi
  printf 'ok: %s\n' "$label"
else
  status=$?
  if [ -n "${QUIET_COMMAND_LOG:-}" ]; then
    # A console can truncate or disconnect; retain the complete file separately.
    printf '%s\n' "$status" > "$log.exit"
    printf 'failed (%s): %s; full log: %s\n' "$status" "$label" "$log" >&2
    tail -c 8192 "$log" >&2
  else
    cat "$log" >&2
    printf '\nfailed (%s): %s\n' "$status" "$label" >&2
  fi
  exit "$status"
fi
