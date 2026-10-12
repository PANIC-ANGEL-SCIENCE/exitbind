#!/bin/sh
# Only four generated data files can reach the isolated branch. No force push.
set -eu
out=$(mktemp -d)
work=$(mktemp -d)
rmdir "$work"
cleanup() { git worktree remove --force "$work" 2>/dev/null || true; rm -rf "$out"; }
trap cleanup EXIT HUP INT TERM
python3 tooling/readme/badges.py --output "$out"
if git ls-remote --exit-code --heads origin readme-badges >/dev/null; then
  git fetch --no-tags origin refs/heads/readme-badges
  git worktree add --detach "$work" FETCH_HEAD
  actual=$(git -C "$work" ls-tree --format='%(objectmode) %(objecttype) %(path)' HEAD | sort)
  expected=$(printf '100644 blob %s\n' ci.svg license.svg release.svg status.json | sort)
  test "$actual" = "$expected" || { echo 'Unexpected badge branch files; refusing'; exit 1; }
else
  git worktree add --detach "$work" HEAD
  git -C "$work" switch --orphan badge-publication
fi
cp "$out/ci.svg" "$out/release.svg" "$out/license.svg" "$out/status.json" "$work/"
git -C "$work" add -- ci.svg release.svg license.svg status.json
if git -C "$work" diff --cached --quiet; then exit 0; fi
git -C "$work" -c user.name=envy404 -c user.email=287204764+envy404@users.noreply.github.com commit -m 'docs: refresh README status badges'
expected_sha=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["main_sha"])' "$out/status.json")
actual_sha=$(git ls-remote origin refs/heads/main | cut -f1)
test "$actual_sha" = "$expected_sha" || { echo 'Main advanced; refusing stale publication'; exit 1; }
git -C "$work" push origin HEAD:refs/heads/readme-badges
