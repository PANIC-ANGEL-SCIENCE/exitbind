#!/bin/sh
set -eu

for term in \
  context_accounting \
  SOULMATE_EXPERIMENTAL_CONTEXT_OBSERVATIONS \
  _context-report \
  internal-context-dogfood
do
  if grep -RIn -- "$term" Cargo.toml src >/dev/null; then
    echo "experimental context observation code reached the product surface" >&2
    exit 1
  fi
done

test ! -e src/context_accounting.rs
test ! -e src/codex_usage.rs
