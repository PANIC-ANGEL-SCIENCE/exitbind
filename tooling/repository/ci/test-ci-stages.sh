#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
cd "$root"
. "$root/tooling/repository/ci/ci-stages.sh"

fail() {
  printf 'ci-stage contract: %s\n' "$*" >&2
  exit 1
}

# Set by the candidate-history fixture for the ci-local runs it starts. Those
# runs reach this file again through ci-stage-contract, inside a clone whose
# stage 0 is the fixture stub; the nested copy must not start another fixture
# (a stage-0 regression would otherwise nest until the job timeout) and cannot
# compare the product guard against the stub. The outer run owns both checks.
nested=${EXITBIND_CI_CONTRACT_NESTED:-}

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

# Test actual command dispatch below rather than prescribing source spelling,
# a gate inventory, stage count or duplicate literal order.
if ci_stage_selected pre-return unknown linux; then fail 'unknown stage accepted'; fi
if ci_repository_rule_check_selected make-coffee linux full; then fail 'unknown check accepted'; fi
python3 "$root/tooling/repository/checks/check_commit_identity.py" --self-test ||
  fail 'commit identity scan self-test failed'

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
resolved=$(prepare_proof_base 0000000000000000000000000000000000000000 2>&1) || fail "a zero proof base failed: $resolved"
case $resolved in
  *"proof-base=$fork") ;;
  *) fail "a zero proof base must resolve to HEAD^ $fork: $resolved" ;;
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
# detached, branchless clone; it runs ci-local itself, so the base the guard
# receives is prepared by ci-local, not by the fixture. Its commits use the
# canonical identity the guard reads from tests/privacy_gate.rs, so only the
# candidate's history can fail. Fixture paths stay lowercase: the release macOS
# runner is case-insensitive, and a case-only difference in a path cannot be
# proven there.
#
# A stage-0 regression (the R2 case this fixture exists to catch) would let the
# run continue into ci-stage-contract, which executes this file again. The nested
# runs export EXITBIND_CI_CONTRACT_NESTED=1 so that copy skips this block and the
# outer assertion prints instead of nesting until the job timeout, and they point
# CARGO at a stub that only answers --version, so a run that got past stage 0
# cannot start a build in a fresh target directory.
if [ "$nested" = 1 ]; then
  printf 'ci-stage contract: nested run (executed ci-local fixture skipped)\n'
else
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
  # working tree so a local, not-yet-committed edit is what runs here too. This
  # file is overlaid as well: the nested run inside the clone must carry the
  # sentinel guard, not the parent commit's copy.
  for file in check-repository-rules.sh ci-stages.sh ci-local.sh \
    verification-context.sh ci-preflight.sh test-ci-stages.sh test-affected.sh; do
    cp "$root/tooling/repository/ci/$file" "$history_work/repo/tooling/repository/ci/$file"
  done
  # The merge itself keeps published history; only the contributing head drops it.
  # The control is the one check, not a second guard run.
  if ! (cd "$history_work/repo" && sh scripts/check-public-history.sh HEAD) \
    > "$history_work/retained.out" 2>&1; then
    fail "the merge commit itself must retain public history: $(tail -n 3 "$history_work/retained.out")"
  fi
  # Stage 0 costs ~31 s on Linux; the contract needs its order and its stop, not
  # three full guard runs. The fixture stage 0 is a stub that records exactly
  # what ci-local passed and runs the one check that decides the candidate
  # (~0.03 s), so a candidate that dropped history still stops the run at stage
  # 0. The full guard runs as ci-local's own stage in every pipeline, and the
  # macOS selection below executes the product guard's forwarding itself.
  cargo_stub=$history_work/cargo-stub
  cat > "$cargo_stub" <<'STUB'
#!/bin/sh
# ci-local may ask for a version; the contract must never start a build.
case ${1:-} in
  --version) printf 'cargo 0.0.0-contract-fixture\n'; exit 0 ;;
esac
printf 'ci contract fixture: cargo was invoked: %s\n' "$*" >&2
exit 1
STUB
  chmod +x "$cargo_stub"
  stage0=$history_work/repo/tooling/repository/ci/check-repository-rules.sh
  cat > "$stage0" <<'STUB'
#!/bin/sh
# Contract fixture stage 0: record the call, then run the candidate check.
set -u
record=${EXITBIND_CI_CONTRACT_RECORD:?contract fixture has no record path}
printf 'base=%s platform=%s mode=%s candidate=%s\n' \
  "${1:-}" "${2:-}" "${3:-}" "${CANDIDATE_SHA:-}" >> "$record"
