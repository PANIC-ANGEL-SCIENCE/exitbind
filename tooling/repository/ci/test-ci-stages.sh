#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
cd "$root"
. "$root/tooling/repository/ci/ci-stages.sh"

fail() {
  printf 'ci-stage contract: %s\n' "$*" >&2
  exit 1
}

# Public CI checks out a commit, not a branch (actions/checkout receives a SHA),
# so every fixture repo takes that shape: detached HEAD, no refs/heads. A
# fixture that needs a branch or a symbolic HEAD then fails here, not on a
# runner.
detach_without_branches() {
  git -C "$1" checkout -q --detach "$(git -C "$1" rev-parse HEAD)" ||
    fail "fixture repo could not detach: $1"
  git -C "$1" for-each-ref --format='delete %(refname)' refs/heads |
    git -C "$1" update-ref --stdin || fail "fixture repo kept its branches: $1"
  test -z "$(git -C "$1" for-each-ref refs/heads)" ||
    fail "fixture repo kept local branches: $1"
  if git -C "$1" symbolic-ref -q HEAD >/dev/null; then
    fail "fixture HEAD is still symbolic: $1"
  fi
}

selected_stages() {
  mode=$1
  platform=$2
  for stage in $(ci_stage_order); do
    if ci_stage_selected "$mode" "$stage" "$platform"; then
      printf '%s\n' "$stage"
    fi
  done
}

expect() {
  mode=$1
  platform=$2
  expected=$3
  actual=$(selected_stages "$mode" "$platform")
  test "$actual" = "$expected" || {
    printf 'mode=%s platform=%s\nexpected:\n%s\nactual:\n%s\n' \
      "$mode" "$platform" "$expected" "$actual" >&2
    fail 'stage membership drifted'
  }
}

pre_return_common='repository-rules
workflows
workflow-regression
ci-stage-contract
quiet-command-regression'
pre_return_linux="$pre_return_common
fmt
clippy
architecture-budget
privacy-gate
release-integrity
docs-workflow
first-impression
value-contract
plugin-distribution
affected-tests"
pre_return_macos="$pre_return_common
architecture-budget
privacy-gate
release-integrity
docs-workflow
first-impression
value-contract
plugin-distribution
affected-tests"
full_linux='repository-rules
workflows
workflow-regression
ci-stage-contract
quiet-command-regression
fmt
clippy
tests
value-proof
drift-warning
native-handoff'
full_macos='repository-rules
workflows
workflow-regression
ci-stage-contract
quiet-command-regression
tests
value-proof'

expect pre-return linux "$pre_return_linux"
expect pre-return macos "$pre_return_macos"
expect full linux "$full_linux"
expect full macos "$full_macos"

# Shared stages run in every mode on every platform, including the stage that
# runs the inventory checks below: a local branch run proves only one cell.
for stage in repository-rules workflows workflow-regression ci-stage-contract quiet-command-regression; do
  for mode in full pre-return; do
    for platform in linux macos; do
      ci_stage_selected "$mode" "$stage" "$platform" ||
        fail "shared stage omitted: $stage $mode $platform"
    done
  done
done
for stage in tests value-proof drift-warning native-handoff; do
  if ci_stage_selected pre-return "$stage" linux; then
    fail "full-only stage selected for pre-return: $stage"
  fi
done
for stage in architecture-budget privacy-gate release-integrity docs-workflow first-impression value-contract plugin-distribution affected-tests; do
  ci_stage_selected pre-return "$stage" linux || fail "pre-return invariant omitted: $stage"
  if ci_stage_selected full "$stage" linux; then
    fail "invariant target reruns in full mode: $stage"
  fi
