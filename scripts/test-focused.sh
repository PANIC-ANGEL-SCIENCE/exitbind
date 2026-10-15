#!/bin/sh
# Run one selected Cargo test target through the same owned context as CI.
set -eu
fail() { printf 'test-focused: %s\n' "$*" >&2; exit 1; }
test "$#" -ge 2 && test "$1" = --test || fail 'usage: test-focused.sh --test TARGET [FILTER] [-- TEST_OPTIONS]'
validate_selection() {
  filter_seen=false
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --test)
        test "$#" -ge 2 && test -n "$2" || fail '--test requires a target'
        case "$2" in -*) fail '--test requires a target' ;; esac
        shift 2
        ;;
      --) return 0 ;;
      -*) fail 'only --test targets and one filter are accepted before --' ;;
      *)
        test "$filter_seen" = false || fail 'only one test filter is accepted'
        filter_seen=true
        shift
        ;;
    esac
  done
}
validate_selection "$@"
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
. "$root/scripts/verification-context.sh"
prepare_verification_context "$root"
ci_mode=focused
begin_verification_run
run focused "$CARGO" test --locked "$@"
# A typo or ignored selection must not silently report focused success.
python3 - "$run_dir/focused.log" <<'PYTEST'
import importlib.util
import pathlib
import sys
sys.dont_write_bytecode = True
helper = pathlib.Path.cwd() / "scripts/quiet-command.py"
spec = importlib.util.spec_from_file_location("quiet_command", helper)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
counts, _, reason = module.parse_cargo_output(pathlib.Path(sys.argv[1]).read_bytes())
if counts is None:
    raise SystemExit("focused test selection was not verified: " + str(reason))
PYTEST
current_stage=focused-complete
