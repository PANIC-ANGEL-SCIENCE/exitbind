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

python3 tooling/repository/checks/check_rust_dependencies.py
python3 tooling/repository/checks/check_test_target_map.py
python3 tooling/repository/checks/check_package_inventory.py
python3 tooling/repository/checks/check_package_inventory.py --self-test
python3 tooling/repository/checks/check_skill_projection.py --check
python3 tooling/repository/checks/check_legacy_inventory.py --self-test
python3 tooling/repository/checks/check_legacy_inventory.py
python3 tooling/repository/checks/check_gate_inventory.py --self-test
python3 tooling/repository/checks/check_gate_inventory.py
