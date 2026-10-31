#!/bin/sh
# Existing CI stages. Pre-return selects their applicable subset from the
# changed-path relation; final native qualification retains the full recipe.
ci_stage_order() {
  printf '%s\n' repository-rules workflows workflow-regression \
    ci-stage-contract quiet-command-regression fmt clippy tests affected-tests \
    value-proof drift-warning native-handoff
}

ci_stage_selected() {
  mode=$1 stage=$2 platform=$3
  scope=${CI_PRE_RETURN_SCOPE:-broad}
  case "$stage" in
    repository-rules) return 0 ;;
    workflows|workflow-regression|ci-stage-contract|quiet-command-regression)
      test "$mode" = full && return 0
      case " $scope " in *" broad "*) return 0 ;; *) return 1 ;; esac ;;
    fmt|clippy)
      test "$platform" = linux || return 1
      test "$mode" = full && return 0
      case " $scope " in *" rust "*|*" broad "*) return 0 ;; esac
      if [ "$stage" = clippy ]; then
        case " $scope " in *" build "*) return 0 ;; esac
      fi
      return 1 ;;
    affected-tests) test "$mode" = pre-return ;;
    tests|value-proof) test "$mode" = full ;;
    drift-warning|native-handoff) test "$mode" = full && test "$platform" = linux ;;
    *) return 1 ;;
  esac
}

ci_repository_rule_check_selected() {
  check=$1 platform=$2 mode=$3
  case "$check" in
    path-rendering|product-surface|release-refs) test "$platform" = linux ;;
    public-history|commit-identity) return 0 ;;
    *) return 1 ;;
  esac
}
