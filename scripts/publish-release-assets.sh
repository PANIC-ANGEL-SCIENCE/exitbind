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
  printf '%s\n' "exitbind: release tag '$tag' does not exactly match current package version '$expected_tag'" >&2
  exit 1
}
prerelease=false
case "${version%%+*}" in *-*) prerelease=true ;; esac

if release=$(gh release view "$tag" --repo "$repository" --json isPrerelease,isDraft --jq ".isPrerelease == $prerelease and .isDraft == false"); then
  test "$release" = true || {
    printf '%s\n' "exitbind: existing release must be published and match the package release channel" >&2
    exit 1
  }
else
  if test "$prerelease" = true; then
    gh release create "$tag" --repo "$repository" --verify-tag --title "Exitbind $tag" --generate-notes --prerelease --latest=false
  else
    gh release create "$tag" --repo "$repository" --verify-tag --title "Exitbind $tag" --generate-notes
  fi
fi

gh release upload "$tag" --repo "$repository" dist/*
