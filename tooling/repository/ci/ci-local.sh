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

# Full mode prepares the comparison base before stage 0: the guard reads the
# base, so a commit the checkout does not have yet (a force-replaced branch tip
# on public CI) must be fetched by prepare_value_proof_base first. Pre-return
# passes its declared base unchanged and prepares only when VALUE_PROOF_BASE is
# set, as before.
if [ "$ci_mode" = full ] || [ -n "${VALUE_PROOF_BASE:-}" ]; then
  prepare_value_proof_base
fi

# Track only successful invariant targets in this invocation. The affected
# selector can omit an exact target/features pair already run here; nothing is
# persisted or reused from another CI run.
passed_targets=
remember_passed_target() {
  # Match the affected selector's requirement that at least one test passed.
  # A successful command with zero tests must still reach that selector.
  passed_count=$(sed -n 's/^test result: [A-Za-z]*\. \([0-9][0-9]*\) passed;.*/\1/p' \
    "$run_dir/$current_stage.log" | awk '{n += $1} END {print n + 0}') || return 0
  test "$passed_count" -gt 0 || return 0
  # Unknown argument shapes are not reusable and therefore get rerun.
  if [ "$#" -eq 6 ] && [ "$1" = test ] && [ "$2" = --locked ] &&
    [ "$3" = --features ] && [ "$5" = --test ]; then
    passed_pair="$6=$4"
  elif [ "$#" -eq 4 ] && [ "$1" = test ] && [ "$2" = --locked ] && [ "$3" = --test ]; then
    passed_pair="$4="
  else
    return 0
  fi
  passed_targets="${passed_targets:+$passed_targets }$passed_pair"
}

for stage in $(ci_stage_order); do
  ci_stage_selected "$ci_mode" "$stage" "$platform" || continue
  case "$stage" in
    repository-rules)
      if [ "$ci_mode" = pre-return ]; then
        rules_base=${PRE_RETURN_BASE:-${VALUE_PROOF_BASE:-}}
        test -n "$rules_base" || rules_base=origin/main
      else
        rules_base=$VALUE_PROOF_BASE
      fi
      run repository-rules sh ./tooling/repository/ci/check-repository-rules.sh "$rules_base" "$platform" "$ci_mode"
      ;;
    workflows) run workflows ./scripts/check-workflows.sh ;;
    workflow-regression) run workflow-regression ./scripts/test-workflow-validation.sh ;;
    ci-stage-contract) run ci-stage-contract ./scripts/test-ci-stages.sh ;;
    quiet-command-regression) run quiet-command-regression python3 ./scripts/test-quiet-command.py ;;
    fmt) run fmt "$CARGO" fmt --check ;;
    clippy) run clippy "$CARGO" clippy --locked --all-targets --features repository-checks -- -D warnings ;;
    tests) run tests "$CARGO" test --locked --features repository-checks ;;
    architecture-budget)
      set -- test --locked --features repository-checks --test architecture_budget
      run architecture-budget "$CARGO" "$@"
      remember_passed_target "$@" ;;
    privacy-gate)
      set -- test --locked --features repository-checks --test privacy_gate
      run privacy-gate "$CARGO" "$@"
      remember_passed_target "$@" ;;
    release-integrity)
      set -- test --locked --features repository-checks --test release_integrity
      run release-integrity "$CARGO" "$@"
      remember_passed_target "$@" ;;
    docs-workflow)
      set -- test --locked --features repository-checks --test docs_workflow
      run docs-workflow "$CARGO" "$@"
      remember_passed_target "$@" ;;
    first-impression)
      set -- test --locked --test first_impression
      run first-impression "$CARGO" "$@"
      remember_passed_target "$@" ;;
    value-contract)
      set -- test --locked --test value_contract
      run value-contract "$CARGO" "$@"
      remember_passed_target "$@" ;;
    plugin-distribution)
      set -- test --locked --features repository-checks --test plugin_distribution
      run plugin-distribution "$CARGO" "$@"
      remember_passed_target "$@" ;;
    affected-tests)
      affected_base=${PRE_RETURN_BASE:-${VALUE_PROOF_BASE:-}}
      test -n "$affected_base" || fail 'affected-tests needs PRE_RETURN_BASE or VALUE_PROOF_BASE'
      run affected-tests sh ./tooling/repository/ci/test-affected.sh --base "$affected_base" --skip-passed "$passed_targets"
      ;;
    value-proof) run value-proof ./scripts/run-value-proof-suite.sh ;;
    drift-warning) run drift-warning ./scripts/demo-drift-warning.sh ;;
    native-handoff)
      run native-handoff "$CARGO" test --locked --test away_native real_tmux_child_presents_bound_evidence_without_persisting_the_prompt -- --ignored --exact
      ./scripts/assert-test-ran.sh real_tmux_child_presents_bound_evidence_without_persisting_the_prompt < "$run_dir/native-handoff.log" || fail 'native-handoff target did not execute and pass'
      ;;
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
