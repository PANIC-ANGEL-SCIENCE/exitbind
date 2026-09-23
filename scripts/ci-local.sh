#!/bin/sh
# Shared native CI checks; run from any working directory.
set -eu

fail() { printf 'ci-local: %s\n' "$*" >&2; exit 1; }
checks_only=false
case "${1:-}" in
  '') test "$#" -eq 0 || fail 'usage: ci-local.sh [--checks-only]' ;;
  --checks-only) test "$#" -eq 1 || fail 'usage: ci-local.sh [--checks-only]'; checks_only=true ;;
  *) fail 'usage: ci-local.sh [--checks-only]' ;;
esac

# Honor an explicit Cargo, otherwise preserve PATH before trying its usual home.
if [ -n "${CARGO:-}" ]; then
  cargo_path=$(command -v "$CARGO") || fail 'CARGO must name an executable'
elif command -v cargo >/dev/null 2>&1; then
  cargo_path=$(command -v cargo)
else
  cargo_path=${CARGO_HOME:-$HOME/.cargo}/bin/cargo
fi
test -x "$cargo_path" || fail 'Cargo unavailable; set CARGO or add Cargo to PATH'
cargo_dir=$(CDPATH= cd -- "$(dirname -- "$cargo_path")" && pwd)
CARGO="$cargo_dir/$(basename -- "$cargo_path")"
export CARGO
PATH="$cargo_dir:$PATH"
export PATH
cd -- "$(dirname -- "$0")/.."
root=$(pwd -P)

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) platform=linux; target=x86_64-unknown-linux-gnu ;;
  Darwin:arm64) platform=macos; target=aarch64-apple-darwin ;;
  Darwin:x86_64) platform=macos; target=x86_64-apple-darwin ;;
  *) fail 'supported native hosts: Linux x86_64, macOS arm64 or x86_64' ;;
esac
command -v rustc >/dev/null 2>&1 || fail 'rustc unavailable; use the repository toolchain'
test "$(rustc -vV | sed -n 's/^host: //p')" = "$target" || fail 'Rust host does not match native CI target'
test -z "${CARGO_BUILD_TARGET:-}" || fail 'unset CARGO_BUILD_TARGET for native CI checks'
if [ "$platform" = linux ]; then
  command -v tmux >/dev/null 2>&1 || fail 'tmux is required for the native handoff check; install it first'
fi

