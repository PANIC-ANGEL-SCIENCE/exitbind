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

python=$(command -v python3) || {
  echo 'quiet-command: Python 3 is required to capture and project command output' >&2
  exit 127
}
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd) || exit 1
exec "$python" "$script_dir/quiet-command.py" run "$label" "$@"
