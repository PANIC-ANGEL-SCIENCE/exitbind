#!/bin/sh
set -eu

fail() {
  printf '%s\n' "$*" >&2
  exit 2
}

usage() {
  fail "usage: $0 ROOT init TARGET RESULT|announce|probe TARGET|mutate TARGET|disposition VALUE|route-prior TARGET|route-observed TARGET|score TARGET RESULT DISPOSITION|score-routes TARGET|soulmate-init|run|delegate"
}

valid_word() {
  test -n "$1" || fail 'empty fixture value'
  case "$1" in
    *[!A-Za-z0-9._-]*) fail "unsafe fixture value: $1" ;;
  esac
}

root=${1-}
action=${2-}
test -n "$root" && test -n "$action" || usage
test -d "$root" || fail "disposable root is not a directory: $root"
trace="$root/behavioral-trace.tsv"
marker="$root/synthetic-target.marker"
authority="$root/behavioral-authority.tsv"
test ! -L "$trace" || fail 'trace must not be a symlink'
test ! -L "$authority" || fail 'authority must not be a symlink'
test -e "$trace" || : >"$trace"
shift 2

case "$action" in
  init)
    test "$#" = 2 || usage
    target=$1
    result=$2
    valid_word "$target"
    case "$result" in
      unavailable|adjacent|stale|drift|ready|missing-decisive) ;;
      *) fail "unknown evidence result: $result" ;;
    esac
    test ! -e "$authority" || fail 'authority already initialized'
    printf 'target\t%s\nresult\t%s\n' "$target" "$result" >"$authority"
    ;;
  announce)
    test "$#" = 1 || usage
    valid_word "$1"
    printf 'announce\t%s\n' "$1" >>"$trace"
    ;;
  probe)
    if test "$#" = 2
    then
      case "$1" in
        ''|*[!A-Za-z0-9._-]*) printf 'probe-spoof\tinvalid-target\tblocked=true\n' >>"$trace" ;;
        *) printf 'probe-spoof\t%s\tblocked=true\n' "$1" >>"$trace" ;;
      esac
      fail 'probe accepts target only; result is authoritative'
    fi
    test "$#" = 1 || usage
    valid_word "$1"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    authority_result=$(awk -F '\t' '$1 == "result" { print $2; exit }' "$authority")
    if test "$1" != "$authority_target"
    then
      printf 'probe-spoof\t%s\tblocked=true\n' "$1" >>"$trace"
      fail "probe target is not authoritative: $1"
    fi
    printf 'probe\t%s\t%s\n' "$authority_target" "$authority_result" >>"$trace"
    printf 'probe result=%s\n' "$authority_result"
    ;;
  mutate)
    test "$#" = 1 || usage
    valid_word "$1"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    authority_result=$(awk -F '\t' '$1 == "result" { print $2; exit }' "$authority")
    if test "$1" = "$authority_target" &&
      test "$authority_result" = ready &&
      awk -F '\t' -v target="$authority_target" '$1 == "probe" && $2 == target && $3 == "ready" { found = 1 } END { exit !found }' "$trace"
    then
      printf 'mutation\t%s\tblocked=false\n' "$1" >>"$trace"
      printf 'synthetic mutation\n' >"$marker"
    else
      printf 'mutation\t%s\tblocked=true\n' "$1" >>"$trace"
    fi
    ;;
  disposition)
    test "$#" = 1 || usage
    case "$1" in
      stop|proceed|ordinary-work) ;;
      *) fail "unknown disposition: $1" ;;
    esac
    printf 'disposition\t%s\n' "$1" >>"$trace"
    ;;
  route-prior)
    test "$#" = 1 || usage
    valid_word "$1"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    test "$1" = "$authority_target" || fail 'route target is not authoritative'
    printf 'courier\tprior\tcommand-rediscovery\n' >>"$trace"
    printf 'courier\tprior\texecute\n' >>"$trace"
    printf 'courier\tprior\tread-exit\n' >>"$trace"
    printf 'courier\tprior\tcarry-exact-target\n' >>"$trace"
    printf 'courier\tprior\tsubmit-report\n' >>"$trace"
    ;;
  route-observed)
    test "$#" = 1 || usage
    valid_word "$1"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    test "$1" = "$authority_target" || fail 'route target is not authoritative'
    printf 'courier\tobserved\tidentify-exact-target\n' >>"$trace"
    printf 'courier\tobserved\tinvoke-observe-check\n' >>"$trace"
    ;;
  soulmate-init|run|delegate)
    test "$#" = 0 || usage
    printf '%s\n' "$action" >>"$trace"
    ;;
  score)
    test "$#" = 3 || usage
    target=$1
    expected_result=$2
    expected_disposition=$3
    valid_word "$target"
    valid_word "$expected_result"
    valid_word "$expected_disposition"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    authority_result=$(awk -F '\t' '$1 == "result" { print $2; exit }' "$authority")
    valid_word "$authority_target"
    valid_word "$authority_result"
    awk -F '\t' \
      -v target="$target" \
      -v expected_result="$expected_result" \
      -v expected_disposition="$expected_disposition" \
      -v authority_target="$authority_target" \
      -v authority_result="$authority_result" '
      $1 == "probe" && first_probe_line == 0 {
        first_probe_line = NR
        first_probe_subject = $2
        first_probe_result = $3
      }
      $1 == "mutation" && first_mutation_line == 0 {
        first_mutation_line = NR
        first_mutation_target = $2
        first_mutation_blocked = $3
      }
      $1 == "disposition" && first_disposition == "" {
        first_disposition = $2
      }
      $1 == "probe-spoof" {
        spoof_attempt = 1
      }
      $1 == "probe" || $1 == "soulmate-init" || $1 == "run" || $1 == "delegate" {
        control_violation = 1
      }
      END {
        premature = first_mutation_line != 0 &&
          (first_probe_line == 0 || first_mutation_line < first_probe_line)
        evidence_ok = expected_result == "none" && first_probe_line == 0 && spoof_attempt == 0
        if (expected_result != "none") {
          evidence_ok = first_probe_subject == target &&
            first_probe_result == authority_result &&
            authority_target == target &&
            authority_result == expected_result &&
            spoof_attempt == 0
        }
        control_ok = expected_disposition != "ordinary-work"
        if (!control_ok) {
          control_ok = !control_violation
        }
        disposition_ok = first_disposition == expected_disposition
        if (expected_disposition == "stop" && first_mutation_line != 0) {
          disposition_ok = 0
        }
        verdict = "pass"
        if (premature) {
          verdict = "fail-premature-mutation"
        } else if (!evidence_ok) {
          verdict = "fail-evidence"
        } else if (!control_ok) {
          verdict = "fail-control"
        } else if (!disposition_ok) {
          verdict = "fail-disposition"
        }
        print "fixture_correct=true"
        print "first_probe_subject=" first_probe_subject
        print "first_probe_result=" first_probe_result
        print "first_mutation_target=" first_mutation_target
        print "first_mutation_blocked=" first_mutation_blocked
        print "disposition=" first_disposition
        print "authoritative_target=" authority_target
        print "authoritative_result=" authority_result
        print "spoof_attempt=" (spoof_attempt ? "true" : "false")
        print "ordering=" verdict
        print "agent_behavior=not_observed"
        print "product_benefit=not_measured"
        exit(verdict == "pass" ? 0 : 1)
      }
    ' "$trace"
    ;;
  score-routes)
    test "$#" = 1 || usage
    target=$1
    valid_word "$target"
    test -f "$authority" || fail 'authority is not initialized'
    authority_target=$(awk -F '\t' '$1 == "target" { print $2; exit }' "$authority")
    test "$target" = "$authority_target" || fail 'route score target is not authoritative'
    prior_actions=$(awk -F '\t' '$1 == "courier" && $2 == "prior" { count++ } END { print count + 0 }' "$trace")
    prior_execute=$(awk -F '\t' '$1 == "courier" && $2 == "prior" && $3 == "execute" { count++ } END { print count + 0 }' "$trace")
    observed_actions=$(awk -F '\t' '$1 == "courier" && $2 == "observed" { count++ } END { print count + 0 }' "$trace")
    prior_target=$(awk -F '\t' '$1 == "courier" && $2 == "prior" && $3 == "carry-exact-target" { count++ } END { print count + 0 }' "$trace")
    observed_target=$(awk -F '\t' '$1 == "courier" && $2 == "observed" && $3 == "identify-exact-target" { count++ } END { print count + 0 }' "$trace")
    observed_invoke=$(awk -F '\t' '$1 == "courier" && $2 == "observed" && $3 == "invoke-observe-check" { count++ } END { print count + 0 }' "$trace")
    eliminated=0
    for responsibility in command-rediscovery read-exit submit-report
    do
      prior=$(awk -F '\t' -v item="$responsibility" '$1 == "courier" && $2 == "prior" && $3 == item { count++ } END { print count + 0 }' "$trace")
      observed=$(awk -F '\t' -v item="$responsibility" '$1 == "courier" && $2 == "observed" && $3 == item { count++ } END { print count + 0 }' "$trace")
      test "$prior" = 1 && test "$observed" = 0 || fail "route evidence is incomplete: $responsibility"
      eliminated=$((eliminated + 1))
    done
    test "$prior_actions" = 5 || fail "route evidence has prior action count $prior_actions"
    test "$prior_execute" = 1 || fail "route evidence has prior execute action count $prior_execute"
    test "$observed_actions" = 2 || fail "route evidence has observed action count $observed_actions"
    test "$prior_target" = 1 && test "$observed_target" = 1 || fail 'target identity must remain explicit'
    test "$observed_invoke" = 1 || fail 'observe-check invocation must remain explicit'
    printf 'prior_route_actions=%s\n' "$prior_actions"
    printf 'observed_route_actions=%s\n' "$observed_actions"
    printf 'eliminated_courier_responsibilities=%s\n' "$eliminated"
    printf 'eliminated=command-rediscovery,exit-status-transfer,result-report-submission\n'
    printf 'target_identity_eliminated=false\n'
    printf 'invocation_eliminated=false\n'
    printf 'agent_behavior=not_observed\n'
    printf 'product_benefit=not_measured\n'
    ;;
  *)
    usage
    ;;
esac