done
# The four former stages now run inside stage 0, never as stages of their own.
for retired in history path-rendering product-surface release-refs; do
  for mode in full pre-return; do
    for platform in linux macos; do
      if ci_stage_selected "$mode" "$retired" "$platform"; then
        fail "retired stage still selected: $retired $mode $platform"
      fi
    done
  done
  if grep -F "    $retired)" "$root/tooling/repository/ci/ci-local.sh" >/dev/null; then
    fail "retired stage still has a command dispatch: $retired"
  fi
done
# Every check stage 0 runs keeps the applicability its former stage had, in all
# four mode/platform cells. A single sample per check let a full-mode Linux size
# scan and a pre-return-only rule both drift unseen.
check_selected() {
  check=$1
  platform=$2
  mode=$3
  want=$4
  if ci_repository_rule_check_selected "$check" "$platform" "$mode"; then
    got=yes
  else
    got=no
  fi
  test "$got" = "$want" ||
    fail "stage 0 check selection changed: $check $platform $mode expected $want got $got"
}
check_matrix='architecture-budget linux pre-return yes
architecture-budget linux full yes
architecture-budget macos pre-return yes
architecture-budget macos full no
path-rendering linux pre-return yes
path-rendering linux full yes
path-rendering macos pre-return no
path-rendering macos full no
product-surface linux pre-return yes
product-surface linux full yes
product-surface macos pre-return no
product-surface macos full no
release-refs linux pre-return yes
release-refs linux full yes
release-refs macos pre-return no
release-refs macos full no
public-history linux pre-return yes
public-history linux full yes
public-history macos pre-return yes
public-history macos full yes
commit-identity linux pre-return yes
commit-identity linux full yes
commit-identity macos pre-return yes
commit-identity macos full yes'
matrix_rows=$(printf '%s\n' "$check_matrix" | wc -l | tr -d ' ')
test "$matrix_rows" -eq 24 || fail "stage 0 matrix must cover every check in every cell: $matrix_rows rows"
while read -r check platform mode want; do
  check_selected "$check" "$platform" "$mode" "$want"
done <<EOF
$check_matrix
EOF
# The guard's dispatch is the inventory of checks that exist: a new check must
# gain its four rows above before the guard can run it.
guard_checks=$(sed -n 's/^    \([a-z][a-z-]*\)) step .*/\1/p' \
  "$root/tooling/repository/ci/check-repository-rules.sh" | sort)
matrix_checks=$(printf '%s\n' "$check_matrix" | awk '{ print $1 }' | sort -u)
test -n "$guard_checks" && test "$guard_checks" = "$matrix_checks" ||
  fail "stage 0 matrix does not cover the guard's dispatch: $matrix_checks"
if ci_repository_rule_check_selected make-coffee linux pre-return; then
  fail 'an unknown stage 0 check was selected'
fi
if ci_stage_selected pre-return unknown linux; then
  fail 'unknown stage accepted'
fi
for stage in $(ci_stage_order); do
  grep -F "    $stage)" "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
    fail "selected stage has no command dispatch: $stage"
done
# The no-build guard owns stage 0: it stays first and runs before any cargo
# stage, so a repository-rule violation fails in seconds, not after a build.
first_stage=$(ci_stage_order | sed -n '1p')
test "$first_stage" = repository-rules ||
  fail 'stage 0 must be the no-build repository-rule guard'
rules_line=$(grep -n -F 'run repository-rules' "$root/tooling/repository/ci/ci-local.sh" | head -n 1 | cut -d: -f1)
cargo_line=$(grep -n -F '"$CARGO"' "$root/tooling/repository/ci/ci-local.sh" | head -n 1 | cut -d: -f1)
test -n "$rules_line" && test -n "$cargo_line" && test "$rules_line" -lt "$cargo_line" ||
  fail 'the no-build guard must run before the first cargo stage'
