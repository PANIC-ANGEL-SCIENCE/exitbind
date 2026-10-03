#!/bin/sh
# Sourced by ci-local: no build, global PATH edit, service or remembered host facts.

canonical_destination() {
  requested_path=$1
  case "$requested_path" in /*) ;; *) requested_path="$root/$requested_path" ;; esac
  case "$requested_path" in *'
'*) fail 'execution paths must not contain newlines' ;; esac
  unresolved_suffix=
  while [ ! -e "$requested_path" ]; do
    path_leaf=$(basename -- "$requested_path")
    case "$path_leaf" in .|..) fail 'unresolved execution path contains dot components' ;; esac
    unresolved_suffix="/$path_leaf$unresolved_suffix"
    requested_path=$(dirname -- "$requested_path")
  done
  existing_path=$(CDPATH= cd -- "$requested_path" && pwd -P) || fail 'execution path parent is not a directory'
  printf '%s%s\n' "$existing_path" "$unresolved_suffix"
}

outside_product() {
  case "$2" in "$root"|"$root"/*) fail "$1 must be outside the checkout; existing files were preserved" ;; esac
}

prepare_execution_roots() {
  if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    checkout_key=$(printf '%s' "$root" | cksum | awk '{print $1}')
    CARGO_TARGET_DIR="${TMPDIR:-/tmp}/exitbind-ci-$checkout_key"
  fi
  CARGO_TARGET_DIR=$(canonical_destination "$CARGO_TARGET_DIR")
  outside_product CARGO_TARGET_DIR "$CARGO_TARGET_DIR"
  TMPDIR=$(canonical_destination "${TMPDIR:-$CARGO_TARGET_DIR/tmp}")
  outside_product TMPDIR "$TMPDIR"
  CI_LOG_ROOT=$(canonical_destination "${CI_LOG_ROOT:-$CARGO_TARGET_DIR/ci-local-runs}")
  outside_product CI_LOG_ROOT "$CI_LOG_ROOT"
  # Validate every destination before the first mkdir, including symlink parents.
  (umask 077; mkdir -p "$CARGO_TARGET_DIR" "$TMPDIR" "$CI_LOG_ROOT")
  export CARGO_TARGET_DIR TMPDIR CI_LOG_ROOT
}

show_execution_preflight() {
  printf 'preflight=ready\nroot=%s\ncargo=%s\ntarget=%s\ntemp=%s\nlogs=%s\n' "$root" "$CARGO" "$CARGO_TARGET_DIR" "$TMPDIR" "$CI_LOG_ROOT"
  "$CARGO" --version
  rustc --version
  df -Pk "$CARGO_TARGET_DIR" "$TMPDIR" "$CI_LOG_ROOT"
  if [ -r /proc/meminfo ]; then
    awk '/^(MemAvailable|SwapFree):/ {print}' /proc/meminfo
    cat /proc/loadavg
  fi
  printf 'build-jobs=%s\n' "${CARGO_BUILD_JOBS:-1}"
  printf 'observed-build-processes='
  ps -eo comm | awk '$1 == "cargo" || $1 == "rustc" {n++} END {print n+0}'
}
