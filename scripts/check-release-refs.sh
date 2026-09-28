#!/bin/sh
set -eu

fail() {
  printf '%s\n' "$*" >&2
  exit 1
}

equal_version() {
  test -n "$2" && test "$2" = "$3" ||
    fail "$1 version is '$2'; expected '$3'"
}

for source in Cargo.toml Cargo.lock README.md REFERENCE.md CHANGELOG.md install.sh \
  plugin.json systems.soulmate/.codex-plugin/plugin.json \
  systems.soulmate/.claude-plugin/plugin.json scripts/ci-wsl.sh \
  docs examples schema scripts src
do
  test -r "$source" || fail "missing or unreadable release source: $source"
done

plain=$(awk '
  /^\[package\]$/ { package = 1; next }
  /^\[/ { package = 0 }
  package && /^version = "/ { sub(/^version = "/, ""); sub(/"$/, ""); print }
' Cargo.toml)
test "$(printf '%s\n' "$plain" | wc -l | tr -d ' ')" = 1 ||
  fail 'Cargo.toml must contain one package version'
printf '%s\n' "$plain" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?([+][0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?$' ||
  fail 'Cargo.toml package version is missing or invalid'
current="v$plain"

lock_version=$(awk '
  /^\[\[package\]\]$/ { exitbind = 0 }
  /^name = "exitbind"$/ { exitbind = 1 }
  exitbind && /^version = "/ { sub(/^version = "/, ""); sub(/"$/, ""); print }
' Cargo.lock)
equal_version Cargo.lock "$lock_version" "$plain"

# A literal command inside an HTML comment is not an installation instruction.
# Keep the whole-line command requirement while tracking only HTML comments.
awk -v command="curl -fsSL https://raw.githubusercontent.com/envy404/exitbind/$current/install.sh | sh" '
  {
    if (!comment && $0 == command) found = 1
    line = $0
    while (length(line)) {
      if (comment) {
        end = index(line, "-->")
        if (!end) break
        line = substr(line, end + 3)
        comment = 0
      } else {
        start = index(line, "<!--")
        if (!start) break
        line = substr(line, start + 4)
        comment = 1
      }
    }
  }
  END { exit !found }
' README.md || fail "README.md is missing the visible current $current install command"
installer_version=$(sed -n 's/^version="${EXITBIND_VERSION:-\([^}]*\)}"$/\1/p' install.sh)
equal_version install.sh "$installer_version" "$current"
changelog_version=$(awk '/^## [0-9]/ { print $2; exit }' CHANGELOG.md)
equal_version CHANGELOG.md "$changelog_version" "$plain"

for manifest in plugin.json; do
  version=$(sed -n 's/^[[:space:]]*"version": "\([0-9][^"]*\)",*$/\1/p' "$manifest")
  equal_version "$manifest" "$version" "$plain"
done
for manifest in \
  systems.soulmate/.codex-plugin/plugin.json \
  systems.soulmate/.claude-plugin/plugin.json
do
  version=$(sed -n 's/^[[:space:]]*"version": "\([0-9][^"]*\)",*$/\1/p' "$manifest")
  equal_version "$manifest" "$version" '0.25.0-rc.5'
done

wsl_version=$(sed -n 's/^test "$(exitbind version)" = "\([0-9][^"]*\)"$/\1/p' scripts/ci-wsl.sh)
equal_version scripts/ci-wsl.sh "$wsl_version" "$plain"

# Historical CHANGELOG entries are deliberately outside the current-reference scan.
set -- README.md REFERENCE.md install.sh docs examples schema scripts src
# Enumerate with find: recursive grep differs across hosts in whether it follows
# links. Validate and scan these explicit paths, preserving the .git exclusion.
# These three schema files are immutable Soulmate compatibility artifacts. Their
# historical repository/version IDs are intentionally not current release refs.
refs=$(find -L "$@" \
  \( -path "schema/harness-manifest.schema.json" \
     -o -path "schema/soulmate.schema.json" \
     -o -path "schema/run-boundary.schema.json" \) -prune \
  -o -type d -name .git -prune -o -exec sh -c '
  for source do
    if ! test -r "$source" || { test -d "$source" && ! test -x "$source"; }; then
      printf "missing or unreadable scan source: %s\n" "$source" >&2
      exit 1
    fi
    test ! -d "$source" || continue
    if grep -InE "v[0-9]+\.[0-9]+\.[0-9]+" /dev/null "$source"; then
      :
    else
      status=$?
      test "$status" = 1 || exit "$status"
    fi
  done
' sh {} +) || fail 'release-source traversal or reference scan failed'
printf '%s\n' "$refs" | awk -v current="$current" '
  {
    line = $0
    # These two literals are the immutable cutoff for the historical Soulmate
    # installer route. Ignore only the exact guard/message; all other release
    # references in install.sh remain subject to the current-version scan.
    if ($0 ~ /^install\.sh:[0-9]+:if test "\$surface" = soulmate && test "\$version" = v0\.25\.0; then$/ ||
        ($0 ~ /^install\.sh:[0-9]+:  echo/ && index($0, "Soulmate distribution ended at v0.25.0; install Exitbind instead") > 0)) {
      gsub(/v0\.25\.0/, current, line)
    }
    while (match(line, /v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?([+][0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?/)) {
      version = substr(line, RSTART, RLENGTH)
      if (version != current) {
        print "stale release reference (expected " current "): " $0
        failed = 1
        break
      }
      line = substr(line, RSTART + RLENGTH)
    }
  }
  END { exit failed }
' >&2
