#!/bin/sh
# Compatibility entrypoint; repository packaging lives under tooling/repository.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
exec sh "$root/tooling/repository/release/package-plugin.sh" "$@"
