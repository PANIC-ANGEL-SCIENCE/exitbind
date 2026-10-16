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

for source in Cargo.toml Cargo.lock README.md REFERENCE.md SECURITY.md CHANGELOG.md install.sh \
  docs/release-version-policy.md tooling/repository/release/check-installation-pointer.py \
  plugin.json systems.soulmate/.codex-plugin/plugin.json \
  systems.soulmate/.claude-plugin/plugin.json tooling/repository/ci/ci-wsl.sh \
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
published=$(sed -n 's/^Published stable tag: //p' docs/release-version-policy.md)
candidate=$(sed -n 's/^Source candidate tag: //p' docs/release-version-policy.md)
printf '%s\n' "$published" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' ||
  fail 'release policy must declare one valid published stable tag'
test "$(printf '%s\n' "$published" | wc -l | tr -d ' ')" = 1 ||
  fail 'release policy has duplicate published stable tags'
equal_version 'release policy source candidate' "$candidate" "$current"

lock_version=$(awk '
  /^\[\[package\]\]$/ { exitbind = 0 }
  /^name = "exitbind"$/ { exitbind = 1 }
  exitbind && /^version = "/ { sub(/^version = "/, ""); sub(/"$/, ""); print }
' Cargo.lock)
equal_version Cargo.lock "$lock_version" "$plain"

# A literal command inside an HTML comment is not an installation instruction.
# Keep the whole-line command requirement while tracking only HTML comments.
awk -v command="curl -fsSL https://raw.githubusercontent.com/PANIC-ANGEL-SCIENCE/exitbind/$published/install.sh | sh" '
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
' README.md || fail "README.md is missing the visible published $published install command"
installer_version=$(sed -n 's/^version="${EXITBIND_VERSION:-\([^}]*\)}"$/\1/p' install.sh)
equal_version install.sh "$installer_version" "$current"
changelog_version=$(awk '
  /^## Unreleased — [0-9]+\.[0-9]+\.[0-9]+ source candidate$/ { print $4; exit }
  /^## [0-9]/ { print $2; exit }
' CHANGELOG.md)
equal_version CHANGELOG.md "$changelog_version" "$plain"

# Current installed guides must not retain the pre-release availability claims
# while version substitution alone makes their references look current.
for guide in docs/goal-requirements.md docs/legacy-compatibility.md; do
  test -r "$guide" || fail "missing or unreadable release guide: $guide"
  awk '
    { text = text " " $0 }
    END {
      gsub(/[[:space:]]+/, " ", text)
      text = tolower(text)
      exit (index(text, "unreleased source line") ||
            index(text, "development source; it does not change the stable")) ? 1 : 0
    }
  ' "$guide" || fail "obsolete development-only availability claim: $guide"
done

for manifest in plugin.json plugins/exitbind/plugin.json \
  plugins/exitbind/.codex-plugin/plugin.json \
  plugins/exitbind/.claude-plugin/plugin.json; do
  version=$(sed -n 's/^[[:space:]]*"version": "\([0-9][^"]*\)",*$/\1/p' "$manifest")
  equal_version "$manifest" "$version" "$plain"
done
plugin_validator_version=$(sed -n 's/^VERSION = "\([^"]*\)"$/\1/p' tooling/repository/release/check-plugin-distribution.py)
equal_version tooling/repository/release/check-plugin-distribution.py "$plugin_validator_version" "$plain"
for manifest in \
  systems.soulmate/.codex-plugin/plugin.json \
  systems.soulmate/.claude-plugin/plugin.json
do
  version=$(sed -n 's/^[[:space:]]*"version": "\([0-9][^"]*\)",*$/\1/p' "$manifest")
  equal_version "$manifest" "$version" '0.25.0-rc.5'
done

wsl_version=$(sed -n 's/^test "$(exitbind version)" = "\([0-9][^"]*\)"$/\1/p' tooling/repository/ci/ci-wsl.sh)
equal_version tooling/repository/ci/ci-wsl.sh "$wsl_version" "$plain"

# Historical CHANGELOG entries are deliberately outside the current-reference scan.
set -- README.md REFERENCE.md SECURITY.md install.sh docs examples schema scripts src
# Enumerate with find: recursive grep differs across hosts in whether it follows
# links. Validate and scan these explicit paths, preserving the .git exclusion.
# These three schema files are immutable Soulmate compatibility artifacts. Their
# historical repository/version IDs are intentionally not current release refs.
# The checker is excluded because its policy necessarily names historical refs.
refs=$(find -L "$@" \
  \( -path "schema/harness-manifest.schema.json" \
     -o -path "schema/soulmate.schema.json" \
     -o -path "schema/run-boundary.schema.json" \
     -o -path "tooling/repository/release/check-release-refs.sh" \) -prune \
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
printf '%s\n' "$refs" | awk -v current="$current" -v published="$published" '
  BEGIN { legacy = "v0.25.0" }
  {
    line = $0
    # These two literals are the immutable cutoff for the historical Soulmate
    # installer route. Ignore only the exact guard/message; all other release
    # references in install.sh remain subject to the current-version scan.
    if ($0 ~ /^install\.sh:[0-9]+:if test "\$surface" = soulmate && test "\$version" = v0\.25\.0; then$/ ||
        ($0 ~ /^install\.sh:[0-9]+:  echo/ && index($0, "Soulmate distribution ended at v0.25.0; install Exitbind instead") > 0)) {
      gsub(/v0\.25\.0/, current, line)
    }
    # These exact references describe retained compatibility facts.
    if ($0 ~ /^docs\/legacy-compatibility\.md:[0-9]+:/ &&
        (index($0, "`" legacy "`; historical releases and records remain available.") > 0 ||
         index($0, "Not built or installed by default at `" legacy "`;") > 0 ||
         index($0, "the `" legacy "` installer refuses a Soulmate install") > 0 ||
         index($0, "the `" legacy "` package does not ship it") > 0)) {
      gsub(legacy, current, line)
    }
    # This closed maintenance packet records its historical release and branch.
    # Other versions and files remain subject to the current-reference check.
    if ($0 ~ /^docs\/maintenance\/v0\.25\.1\/(README|01_REVIEWER_HANDOFF|02_R14_CLOSEOUT|03_VALIDATION_AND_ADAPTER)\.md:[0-9]+:/) {
      gsub(/v0\.25\.1/, current, line)
      if (index($0, "- branch: `fix/r13-v0.25.1-hardening`;") > 0) {
        sub(/-hardening/, "", line)
      }
    }
    # These exact quoted literals are numeric-order/invalid-version fixtures,
    # not installation links. Other references in this source stay checked.
    if ($0 ~ /^src\/distribution\/update\/version_policy_tests\.rs:[0-9]+:/) {
      gsub(/"v0\.31\.(0|9|99|01|09|010)"/, "\"" current "\"", line)
      gsub(/"v0\.32\.10"/, "\"" current "\"", line)
      gsub(/"v0\.031\.10"/, "\"" current "\"", line)
    }
    published_guide = ($0 ~ /^(README\.md|REFERENCE\.md|SECURITY\.md|docs\/(platform-support|value-proof-methodology|release-version-policy|goal-requirements|windows-wsl|first-checked-run|onboarding|upgrade-and-rollback)\.md):[0-9]+:/)
    while (match(line, /v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?([+][0-9A-Za-z]+([.-][0-9A-Za-z]+)*)?/)) {
      version = substr(line, RSTART, RLENGTH)
      if (version != current && !(published_guide && version == published)) {
        print "stale release reference (expected " current "): " $0
        failed = 1
        break
      }
      line = substr(line, RSTART + RLENGTH)
    }
  }
  END { exit failed }
' >&2
