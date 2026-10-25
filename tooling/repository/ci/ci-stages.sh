#!/bin/sh
# Canonical native-check stage membership shared by full and pre-return CI.

ci_stage_order() {
  printf '%s\n' \
    repository-rules \
    history \
    workflows \
    workflow-regression \
    ci-stage-contract \
    quiet-command-regression \
    fmt \
    path-rendering \
    product-surface \
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
    native-handoff \
    release-refs
}

ci_stage_selected() {
  mode=$1
  stage=$2
  platform=$3

  case "$stage" in
    repository-rules|history|workflows|workflow-regression|ci-stage-contract|quiet-command-regression)
      return 0
      ;;
    fmt|path-rendering|product-surface|clippy)
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
    release-refs)
      test "$platform" = linux && { test "$mode" = full || test "$mode" = pre-return; }
      return
      ;;
    *)
      return 1
      ;;
  esac
}
