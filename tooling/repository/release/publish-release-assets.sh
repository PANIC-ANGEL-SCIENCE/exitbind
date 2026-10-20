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

# A stable release becomes GitHub's "latest" as soon as it is published, and
# README installs from releases/latest/download/install.sh. So a new release is
# created as a draft, every asset is uploaded and read back, and only then is it
# published. A draft left by an interrupted run of this channel is resumed.
state=$(gh release view "$tag" --repo "$repository" --json isPrerelease,isDraft --jq '"\(.isDraft) \(.isPrerelease)"') || state=missing
case "$state" in
  missing)
    if test "$prerelease" = true; then
      gh release create "$tag" --repo "$repository" --verify-tag --title "Exitbind $tag" --generate-notes --draft --prerelease --latest=false
    else
      gh release create "$tag" --repo "$repository" --verify-tag --title "Exitbind $tag" --generate-notes --draft
    fi
    draft=true
    ;;
  "true $prerelease") draft=true ;;
  "false $prerelease") draft=false ;;
  *)
    printf '%s\n' "exitbind: existing release must be published or a draft of the package release channel" >&2
    exit 1
    ;;
esac

gh release upload "$tag" --repo "$repository" dist/*
test "$draft" = true || exit 0

expected=$(for file in dist/*; do printf '%s %s\n' "${file#dist/}" "$(wc -c < "$file" | tr -d ' ')"; done | sort)
actual=$(gh release view "$tag" --repo "$repository" --json assets --jq '.assets[] | "\(.name) \(.size)"' | sort)
test "$actual" = "$expected" || {
  printf '%s\n' "exitbind: draft release assets differ from the verified dist set; not publishing" >&2
  exit 1
}
if test "$prerelease" = true; then
  gh release edit "$tag" --repo "$repository" --draft=false --latest=false
else
  gh release edit "$tag" --repo "$repository" --draft=false --latest
fi
