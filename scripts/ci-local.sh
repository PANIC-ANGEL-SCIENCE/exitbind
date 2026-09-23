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
EXITBIND_BIN="$CARGO_TARGET_DIR/debug/exitbind"
export EXITBIND_BIN

run() { ./scripts/quiet-command.sh "$@"; }
run history ./scripts/check-public-history.sh "${CANDIDATE_SHA:-HEAD}"
VALUE_PROOF_BASE=${VALUE_PROOF_BASE:-}
if [ -z "$VALUE_PROOF_BASE" ] || [ "$VALUE_PROOF_BASE" = "0000000000000000000000000000000000000000" ]; then
  VALUE_PROOF_BASE=$(git rev-parse --verify HEAD^)
elif ! git cat-file -e "${VALUE_PROOF_BASE}^{commit}"; then
  git fetch --no-tags origin "$VALUE_PROOF_BASE"
fi
git cat-file -e "${VALUE_PROOF_BASE}^{commit}"
export VALUE_PROOF_BASE
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
  run refusal ./scripts/demo-refusal.sh
  run native-handoff "$CARGO" test --locked --test away_native real_tmux_child_presents_bound_evidence_without_persisting_the_prompt -- --ignored --exact
  run release-refs ./scripts/check-release-refs.sh
fi

if [ "$checks_only" = true ]; then
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
run package ./scripts/package-release.sh "$target" "$release_bin" dist
run installer ./scripts/installer-smoke.sh dist "$target"
