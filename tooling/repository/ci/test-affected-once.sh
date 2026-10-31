#!/bin/sh
# Exercise the real CI dispatch and affected selector with a recording Cargo.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd -P)
fail() { printf 'affected-once contract: %s\n' "$*" >&2; exit 1; }
fixture=$(mktemp -d "${TMPDIR:-/tmp}/affected-once.XXXXXX")
trap 'rm -rf "$fixture"' EXIT HUP INT TERM
git clone -q --shared "$root" "$fixture/repo" || fail 'fixture clone failed'
repo=$fixture/repo
for file in ci-local.sh ci-stages.sh test-affected.sh verification-context.sh ci-preflight.sh; do
  cp "$root/tooling/repository/ci/$file" "$repo/tooling/repository/ci/$file"
done
# Other stages are unrelated to the target-selection contract. Keep the real
# stage runner and selector, while isolating the fixture from workflow tools.
for file in check-workflows.sh test-workflow-validation.sh test-ci-stages.sh; do
  printf '#!/bin/sh\nexit 0\n' > "$repo/scripts/$file"
done
printf 'raise SystemExit(0)\n' > "$repo/scripts/test-quiet-command.py"
printf '#!/bin/sh\nexit 0\n' > "$repo/tooling/repository/ci/check-repository-rules.sh"
cat > "$fixture/cargo" <<'CARGO'
#!/bin/sh
case ${1:-} in
  --version) printf 'cargo selection-fixture\n'; exit 0 ;;
  fmt|clippy) exit 0 ;;
  test)
    printf '%s\n' "$*" >> "$CARGO_RECORD"
    case " $* " in
      *" --test ${FAIL_TARGET:-__none__} "*) exit 47 ;;
    esac
    case " $* " in
      *" --test ${ZERO_TARGET:-__none__} "*)
        printf 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'
        exit 0 ;;
    esac
    printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out\n'
    ;;
  *) exit 2 ;;
esac
CARGO
chmod +x "$fixture/cargo"
mkdir -p "$fixture/tmp" "$fixture/logs"
# A dirty source selects binary unit tests. An untracked, otherwise unreferenced
# test selects every target, including targets unrelated to the source edit.
printf '\n// affected selection fixture\n' >> "$repo/src/main.rs"
base=$(git -C "$repo" rev-parse HEAD)
export CARGO="$fixture/cargo" CARGO_TARGET_DIR="$fixture/target" TMPDIR="$fixture/tmp" CI_LOG_ROOT="$fixture/logs" CARGO_RECORD="$fixture/cargo.calls" VALUE_PROOF_BASE="$base" PYTHONDONTWRITEBYTECODE=1
unset PRE_RETURN_BASE FAIL_TARGET ZERO_TARGET 2>/dev/null || :
(cd "$repo" && sh tooling/repository/ci/test-affected.sh --base "$base" --list) > "$fixture/before-untracked.out" 2>&1 || fail 'dirty selection failed'
if grep -F ' --test away_native' "$fixture/before-untracked.out" >/dev/null; then
  fail 'fixture does not isolate untracked-file selection'
fi
printf '// untracked affected selection fixture\n' > "$repo/tests/affected_once_untracked.rs"
if ! (cd "$repo" && sh tooling/repository/ci/ci-local.sh --pre-return) > "$fixture/pass.out" 2>&1; then
  cat "$fixture/pass.out" >&2
  fail 'real pre-return dispatch did not succeed with the recording Cargo'
fi
for target in architecture_budget privacy_gate release_integrity docs_workflow first_impression plugin_distribution; do
  count=$(grep -Ec " --test $target$" "$CARGO_RECORD" || :)
  test "$count" -eq 1 || fail "$target must execute once in pre-return; saw $count"
done
for target in architecture_budget privacy_gate release_integrity docs_workflow plugin_distribution; do
  grep -F "test --locked --features repository-checks --test $target" "$CARGO_RECORD" >/dev/null ||
    fail "$target did not retain repository-checks"
done
grep -F 'test --locked --test first_impression' "$CARGO_RECORD" >/dev/null || fail 'first_impression features changed'
test "$(grep -Ec ' --test value_contract$' "$CARGO_RECORD" || :)" -eq 2 ||
  fail 'value_contract must execute with both distinct feature sets'