# Full mode prepares the comparison base before stage 0: on public CI the push
# base can be a commit this checkout does not have (a force-replaced branch
# tip), and prepare_value_proof_base fetches it before the guard reads it. The
# candidate-history fixture below proves that order by running the real ci-local
# with no VALUE_PROOF_BASE, so a source reorder cannot satisfy this check.
grep -F 'run repository-rules sh ./tooling/repository/ci/check-repository-rules.sh "$rules_base" "$platform" "$ci_mode"' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'stage 0 must receive the prepared base, platform and mode'
grep -F 'ci_repository_rule_check_selected' \
  "$root/tooling/repository/ci/check-repository-rules.sh" >/dev/null ||
  fail 'stage 0 must keep each former stage platform selection'
python3 - "$root" <<'PY' || fail 'repository-rules must be the first inventory gate with cost light'
import json, sys
from pathlib import Path
gates = json.loads((Path(sys.argv[1]) / "tooling/repository/ci/gate-inventory.json").read_text(encoding="utf-8"))["gates"]
first = gates[0]
assert first["id"] == "repository-rules", first
assert first.get("cost") == "light", first
PY
guard="$root/tooling/repository/ci/check-repository-rules.sh"
for check in check_architecture_budget.py check_commit_identity.py check-path-rendering.sh \
  check-product-surface.sh check-release-refs.sh check-public-history.sh; do
  grep -F "$check" "$guard" >/dev/null || fail "the no-build guard no longer runs $check"
done
grep -F 'public-history) step public-history ./scripts/check-public-history.sh HEAD ${CANDIDATE_SHA:+"$CANDIDATE_SHA"} ;;' \
  "$guard" >/dev/null ||
  fail 'stage 0 must forward CANDIDATE_SHA to the public history check'
python3 "$root/tooling/repository/checks/check_architecture_budget.py" --self-test ||
  fail 'architecture budget scan self-test failed'
python3 "$root/tooling/repository/checks/check_commit_identity.py" --self-test ||
  fail 'commit identity scan self-test failed'
grep -F 'plugin-distribution) run plugin-distribution "$CARGO" test --locked --features repository-checks --test plugin_distribution ;;' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'plugin distribution tests must execute behind the repository-checks feature'
grep -F 'run affected-tests sh ./tooling/repository/ci/test-affected.sh --base "$affected_base"' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'pre-return must run the affected test targets with their declared features'
grep -F 'affected_base=${PRE_RETURN_BASE:-${VALUE_PROOF_BASE:-}}' \
  "$root/tooling/repository/ci/ci-local.sh" >/dev/null ||
  fail 'affected tests must default to PRE_RETURN_BASE, then VALUE_PROOF_BASE'
grep -F 'run focused "$CARGO" test --locked --features repository-checks "$@"' \
  "$root/tooling/repository/ci/test-focused.sh" >/dev/null ||
  fail 'focused tests must include repository-only contract targets'

proof_base_functions=$(sed -n \
  -e '/^proof_base_is_ref() {$/,/^}$/p' \
  -e '/^prepare_value_proof_base() {$/,/^}$/p' \
  "$root/tooling/repository/ci/ci-local.sh")
case $proof_base_functions in
  *'proof_base_is_ref() {'*'prepare_value_proof_base() {'*) ;;
  *) fail 'ci-local.sh proof-base preparation functions not found' ;;