exec sh scripts/check-public-history.sh HEAD ${CANDIDATE_SHA:+"$CANDIDATE_SHA"}
STUB
  # The working selector implements proportional stages, so the fixture must
  # execute these current bytes instead of its historical clone's selector.
  cp "$root/tooling/repository/checks/test_target_sources.py" \
    "$history_work/repo/tooling/repository/checks/test_target_sources.py"
  # Full mode with an absent base and a pull-request head, executed through
  # ci-local's own ordering: prepare_value_proof_base must run first (HEAD^ is the
  # merge's first parent), stage 0 must receive that base and CANDIDATE_SHA, and
  # the run must stop before any cargo stage.
  if (cd "$history_work/repo" &&
    unset VALUE_PROOF_BASE PRE_RETURN_BASE &&
    EXITBIND_CI_CONTRACT_NESTED=1 EXITBIND_CI_CONTRACT_RECORD="$history_work/stage0.args" \
      CARGO="$cargo_stub" CARGO_TARGET_DIR="$history_work/target" TMPDIR="$history_work/tmp" \
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
  grep -F "$orphan does not retain published checkpoint" "$full_log/repository-rules.log" >/dev/null ||
    fail 'full mode did not check the pull-request head for public history'
  # Pre-return with an absent VALUE_PROOF_BASE must not prepare one: it passes the
  # declared PRE_RETURN_BASE to stage 0 and checks the same candidate.
  if (cd "$history_work/repo" &&
    unset VALUE_PROOF_BASE &&
    EXITBIND_CI_CONTRACT_NESTED=1 EXITBIND_CI_CONTRACT_RECORD="$history_work/stage0.args" \
      CARGO="$cargo_stub" CARGO_TARGET_DIR="$history_work/target" TMPDIR="$history_work/tmp" \
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
  grep -F "$orphan does not retain published checkpoint" "$pre_log/repository-rules.log" >/dev/null ||
    fail 'pre-return did not check the pull-request head for public history'
  # What ci-local passed to stage 0, recorded by the stub: the prepared base in
  # full mode, the declared one in pre-return, the host platform and the head.
  test -f "$history_work/stage0.args" || fail 'stage 0 was never reached'
  test "$(wc -l < "$history_work/stage0.args")" -eq 2 ||
    fail 'stage 0 must run exactly once in each executed mode'
  grep -F "base=$start platform=$host_platform mode=full candidate=$orphan" \
    "$history_work/stage0.args" >/dev/null ||
    fail 'full mode did not pass the prepared base, host platform and candidate to stage 0'
  grep -F "base=$start platform=$host_platform mode=pre-return candidate=$orphan" \
    "$history_work/stage0.args" >/dev/null ||
    fail 'pre-return did not pass its declared base, host platform and candidate to stage 0'
  # The product guard itself still forwards the candidate; the macOS selection
  # runs it here in about 0.1 s instead of paying for the full Linux stage 0.
  cp "$root/tooling/repository/ci/check-repository-rules.sh" "$stage0"
  if (cd "$history_work/repo" && CANDIDATE_SHA="$orphan" \
    sh tooling/repository/ci/check-repository-rules.sh HEAD macos full) \
    > "$history_work/guard.out" 2>&1; then
    fail 'stage 0 accepted a candidate that dropped public history'
  fi
  grep -F 'FAIL public-history' "$history_work/guard.out" >/dev/null &&
    grep -F "$orphan does not retain published checkpoint" "$history_work/guard.out" >/dev/null ||
    fail 'stage 0 did not forward the pull-request head to public history'
  # Pre-return's affected-tests stage also runs the binary's unit tests when
  # src/ changed: no integration target runs them, and a host-dependent unit
  # test once reached public macOS readiness unrun (fl-0023). Listing needs no
  # build; a change outside src/ and the manifest must not select them.
  # Finish the fixture's checker update first. Its copied current scripts are
  # real build/check inputs; leaving them dirty would correctly select the
  # conservative unknown-dependency fallback instead of a docs-only subject.
  git -C "$history_work/repo" add -A
  git -C "$history_work/repo" commit -q --allow-empty -m 'Prepare affected selector fixture'
  printf '\n// contract fixture edit\n' >> "$history_work/repo/src/main.rs"
  (cd "$history_work/repo" && sh tooling/repository/ci/test-affected.sh --base HEAD --list) \
    > "$history_work/affected-src.out" 2>&1 || fail "affected listing failed: $(tail -n 3 "$history_work/affected-src.out")"
  grep -E ' test --locked --bin exitbind$' "$history_work/affected-src.out" >/dev/null ||
    fail 'a src/ change must select the binary unit tests in affected-tests'
  git -C "$history_work/repo" checkout -q -- src/main.rs
  printf '\ncontract fixture edit\n' >> "$history_work/repo/README.md"
  (cd "$history_work/repo" && sh tooling/repository/ci/test-affected.sh --base HEAD --list) \
    > "$history_work/affected-docs.out" 2>&1 || fail "affected listing failed: $(tail -n 3 "$history_work/affected-docs.out")"
  if grep -F -- '--bin exitbind' "$history_work/affected-docs.out" >/dev/null; then
    fail 'a change outside src/ and the manifest must not select the binary unit tests'
  fi
  rm -rf "$history_work"
  trap - EXIT HUP INT TERM
fi

if [ "$nested" != 1 ]; then
  sh "$root/tooling/repository/ci/test-affected-once.sh" || fail 'affected target execution contract failed'
fi

"$root/tooling/repository/ci/test-ci-selected-history.sh"


printf 'ci-stage contract passed\n'
