#!/bin/sh
# Verify every transferred archive against its bundle and exact binary before publication.
set -eu
: "${RELEASE_TARGETS:?release target inventory is required}"
: "${GH_TOKEN:?workflow token is required}"
: "${GITHUB_REPOSITORY:?repository identity is required}"
: "${GITHUB_REF:?source ref is required}"
: "${GITHUB_SHA:?source digest is required}"
expected=$(for target in $RELEASE_TARGETS; do
  printf '%s\n' "$target.bundle.json" "exitbind-$target" \
    "exitbind-$target.tar.gz" "exitbind-$target.tar.gz.sha256"
done | sort)
actual=$(for file in release-evidence/*; do basename "$file"; done | sort)
test "$actual" = "$expected"
verified_payloads=$(mktemp -d)
cleanup_verified_payloads() { rm -rf "$verified_payloads"; }
trap cleanup_verified_payloads EXIT HUP INT TERM
# Validate every transferred digest and archive inventory before invoking any
# attestation verifier. This preserves the release gate's all-target phases.
for target in $RELEASE_TARGETS; do
  archive="exitbind-$target.tar.gz"
  (cd release-evidence && sha256sum -c "$archive.sha256")
  test "$(tar -tzf "release-evidence/$archive")" = "exitbind-$target"
done
for target in $RELEASE_TARGETS; do
  archive="exitbind-$target.tar.gz"
  gh attestation verify "release-evidence/$archive" \
    --bundle "release-evidence/$target.bundle.json" \
    --repo "$GITHUB_REPOSITORY" \
    --signer-workflow "$GITHUB_REPOSITORY/.github/workflows/release.yml" \
    --source-ref "$GITHUB_REF" \
    --source-digest "$GITHUB_SHA" \
    --deny-self-hosted-runners
done
for target in $RELEASE_TARGETS; do
  archive="exitbind-$target.tar.gz"
  payload_dir="$verified_payloads/$target"
  mkdir "$payload_dir"
  tar -xzf "release-evidence/$archive" -C "$payload_dir"
  test ! -L "$payload_dir/exitbind-$target"
  test -f "$payload_dir/exitbind-$target"
  test -x "$payload_dir/exitbind-$target"
  cmp "$payload_dir/exitbind-$target" "release-evidence/exitbind-$target"
done
mkdir dist
for target in $RELEASE_TARGETS; do
  cp "release-evidence/exitbind-$target" dist/
  cp "release-evidence/exitbind-$target.tar.gz" dist/
  cp "release-evidence/exitbind-$target.tar.gz.sha256" dist/
done