esac
proof_work=$(mktemp -d "${TMPDIR:-/tmp}/ci-proof-base.XXXXXX")
trap 'rm -rf "$proof_work"' EXIT HUP INT TERM
git init -q --bare "$proof_work/remote.git"
git -C "$proof_work/remote.git" symbolic-ref HEAD refs/heads/main
git -C "$proof_work/remote.git" config uploadpack.allowAnySHA1InWant true
git init -q "$proof_work/seed"
git -C "$proof_work/seed" config user.name 'CI proof-base fixture'
git -C "$proof_work/seed" config user.email 'ci-proof-base@example.invalid'
git -C "$proof_work/seed" commit -q --allow-empty -m fork
fork=$(git -C "$proof_work/seed" rev-parse HEAD)
git -C "$proof_work/seed" commit -q --allow-empty -m main-moved
main_tip=$(git -C "$proof_work/seed" rev-parse HEAD)
git -C "$proof_work/seed" remote add origin "$proof_work/remote.git"
git -C "$proof_work/seed" push -q origin HEAD:refs/heads/main
git -C "$proof_work/seed" checkout -q --detach "$fork"
git -C "$proof_work/seed" commit -q --allow-empty -m candidate
git -C "$proof_work/seed" push -q origin HEAD:refs/heads/candidate
# The subject under test is a clone in the CI shape: detached, no local branches.
git clone -q "$proof_work/remote.git" "$proof_work/source"
git -C "$proof_work/source" checkout -q --detach origin/candidate
detach_without_branches "$proof_work/source"
git -C "$proof_work/source" update-ref refs/remotes/pub/main "$main_tip"
mkdir "$proof_work/run"
# A fresh shell keeps set -e in force even when the caller tests the status.
prepare_proof_base() {
  (
    cd "$proof_work/source"
    VALUE_PROOF_BASE=$1 run_dir=$proof_work/run platform=fixture \
      PROOF_BASE_FUNCTIONS=$proof_base_functions \
      sh -eu -c 'eval "$PROOF_BASE_FUNCTIONS"; prepare_value_proof_base'
  )
}
resolved=$(prepare_proof_base pub/main 2>&1) || fail "a ref proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "a ref proof base must resolve to its merge-base $fork: $resolved" ;;
esac
resolved=$(prepare_proof_base "$fork" 2>&1) || fail "a SHA proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "a SHA proof base must be used unchanged: $resolved" ;;
esac
# An absent base is HEAD^: this is the path a push event with no declared base
# takes, and the detached checkout shape must still have a first parent.
resolved=$(prepare_proof_base '' 2>&1) || fail "an absent proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "an absent proof base must resolve to HEAD^ $fork: $resolved" ;;
esac
# A base this checkout does not have yet is fetched before stage 0 reads it:
# a second clone advances the remote branch, and the first clone cannot see
# that commit until prepare_value_proof_base fetches it.
git clone -q "$proof_work/remote.git" "$proof_work/second"
git -C "$proof_work/second" config user.name 'CI proof-base fixture'
git -C "$proof_work/second" config user.email 'ci-proof-base@example.invalid'
git -C "$proof_work/second" checkout -q --detach origin/candidate
git -C "$proof_work/second" commit -q --allow-empty -m later
git -C "$proof_work/second" push -q origin HEAD:refs/heads/candidate
detach_without_branches "$proof_work/second"
later=$(git -C "$proof_work/second" rev-parse HEAD)
if git -C "$proof_work/source" cat-file -e "${later}^{commit}" 2>/dev/null; then
  fail 'the fixture base is already present before the fetch case'
fi
resolved=$(prepare_proof_base "$later" 2>&1) || fail "an unfetched base must be fetched: $resolved"
case $resolved in
  *"proof-base=$later") ;;
  *) fail "an unfetched base must resolve to itself after the fetch: $resolved" ;;
esac
git -C "$proof_work/source" cat-file -e "${later}^{commit}" 2>/dev/null ||
  fail 'prepare_value_proof_base did not fetch the missing base'
if resolved=$(prepare_proof_base no-such-ref 2>&1); then
  fail "an unknown ref proof base must fail: $resolved"
fi
rm -rf "$proof_work"
trap - EXIT HUP INT TERM

