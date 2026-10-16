#!/bin/sh
# Compatibility entrypoint; implementation is owned by tooling/repository.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
exec sh "$root/tooling/repository/ci/ci-failure.sh" "$@"
