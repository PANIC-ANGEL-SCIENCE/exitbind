#!/bin/sh
# Compatibility entrypoint for repository README administration.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd -P)
exec sh "$root/tooling/repository/ci/readme/publish.sh" "$@"