# A pull-request head can drop published history while the merge keeps it. The
# retired history stage checked CANDIDATE_SHA; stage 0 must forward it, in both
# modes, from the checkout shape public CI actually has. The fixture is a
# detached, branchless clone; it runs the real ci-local twice, so the base the
# guard receives is prepared by ci-local itself, not by the fixture. Its commits
# use the canonical identity the guard reads from tests/privacy_gate.rs, so only
# the candidate's history can fail. Fixture paths stay lowercase: the release
# macOS runner is case-insensitive, and a case-only difference in a path cannot
# be proven there.
identity_name=$(sed -n 's/^const ALLOWED_NAME: &str = "\(.*\)";$/\1/p' "$root/tests/privacy_gate.rs")
identity_email=$(sed -n 's/^const ALLOWED_EMAIL: &str = "\(.*\)";$/\1/p' "$root/tests/privacy_gate.rs")
test -n "$identity_name" && test -n "$identity_email" ||
  fail 'cannot read the canonical commit identity from tests/privacy_gate.rs'
case "$(uname -s)" in
  Darwin) host_platform=macos ;;
  *) host_platform=linux ;;
esac
history_work=$(mktemp -d "${TMPDIR:-/tmp}/ci-candidate-history.XXXXXX")
trap 'rm -rf "$history_work"' EXIT HUP INT TERM
git clone -q "$root" "$history_work/repo" || fail 'candidate history fixture clone failed'
start=$(git -C "$history_work/repo" rev-parse HEAD)
detach_without_branches "$history_work/repo"
git -C "$history_work/repo" config user.name "$identity_name"
git -C "$history_work/repo" config user.email "$identity_email"
git -C "$history_work/repo" checkout -q --orphan candidate-orphan
git -C "$history_work/repo" commit -q -m 'fixture orphan head'
orphan=$(git -C "$history_work/repo" rev-parse HEAD)
git -C "$history_work/repo" checkout -q --detach "$start"
git -C "$history_work/repo" update-ref -d refs/heads/candidate-orphan
test -z "$(git -C "$history_work/repo" for-each-ref refs/heads)" ||
  fail 'candidate history fixture kept local branches'
git -C "$history_work/repo" merge -q -s ours --allow-unrelated-histories \
  -m 'fixture merge' "$orphan"
merge=$(git -C "$history_work/repo" rev-parse HEAD)
if git -C "$history_work/repo" symbolic-ref -q HEAD >/dev/null; then
  fail 'candidate history fixture HEAD is not detached'
fi
# The clone carries committed HEAD; overlay the files under test from the
# working tree so a local, not-yet-committed edit is what runs here too.
for file in check-repository-rules.sh ci-stages.sh ci-local.sh \
  verification-context.sh ci-preflight.sh; do
  cp "$root/tooling/repository/ci/$file" "$history_work/repo/tooling/repository/ci/$file"
done
# The merge itself keeps published history; only the contributing head drops it.
# The control is the one check, not a second guard run: the executed runs below
# already select and run it in both modes.
if ! (cd "$history_work/repo" && sh scripts/check-public-history.sh HEAD) \
  > "$history_work/retained.out" 2>&1; then
  fail "the merge commit itself must retain public history: $(tail -n 3 "$history_work/retained.out")"
fi
# Full mode with an absent base and a pull-request head, executed through
# ci-local's own ordering: prepare_value_proof_base must run first (HEAD^ is the
# merge's first parent), stage 0 must receive that base and CANDIDATE_SHA, and
# the run must stop before any cargo stage.
if (cd "$history_work/repo" &&
  unset VALUE_PROOF_BASE PRE_RETURN_BASE &&
  CARGO_TARGET_DIR="$history_work/target" TMPDIR="$history_work/tmp" \
    CI_LOG_ROOT="$history_work/logs" CANDIDATE_SHA="$orphan" \
    sh tooling/repository/ci/ci-local.sh --checks-only) > "$history_work/full.out" 2>&1; then
  fail 'full mode accepted a candidate that dropped public history'
fi
proof_line=$(grep -n -F "proof-base=$start" "$history_work/full.out" | head -n 1 | cut -d: -f1)
rules_line=$(grep -n -F 'start: repository-rules;' "$history_work/full.out" | head -n 1 | cut -d: -f1)
test -n "$proof_line" && test -n "$rules_line" && test "$proof_line" -lt "$rules_line" ||
  fail 'full mode must prepare the comparison base before stage 0 runs'
