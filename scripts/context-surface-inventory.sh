#!/bin/sh
set -eu

# Measures the byte and line size of the instruction surfaces a session reads,
# so a later context audit compares against a recorded baseline instead of an
# impression. It reports what is there; it sets no threshold, estimates no
# provider token counts, and shortens nothing.
#
# Paths are printed relative to the repository root, which keeps the report
# reproducible across checkouts and free of machine-specific locations.

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/.." && pwd)
cd "$repo_root"

if test "${1:-}" = "--json"; then
  # Machine-readable mode includes the reproducible baseline from HEAD and the
  # working-tree measurement.  Missing/unavailable values stay null; they are
  # never reported as zero telemetry.
  printf '{"method":"wc -c/wc -l plus git HEAD baseline","surfaces":['
  first=1
  for entry in \
    'AGENTS.md|bootstrap|root|none' \
    'skills/exitbind-bootstrap/SKILL.md|bootstrap|router|none' \
    'skills/exitbind/SKILL.md|activated-skill|router|preservation.md' \
    'skills/exitbind/references/preservation.md|delayed-reference|reference|none' \
    'plugins/exitbind/skills/exitbind/SKILL.md|packaged-duplicate|projection|skills/exitbind/SKILL.md' \
    'plugins/exitbind/skills/exitbind/references/preservation.md|packaged-duplicate|projection|references/preservation.md'
  do
    path=${entry%%|*}; rest=${entry#*|}
    stage=${rest%%|*}; rest=${rest#*|}
    projection=${rest%%|*}; duplicate=${rest#*|}
    if test -f "$path"; then
      after_bytes=$(wc -c <"$path" | tr -d ' ')
      after_lines=$(wc -l <"$path" | tr -d ' ')
      after_sha=$(sha256sum "$path" | awk '{print $1}')
    else
      after_bytes=null; after_lines=null; after_sha=null
    fi
    if git cat-file -e "HEAD:$path" 2>/dev/null; then
      before_bytes=$(git cat-file -s "HEAD:$path")
      before_lines=$(git show "HEAD:$path" | wc -l | tr -d ' ')
      # Git's object ID is not a content SHA-256 (and may be SHA-1), so hash
      # the exact HEAD bytes just as the working-tree measurement does.
      before_sha=$(git show "HEAD:$path" | sha256sum | awk '{print $1}')
    else
      before_bytes=null; before_lines=null; before_sha=null
    fi
    test "$first" = 1 || printf ','
    first=0
    printf '{"path":"%s","stage":"%s","projection":"%s","expansionReference":"%s","before":{"bytes":%s,"lines":%s,"sha256":"%s"},"after":{"bytes":%s,"lines":%s,"sha256":"%s"},"duplicateOf":"%s"}' \
      "$path" "$stage" "$projection" "$duplicate" "$before_bytes" "$before_lines" "$before_sha" \
      "$after_bytes" "$after_lines" "$after_sha" "$duplicate"
  done
  printf '],"telemetry":"exact-local-files"}\n'
  exit 0
fi

printf '%-52s %10s %8s  %s\n' surface bytes lines stage
printf '%-52s %10s %8s  %s\n' \
  '----------------------------------------------------' \
  '----------' '--------' '-----'

surface() {
  if test ! -f "$1"; then
    printf '%-52s %10s %8s  %s\n' "$1" absent - "$2"
    return
  fi
  bytes=$(wc -c <"$1" | tr -d ' ')
  lines=$(wc -l <"$1" | tr -d ' ')
  printf '%-52s %10s %8s  %s\n' "$1" "$bytes" "$lines" "$2"
}

# Loaded before a session has chosen anything, and before any work is bound.
surface AGENTS.md bootstrap
surface skills/exitbind-bootstrap/SKILL.md bootstrap

# Read only once the skill is selected and activated.
surface skills/exitbind/SKILL.md activated-skill

# Reached from the activated skill on demand, so it lands later than the
# skill body or not at all.
surface skills/exitbind/references/preservation.md delayed-reference

# Shipped a second time inside the plugin package. Same text, separate copy:
# it is measured here because it is a second surface that can drift.
surface plugins/exitbind/skills/exitbind/SKILL.md packaged-duplicate
surface plugins/exitbind/skills/exitbind/references/preservation.md packaged-duplicate

printf '\nTotal instruction surface: '
total=0
count=0
for path in \
  AGENTS.md \
  skills/exitbind-bootstrap/SKILL.md \
  skills/exitbind/SKILL.md \
  skills/exitbind/references/preservation.md \
  plugins/exitbind/skills/exitbind/SKILL.md \
  plugins/exitbind/skills/exitbind/references/preservation.md
do
  total=$((total + $(wc -c <"$path" | tr -d ' ')))
  count=$((count + 1))
done
printf '%s bytes across %s files\n' "$total" "$count"
