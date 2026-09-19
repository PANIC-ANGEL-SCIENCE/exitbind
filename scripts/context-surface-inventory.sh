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