grep -F "ci-local: platform=$host_platform head=$merge proof-base=$start" "$history_work/full.out" >/dev/null ||
  fail 'full mode did not resolve the absent base to HEAD^ (the merge first parent)'
grep -F 'ci-local: exit=1 stage=repository-rules' "$history_work/full.out" >/dev/null ||
  fail 'full mode did not stop at stage 0 for a candidate that dropped history'
if grep -F 'start: fmt;' "$history_work/full.out" >/dev/null; then
  fail 'full mode reached a cargo stage after stage 0 failed'
fi
full_log=$(sed -n 's/^ci-local: exit=1 stage=repository-rules logs=//p' "$history_work/full.out" | head -n 1)
test -d "$full_log" || fail 'full mode produced no ci-local log directory'
grep -F "base=$start platform=$host_platform mode=full" "$full_log/repository-rules.log" >/dev/null ||
  fail 'full mode did not pass the prepared base to stage 0'
grep -F 'FAIL public-history' "$full_log/repository-rules.log" >/dev/null &&
  grep -F "$orphan does not retain published checkpoint" "$full_log/repository-rules.log" >/dev/null ||
  fail 'full mode did not check the pull-request head for public history'
# Pre-return with an absent VALUE_PROOF_BASE must not prepare one: it passes the
# declared PRE_RETURN_BASE to stage 0 and checks the same candidate.
if (cd "$history_work/repo" &&
  unset VALUE_PROOF_BASE &&
  CARGO_TARGET_DIR="$history_work/target" TMPDIR="$history_work/tmp" \
    CI_LOG_ROOT="$history_work/logs" CANDIDATE_SHA="$orphan" PRE_RETURN_BASE="$start" \
    sh tooling/repository/ci/ci-local.sh --pre-return) > "$history_work/pre-return.out" 2>&1; then
  fail 'pre-return accepted a candidate that dropped public history'
fi
if grep -F 'proof-base=' "$history_work/pre-return.out" >/dev/null; then
  fail 'pre-return must not prepare a base when VALUE_PROOF_BASE is absent'
fi
grep -F 'ci-local: exit=1 stage=repository-rules' "$history_work/pre-return.out" >/dev/null ||
  fail 'pre-return did not stop at stage 0 for a candidate that dropped history'
if grep -F 'start: fmt;' "$history_work/pre-return.out" >/dev/null; then
  fail 'pre-return reached a cargo stage after stage 0 failed'
fi
pre_log=$(sed -n 's/^ci-local: exit=1 stage=repository-rules logs=//p' "$history_work/pre-return.out" | head -n 1)
test -d "$pre_log" || fail 'pre-return produced no ci-local log directory'
grep -F "base=$start platform=$host_platform mode=pre-return" "$pre_log/repository-rules.log" >/dev/null ||
  fail 'pre-return did not pass its declared base to stage 0'
grep -F 'FAIL public-history' "$pre_log/repository-rules.log" >/dev/null &&
  grep -F "$orphan does not retain published checkpoint" "$pre_log/repository-rules.log" >/dev/null ||
  fail 'pre-return did not check the pull-request head for public history'
rm -rf "$history_work"
trap - EXIT HUP INT TERM

"$root/tooling/repository/ci/test-ci-selected-history.sh"

# Inventory checks every platform runs: every gate above in the gate
# inventory (an expired provisional date is reported here and refused at
# release), and every historical reader in docs/legacy-compatibility.md.
for check in check_gate_inventory.py check_legacy_inventory.py; do
  python3 "$root/tooling/repository/checks/$check" --self-test || fail "$check self-test failed"
  python3 "$root/tooling/repository/checks/$check" || fail "$check failed"
done
printf 'ci-stage contract passed\n'
