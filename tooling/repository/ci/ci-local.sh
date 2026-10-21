#!/bin/sh
# Shared native CI checks; run from any working directory.
set -eu

fail() { printf 'ci-local: %s\n' "$*" >&2; exit 1; }
checks_only=false
preflight_only=false
ci_mode=full
case "${1:-}" in
  '') test "$#" -eq 0 || fail 'usage: ci-local.sh [--checks-only|--preflight|--pre-return]' ;;
  --checks-only) test "$#" -eq 1 || fail 'usage: ci-local.sh [--checks-only|--preflight|--pre-return]'; checks_only=true ;;
  --preflight) test "$#" -eq 1 || fail 'usage: ci-local.sh [--checks-only|--preflight|--pre-return]'; preflight_only=true ;;
  --pre-return) test "$#" -eq 1 || fail 'usage: ci-local.sh [--checks-only|--preflight|--pre-return]'; ci_mode=pre-return ;;
  *) fail 'usage: ci-local.sh [--checks-only|--preflight|--pre-return]' ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
cd "$root"
. "$root/tooling/repository/ci/verification-context.sh"
. "$root/tooling/repository/ci/ci-stages.sh"
prepare_verification_context "$root"

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

if [ "$preflight_only" = true ]; then
  show_execution_preflight
  exit 0
fi

begin_verification_run

# A value that is neither empty, all zeros nor a full commit SHA names a ref;
# it is resolved once to the merge-base with HEAD. A SHA is used unchanged.
proof_base_is_ref() {
  case $1 in
    '') return 1 ;;
    *[!0-9a-fA-F]*) return 0 ;;
  esac
  test "${#1}" -ne 40
}

prepare_value_proof_base() {
  VALUE_PROOF_BASE=${VALUE_PROOF_BASE:-}
  if [ -z "$VALUE_PROOF_BASE" ] || [ "$VALUE_PROOF_BASE" = "0000000000000000000000000000000000000000" ]; then
    VALUE_PROOF_BASE=$(git rev-parse --verify HEAD^)
  elif ! git cat-file -e "${VALUE_PROOF_BASE}^{commit}"; then
    git fetch --no-tags origin "$VALUE_PROOF_BASE"
  fi
  git cat-file -e "${VALUE_PROOF_BASE}^{commit}"
  if proof_base_is_ref "$VALUE_PROOF_BASE"; then
    VALUE_PROOF_BASE=$(git merge-base HEAD "$VALUE_PROOF_BASE")
  fi
  export VALUE_PROOF_BASE
  printf 'proof-base=%s\n' "$VALUE_PROOF_BASE" >> "$run_dir/context"
  printf 'ci-local: platform=%s head=%s proof-base=%s\n' "$platform" "$(git rev-parse HEAD)" "$VALUE_PROOF_BASE"
}

if [ "$ci_mode" = pre-return ]; then
  printf 'ci-local: platform=%s head=%s mode=%s\n' "$platform" "$(git rev-parse HEAD)" "$ci_mode"
fi

proof_base_ready=false
if [ -n "${VALUE_PROOF_BASE:-}" ] && { [ "$ci_mode" = pre-return ] || proof_base_is_ref "$VALUE_PROOF_BASE"; }; then
  prepare_value_proof_base
  proof_base_ready=true
fi
for stage in $(ci_stage_order); do
  ci_stage_selected "$ci_mode" "$stage" "$platform" || continue
  if [ "$ci_mode" = full ] && [ "$proof_base_ready" = false ] && { [ "$stage" = fmt ] || [ "$stage" = tests ]; }; then
    prepare_value_proof_base
    proof_base_ready=true
  fi
  case "$stage" in
    history) run history ./scripts/check-public-history.sh "${CANDIDATE_SHA:-HEAD}" ;;
    workflows) run workflows ./scripts/check-workflows.sh ;;
    workflow-regression) run workflow-regression ./scripts/test-workflow-validation.sh ;;
    ci-stage-contract) run ci-stage-contract ./scripts/test-ci-stages.sh ;;
    quiet-command-regression) run quiet-command-regression python3 ./scripts/test-quiet-command.py ;;
    fmt) run fmt "$CARGO" fmt --check ;;
    path-rendering) run path-rendering ./scripts/check-path-rendering.sh ;;
    product-surface) run product-surface ./scripts/check-product-surface.sh ;;
    clippy) run clippy "$CARGO" clippy --locked --all-targets --features repository-checks -- -D warnings ;;
    tests) run tests "$CARGO" test --locked --features repository-checks ;;
    architecture-budget) run architecture-budget "$CARGO" test --locked --features repository-checks --test architecture_budget ;;
    privacy-gate) run privacy-gate "$CARGO" test --locked --features repository-checks --test privacy_gate ;;
    release-integrity) run release-integrity "$CARGO" test --locked --features repository-checks --test release_integrity ;;
    docs-workflow) run docs-workflow "$CARGO" test --locked --features repository-checks --test docs_workflow ;;
    first-impression) run first-impression "$CARGO" test --locked --test first_impression ;;
    value-contract) run value-contract "$CARGO" test --locked --test value_contract ;;
    plugin-distribution) run plugin-distribution "$CARGO" test --locked --features repository-checks --test plugin_distribution ;;
    affected-tests)
      affected_base=${PRE_RETURN_BASE:-${VALUE_PROOF_BASE:-}}
      test -n "$affected_base" || fail 'affected-tests needs PRE_RETURN_BASE or VALUE_PROOF_BASE'
      run affected-tests sh ./tooling/repository/ci/test-affected.sh --base "$affected_base"
      ;;
    value-proof) run value-proof ./scripts/run-value-proof-suite.sh ;;
    drift-warning) run drift-warning ./scripts/demo-drift-warning.sh ;;
    native-handoff)
      run native-handoff "$CARGO" test --locked --test away_native real_tmux_child_presents_bound_evidence_without_persisting_the_prompt -- --ignored --exact
      ./scripts/assert-test-ran.sh real_tmux_child_presents_bound_evidence_without_persisting_the_prompt < "$run_dir/native-handoff.log" || fail 'native-handoff target did not execute and pass'
      ;;
    release-refs) run release-refs ./scripts/check-release-refs.sh ;;
    *) fail "selected stage has no command: $stage" ;;
  esac
done

if [ "$checks_only" = true ]; then
  current_stage=checks-complete
  exit 0
fi
if [ "$ci_mode" = pre-return ]; then
  current_stage=pre-return-complete
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