grep -F 'test --locked --test value_contract' "$CARGO_RECORD" >/dev/null || fail 'non-feature value_contract missing'
grep -F 'test --locked --features repository-checks --test value_contract' "$CARGO_RECORD" >/dev/null || fail 'feature value_contract missing'
grep -F 'test --locked --bin exitbind' "$CARGO_RECORD" >/dev/null || fail 'dirty source did not select binary unit tests'
grep -F ' --test away_native' "$CARGO_RECORD" >/dev/null || fail 'untracked test did not select otherwise unrelated targets'
printf 'pre-return fixture Cargo calls:\n'
cat "$CARGO_RECORD"
# An ordinary standalone affected run does not inherit the earlier passes.
: > "$CARGO_RECORD"
if ! (cd "$repo" && sh tooling/repository/ci/test-affected.sh --base "$base") > "$fixture/standalone.out" 2>&1; then
  cat "$fixture/standalone.out" >&2
  fail 'standalone affected run failed'
fi
for target in architecture_budget privacy_gate release_integrity docs_workflow first_impression plugin_distribution; do
  grep -E " --test $target$" "$CARGO_RECORD" >/dev/null || fail "standalone affected run omitted $target"
done
printf 'standalone affected fixture Cargo calls:\n'
cat "$CARGO_RECORD"
# The caller's pair must match both the target and features; a mismatch runs.
: > "$CARGO_RECORD"
if ! (cd "$repo" && sh tooling/repository/ci/test-affected.sh --base "$base" --skip-passed 'privacy_gate=repository-checks first_impression=repository-checks') > "$fixture/pairs.out" 2>&1; then
  cat "$fixture/pairs.out" >&2
  fail 'feature-pair selection failed'
fi
if grep -E ' --test privacy_gate$' "$CARGO_RECORD" >/dev/null; then
  fail 'matching passed target was repeated'
fi
grep -F 'test --locked --test first_impression' "$CARGO_RECORD" >/dev/null || fail 'different features incorrectly suppressed the target'
printf 'feature-pair selection fixture passed\n'
# Exit zero without any executed test is not an earlier pass. The affected
# stage must rerun that pair and retain its own zero-test failure.
: > "$CARGO_RECORD"
export ZERO_TARGET=privacy_gate
if (cd "$repo" && sh tooling/repository/ci/ci-local.sh --pre-return) > "$fixture/zero.out" 2>&1; then
  fail 'zero-test predecessor suppressed the affected target'
fi
test "$(grep -Ec ' --test privacy_gate$' "$CARGO_RECORD" || :)" -eq 2 ||
  fail 'zero-test predecessor did not rerun its affected target'
grep -F 'stage=affected-tests' "$fixture/zero.out" >/dev/null || fail 'zero-test failure did not reach affected-tests'
unset ZERO_TARGET
printf 'zero-test predecessor fixture Cargo calls:\n'
cat "$CARGO_RECORD"
# A later dirty subject must execute its predecessor again; its failed stage
# must stop the run before affected selection can claim any earlier pass.
printf '// later dirty subject\n' >> "$repo/src/main.rs"
for FAIL_TARGET in architecture_budget privacy_gate release_integrity docs_workflow first_impression plugin_distribution; do
  export FAIL_TARGET
  : > "$CARGO_RECORD"
  if (cd "$repo" && sh tooling/repository/ci/ci-local.sh --pre-return) > "$fixture/fail.out" 2>&1; then
    fail "failed $FAIL_TARGET predecessor returned success"
  fi
  grep -E " --test $FAIL_TARGET$" "$CARGO_RECORD" >/dev/null || fail "new run reused $FAIL_TARGET"
  grep -F 'ci-local: exit=47 stage=' "$fixture/fail.out" >/dev/null || fail "failed $FAIL_TARGET predecessor was not reported"
  if grep -F 'start: affected-tests;' "$fixture/fail.out" >/dev/null; then
    fail "affected selection ran after a failed $FAIL_TARGET predecessor"
  fi
  printf 'new dirty subject rejected failed %s predecessor\n' "$FAIL_TARGET"
  cat "$CARGO_RECORD"
done
printf 'affected-once contract passed\n'
