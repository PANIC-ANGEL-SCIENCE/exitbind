#!/bin/sh
# Exercise a candidate binary against a private copy of this project's state.
set -eu

fail() { printf 'replay-state: %s\n' "$*" >&2; exit 1; }
test "$#" -eq 2 || fail 'usage: replay-state.sh NEW_BIN PROJECT_ROOT'
bin=$1
case "$bin" in /*) ;; *) bin="$(pwd -P)/$bin" ;; esac
test -f "$bin" && test -x "$bin" && test ! -L "$bin" || fail 'NEW_BIN must be a regular executable'
project=$(CDPATH= cd -- "$2" && pwd -P) || fail 'PROJECT_ROOT is unavailable'
test "$(git -C "$project" rev-parse --show-toplevel)" = "$project" || fail 'PROJECT_ROOT must be the Git root'
test -f "$project/exitbind.json" && test ! -L "$project/exitbind.json" || fail 'an ordinary exitbind.json is required'
test -d "$project/.exitbind" && test ! -L "$project/.exitbind" || fail 'an ordinary .exitbind directory is required'
test -z "$(git -C "$project" status --porcelain --untracked-files=no)" || fail 'commit the tracked candidate before replay'
command -v python3 >/dev/null 2>&1 || fail 'python3 is required to inspect the replay result'
python3 - "$project/exitbind.json" "$project/.exitbind" <<'PY' || fail 'replay requires portable project.root "." and self-contained state'
import json, os, pathlib, sys
config = json.loads(pathlib.Path(sys.argv[1]).read_text())
project = config.get('project', {})
if project.get('mode', 'portable') != 'portable' or project.get('root') != '.':
    raise SystemExit(1)
for directory, directories, files in os.walk(sys.argv[2], followlinks=False):
    if any(pathlib.Path(directory, name).is_symlink() for name in directories + files):
        raise SystemExit(1)
PY

scratch=${EXITBIND_REPLAY_TMPDIR:-${TMPDIR:-/tmp}}
test -d "$scratch" || fail 'temporary directory is unavailable'
scratch=$(CDPATH= cd -- "$scratch" && pwd -P)
case "$scratch" in "$project"|"$project"/*) fail 'temporary directory must be outside PROJECT_ROOT' ;; esac
tmp=$(mktemp -d "$scratch/exitbind-replay.XXXXXX") || fail 'cannot create private replay directory'
copy=$tmp/repo
created=false
cleanup() {
  if [ "$created" = true ]; then
    git -C "$project" worktree remove --force "$copy" ||
      printf 'replay-state: inspect retained worktree %s\n' "$copy" >&2
  fi
  if [ ! -e "$copy" ]; then find "$tmp" -depth -delete; fi
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

head=$(git -C "$project" rev-parse HEAD)
git -C "$project" worktree add --detach -q "$copy" "$head"
created=true
# Include private ledgers and local configuration without copying build caches.
python3 - "$project" "$copy" <<'PY'
from pathlib import Path
from shutil import copytree
import sys
source, destination = map(Path, sys.argv[1:])
def ignore(directory, names):
    return {'.git', 'target', 'dist'} & set(names) if Path(directory) == source else set()
copytree(source, destination, dirs_exist_ok=True, symlinks=True, ignore=ignore)
PY
(
  cd "$copy"
  "$bin" work resume --json > "$tmp/resume.json" || fail 'work resume failed on copied state'
  count=0
  for ledger in .exitbind/runs/*.jsonl; do
    test -f "$ledger" || continue
    "$bin" run inspect "$ledger" --json > /dev/null || fail "run inspect failed for $ledger"
    "$bin" run status "$ledger" --json > /dev/null || fail "run status failed for $ledger"
    count=$((count + 1))
  done
  printf 'replay-state: ledgers=%s\n' "$count"
)
python3 - "$tmp/resume.json" "$head" <<'PY'
import json, pathlib, sys
path = pathlib.Path(sys.argv[1])
head = sys.argv[2]
raw = path.read_bytes()
value = json.loads(raw)
if len(raw) > 8192:
    raise SystemExit(f'replay-state: resume packet exceeds 8192 bytes ({len(raw)})')
if value.get('status') not in {'resumed', 'ambiguous', 'unresolved', 'none'}:
    raise SystemExit('replay-state: resume did not return a recovery state')
if value.get('status') == 'ambiguous' and not value.get('works'):
    raise SystemExit('replay-state: ambiguous recovery omitted candidates')
if value.get('recorder', {}).get('commit') != head:
    raise SystemExit('replay-state: recorder commit differs from source HEAD')
if value.get('unreadable'):
    raise SystemExit(f"replay-state: {len(value['unreadable'])} candidate(s) became unreadable")
for work in value.get('works', []):
    command = work.get('command')
    if not isinstance(command, list) or len(command) < 4 or command[1:3] != ['work', 'next'] or command[3] != work.get('work'):
        raise SystemExit('replay-state: recovery candidate lacks its exact next command')
print(f"replay-state: pass status={value['status']} bytes={len(raw)}")
PY
printf 'replay-state: source=%s\n' "$head"
