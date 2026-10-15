#!/bin/sh
# Shared preparation for owned repository verification, never real agent work.

isolate_verification_context() {
  unset CODEX_THREAD_ID CODEX_SESSION_ID CLAUDE_CODE_SESSION_ID EXITBIND_NATIVE_SESSION_ID EXITBIND_SESSION_GOALS_DIR CLAUDE_ENV_FILE XDG_STATE_HOME
  unset GIT_DIR GIT_WORK_TREE GIT_COMMON_DIR GIT_INDEX_FILE GIT_CEILING_DIRECTORIES GIT_DISCOVERY_ACROSS_FILESYSTEM
}

prepare_verification_context() {
  isolate_verification_context
  root=$(CDPATH= cd -- "$1" && pwd -P)
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
  cd -- "$root"

  # Resolve target, fixture and log roots before any build or instrumentation.
  . "$root/scripts/ci-preflight.sh"
  prepare_execution_roots
  CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}
  export CARGO_BUILD_JOBS
}

begin_verification_run() {
  # Retain raw stage output independently of the caller's console/tee log.
  # Git-private storage is excluded from product input fingerprints and commits.
  log_root=$CI_LOG_ROOT
  mkdir -p "$log_root"
  log_root=$(CDPATH= cd -- "$log_root" && pwd -P)
  run_dir=$(umask 077; mktemp -d "$log_root/run.XXXXXX")
  show_execution_preflight > "$run_dir/preflight"
  capture_pid=
  current_stage=setup
  owns_lock=false
  finish() {
    result=$?
    trap - 0
    printf '%s\n' "$result" > "$run_dir/run.exit"
    printf 'stage=%s exit=%s\n' "$current_stage" "$result" > "$run_dir/state"
    if [ "$owns_lock" = true ]; then
      rm -f "$target_lock/owner" && rmdir "$target_lock" ||
        printf 'ci-local: could not remove own target lock: %s\n' "$target_lock" >&2
    fi
    printf 'ci-local: exit=%s stage=%s logs=%s\n' "$result" "$current_stage" "$run_dir"
    exit "$result"
  }
  trap finish 0
  trap 'interrupt_verification 130 INT' INT
  trap 'interrupt_verification 143 TERM' TERM
  trap 'interrupt_verification 129 HUP' HUP
  printf 'ci-local: logs=%s\n' "$run_dir"
  printf 'stage=setup running\n' > "$run_dir/state"

  # Do not let two ci-local invocations or different worktrees mix this cache.
  mkdir -p "$CARGO_TARGET_DIR"
  CARGO_TARGET_DIR=$(CDPATH= cd -- "$CARGO_TARGET_DIR" && pwd -P)
  export CARGO_TARGET_DIR
  EXITBIND_BIN="$CARGO_TARGET_DIR/debug/exitbind"
  export EXITBIND_BIN
  target_lock="$CARGO_TARGET_DIR/.ci-local-lock"
  mkdir "$target_lock" || fail "cannot acquire target lock: $target_lock; inspect its owner and filesystem before retrying"
  owns_lock=true
  printf 'pid=%s\nroot=%s\nlogs=%s\n' "$$" "$root" "$run_dir" > "$target_lock/owner"
  printf 'started=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" >> "$target_lock/owner"
  if [ -r /proc/sys/kernel/random/boot_id ]; then
    printf 'boot=%s\n' "$(cat /proc/sys/kernel/random/boot_id)" >> "$target_lock/owner"
  fi
  printf 'process-start=%s\n' "$(ps -p "$$" -o lstart=)" >> "$target_lock/owner"
  cache_owner="$CARGO_TARGET_DIR/.ci-local-checkout"
  if [ -e "$cache_owner" ]; then
    previous_root=$(cat "$cache_owner")
    test "$previous_root" = "$root" || fail 'target belongs to another checkout; choose a separate CARGO_TARGET_DIR (existing cache retained)'
  else
    printf '%s\n' "$root" > "$cache_owner"
  fi
  {
    printf 'head=%s\nroot=%s\ntarget=%s\ncandidate=%s\nmode=%s\n' "$(git rev-parse HEAD)" "$root" "$CARGO_TARGET_DIR" "${CANDIDATE_SHA:-HEAD}" "$ci_mode"
    "$CARGO" --version
    rustc --version
    git status --porcelain --untracked-files=normal
  } > "$run_dir/context"

}

interrupt_verification() {
  interrupted_status=$1
  interrupted_signal=$2
  trap '' INT TERM HUP
  if [ -n "$capture_pid" ]; then
    kill -s "$interrupted_signal" "$capture_pid" 2>/dev/null || :
    wait "$capture_pid" 2>/dev/null || :
    capture_pid=
  fi
  exit "$interrupted_status"
}

run() {
  current_stage=$1
  printf 'stage=%s running\n' "$current_stage" > "$run_dir/state"
  printf 'start: %s; log: %s/%s.log\n' "$current_stage" "$run_dir" "$current_stage"
  started=$(date +%s)
  QUIET_COMMAND_LOG="$run_dir/$current_stage.log" ./scripts/quiet-command.sh "$@" &
  capture_pid=$!
  if wait "$capture_pid"; then
    stage_status=0
  else
    stage_status=$?
  fi
  capture_pid=
  recorded_status=$(cat "$run_dir/$current_stage.log.exit") || fail "$current_stage has no captured child status"
  test "$recorded_status" = "$stage_status" || fail "$current_stage wrapper status disagrees with its captured child status"
  printf 'stage=%s exit=%s seconds=%s\n' "$current_stage" "$stage_status" "$(($(date +%s) - started))" >> "$run_dir/stages"
  test "$stage_status" -eq 0 || exit "$stage_status"
}

