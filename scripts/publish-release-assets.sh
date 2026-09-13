#!/bin/sh
set -eu

repository=${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}
tag=${GITHUB_REF_NAME:?GITHUB_REF_NAME is required}
version=$(awk '
  /^\[package\]$/ { package = 1; next }
  /^\[/ { package = 0 }
  package && /^version = "/ { sub(/^version = "/, ""); sub(/"$/, ""); print; exit }
' Cargo.toml)
test -n "$version" || {
  printf '%s\n' "exitbind: checked-out Cargo.toml has no package version" >&2
  exit 1
}
expected_tag="v$version"
test "$tag" = "$expected_tag" || {
  printf '%s\n' "exitbind: release tag '$tag' does not exactly match current stable version '$expected_tag'" >&2
  exit 1
}
case "$version" in
  *-*) printf '%s\n' "exitbind: current package version is not stable" >&2; exit 1 ;;
esac

if release=$(gh release view "$tag" --repo "$repository" --json isPrerelease,isDraft --jq '.isPrerelease == false and .isDraft == false'); then
  test "$release" = true || {
    printf '%s\n' "exitbind: existing release must be published and non-prerelease" >&2
    exit 1
  }
else
  gh release create "$tag" --repo "$repository" --verify-tag --title "Exitbind $tag" --generate-notes
fi

gh release upload "$tag" --repo "$repository" dist/*