# Keep Cargo and all binary-consuming scripts on the same output directory.
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$root/target}
case "$CARGO_TARGET_DIR" in /*) ;; *) CARGO_TARGET_DIR="$root/$CARGO_TARGET_DIR" ;; esac
export CARGO_TARGET_DIR

# Retain raw stage output independently of the caller's console/tee log.
# Git-private storage is excluded from product input fingerprints and commits.
log_root=$(git rev-parse --git-path ci-local-runs)
mkdir -p "$log_root"
log_root=$(CDPATH= cd -- "$log_root" && pwd -P)
run_dir=$(umask 077; mktemp -d "$log_root/run.XXXXXX")
current_stage=setup
owns_lock=false
finish() {
  result=$?
  trap - 0
  printf '%s\n' "$result" > "$run_dir/run.exit"
  printf 'stage=%s exit=%s\n' "$current_stage" "$result" > "$run_dir/state"
  if [ "$owns_lock" = true ]; then
    rm -f "$target_lock/owner" && rmdir "$target_lock" ||
      printf 'ci-local: could not remove own target lock: %s\n' "$target_lock" >&2
  fi
  printf 'ci-local: exit=%s stage=%s logs=%s\n' "$result" "$current_stage" "$run_dir"
  exit "$result"
}
trap finish 0
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP
printf 'ci-local: logs=%s\n' "$run_dir"
printf 'stage=setup running\n' > "$run_dir/state"

# Do not let two ci-local invocations or different worktrees mix this cache.
mkdir -p "$CARGO_TARGET_DIR"
CARGO_TARGET_DIR=$(CDPATH= cd -- "$CARGO_TARGET_DIR" && pwd -P)
export CARGO_TARGET_DIR
if [ -z "${TMPDIR:-}" ]; then
  case "$CARGO_TARGET_DIR" in
    "$root"|"$root"/*) TMPDIR=/tmp ;;
    *) TMPDIR="$CARGO_TARGET_DIR/tmp"; (umask 077; mkdir -p "$TMPDIR") ;;
  esac
fi
test -d "$TMPDIR" && test -w "$TMPDIR" || fail 'TMPDIR must be a writable directory'
TMPDIR=$(CDPATH= cd -- "$TMPDIR" && pwd -P)
case "$TMPDIR" in
  "$root"|"$root"/*) fail 'TMPDIR must be outside the checkout for native test fixtures' ;;
esac
export TMPDIR
EXITBIND_BIN="$CARGO_TARGET_DIR/debug/exitbind"
export EXITBIND_BIN
target_lock="$CARGO_TARGET_DIR/.ci-local-lock"
mkdir "$target_lock" || fail "cannot acquire target lock: $target_lock; inspect its owner and filesystem before retrying"
owns_lock=true
printf 'pid=%s\nroot=%s\nlogs=%s\n' "$$" "$root" "$run_dir" > "$target_lock/owner"
cache_owner="$CARGO_TARGET_DIR/.ci-local-checkout"
if [ -e "$cache_owner" ]; then
  previous_root=$(cat "$cache_owner")
  test "$previous_root" = "$root" || fail 'target belongs to another checkout; choose a separate CARGO_TARGET_DIR (existing cache retained)'
else
  printf '%s\n' "$root" > "$cache_owner"
fi
{
  printf 'head=%s\nroot=%s\ntarget=%s\ncandidate=%s\n' "$(git rev-parse HEAD)" "$root" "$CARGO_TARGET_DIR" "${CANDIDATE_SHA:-HEAD}"
  "$CARGO" --version
  rustc --version
  git status --porcelain --untracked-files=normal
} > "$run_dir/context"

run() {
  current_stage=$1
  printf 'stage=%s running\n' "$current_stage" > "$run_dir/state"
  printf 'start: %s; log: %s/%s.log\n' "$current_stage" "$run_dir" "$current_stage"
  started=$(date +%s)
  if QUIET_COMMAND_LOG="$run_dir/$current_stage.log" ./scripts/quiet-command.sh "$@"; then
    stage_status=0
  else
    stage_status=$?
  fi
  printf '%s\n' "$stage_status" > "$run_dir/$current_stage.log.exit"
  printf 'stage=%s exit=%s seconds=%s\n' "$current_stage" "$stage_status" "$(($(date +%s) - started))" >> "$run_dir/stages"
  test "$stage_status" -eq 0 || exit "$stage_status"
}

run history ./scripts/check-public-history.sh "${CANDIDATE_SHA:-HEAD}"
VALUE_PROOF_BASE=${VALUE_PROOF_BASE:-}
if [ -z "$VALUE_PROOF_BASE" ] || [ "$VALUE_PROOF_BASE" = "0000000000000000000000000000000000000000" ]; then
  VALUE_PROOF_BASE=$(git rev-parse --verify HEAD^)
elif ! git cat-file -e "${VALUE_PROOF_BASE}^{commit}"; then
  git fetch --no-tags origin "$VALUE_PROOF_BASE"
fi
git cat-file -e "${VALUE_PROOF_BASE}^{commit}"
export VALUE_PROOF_BASE
printf 'proof-base=%s\n' "$VALUE_PROOF_BASE" >> "$run_dir/context"
printf 'ci-local: platform=%s head=%s proof-base=%s\n' "$platform" "$(git rev-parse HEAD)" "$VALUE_PROOF_BASE"

if [ "$platform" = linux ]; then
  run fmt "$CARGO" fmt --check
  run path-rendering ./scripts/check-path-rendering.sh
  run product-surface ./scripts/check-product-surface.sh
  run clippy "$CARGO" clippy --locked --all-targets -- -D warnings
fi
run tests "$CARGO" test --locked
run value-proof ./scripts/run-value-proof-suite.sh
if [ "$platform" = linux ]; then
  run drift-warning ./scripts/demo-drift-warning.sh
  run native-handoff "$CARGO" test --locked --test away_native real_tmux_child_presents_bound_evidence_without_persisting_the_prompt -- --ignored --exact
  run release-refs ./scripts/check-release-refs.sh
fi

if [ "$checks_only" = true ]; then
  current_stage=checks-complete
  exit 0
fi
EXITBIND_BUILD_COMMIT=${GITHUB_SHA:-$(git rev-parse HEAD)}
export EXITBIND_BUILD_COMMIT
if [ "$platform" = linux ]; then
  run release-build "$CARGO" build --release --locked
  release_bin="$CARGO_TARGET_DIR/release/exitbind"
  run onboarding ./scripts/onboarding-smoke.sh "$release_bin" "$root/skills/exitbind/SKILL.md"
else
  run release-build "$CARGO" build --release --locked --target "$target"
  release_bin="$CARGO_TARGET_DIR/$target/release/exitbind"
fi
package_dir="$run_dir/dist"
run package ./scripts/package-release.sh "$target" "$release_bin" "$package_dir"
run installer ./scripts/installer-smoke.sh "$package_dir" "$target"
current_stage=complete
