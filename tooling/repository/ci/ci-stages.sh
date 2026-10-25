#!/bin/sh
# Canonical native-check stage membership shared by full and pre-return CI.

ci_stage_order() {
  printf '%s\n' \
    repository-rules \
    workflows \
    workflow-regression \
    ci-stage-contract \
    quiet-command-regression \
    fmt \
    clippy \
    tests \
    architecture-budget \
    privacy-gate \
    release-integrity \
    docs-workflow \
    first-impression \
    value-contract \
    plugin-distribution \
    affected-tests \
    value-proof \
    drift-warning \
    native-handoff
}

ci_stage_selected() {
  mode=$1
  stage=$2
  platform=$3

  case "$stage" in
    repository-rules|workflows|workflow-regression|ci-stage-contract|quiet-command-regression)
      return 0
      ;;
    fmt|clippy)
      test "$platform" = linux
      return
      ;;
    architecture-budget|privacy-gate|release-integrity|docs-workflow|first-impression|value-contract|plugin-distribution|affected-tests)
      test "$mode" = pre-return
      return
      ;;
    tests|value-proof)
      test "$mode" = full
      return
      ;;
    drift-warning|native-handoff)
      test "$mode" = full && test "$platform" = linux
      return
      ;;
    *)
      return 1
      ;;
  esac
}

# Stage 0 runs the no-build checks that used to be their own stages. Each check
# keeps the platform and mode applicability its former stage had: the size
# budget runs in pre-return and, on Linux, in full mode so a public-CI push
# fails before the build; path rendering, the product surface and release
# references run only on Linux; public history runs everywhere. An unknown
# check is never selected, so a new check must be added here before the guard
# runs it.
ci_repository_rule_check_selected() {
  check=$1
  platform=$2
  mode=$3
  case "$check" in
    architecture-budget)
      test "$mode" = pre-return || test "$platform" = linux
      ;;
    path-rendering|product-surface|release-refs)
      test "$platform" = linux
      ;;
    public-history|commit-identity)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}
