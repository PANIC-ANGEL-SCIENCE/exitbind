#!/bin/sh
# Require the named exact Cargo test to have executed and passed.
set -eu
test "$#" -eq 1 || { echo 'usage: assert-test-ran.sh TEST_NAME' >&2; exit 2; }
awk -v result="test $1 ... ok" '
  $0 == "running 1 test" { ran = 1 }
  $0 == result { passed = 1 }
  END { exit !(ran && passed) }
'
