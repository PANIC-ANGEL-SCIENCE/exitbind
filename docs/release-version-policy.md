# Release series and compatibility

Source candidate tag: v0.31.22

The source version is 0.31.22. Its release commit carries the tag of the same
name, and the release workflow publishes that tag's assets; a source version or
passing CI alone does not establish an official release. Source metadata,
schemas, installation defaults and current-version mentions in the
documentation all equal the source version, and the release-reference check
validates them.

Installation instructions use GitHub's latest stable release:
`curl -fsSL https://github.com/PANIC-ANGEL-SCIENCE/exitbind/releases/latest/download/install.sh | sh`.
That address moves only when a stable release is published, so it never names
an unpublished version, and it needs no rewrite. Each release publishes its own
`install.sh` with `install.sh.sha256`, beside the binaries; that installer
defaults to its own release's version and verifies the archive checksum. The
release workflow creates a new release as a draft, uploads every asset, reads
the asset list back and only then publishes it, so the latest-release address
never points at a release whose installer is missing.

## Version mentions

Every version mention in the tracked documentation is listed in
`tooling/repository/release/doc-versions.json`: `current` mentions equal the
Cargo package version, and `pinned` mentions keep a historical version with a
reason (a historical fact, a minimum version or a closed maintenance packet).
The same inventory lists the lock file, plugin manifests, schemas, the example
manifest and the installer default. A release commit changes the version in
`Cargo.toml`, runs
`python3 tooling/repository/release/sync-doc-versions.py --apply` to rewrite
every `current` mention, and adds its CHANGELOG entry; no other version edit is
manual. The entry's first line is `Release:` followed by the release tag's URL
for that version, a statement that stays true after publication, so a release
commit never rewrites an older entry and `check-release-refs.sh` rejects an
"Unreleased" heading or a pre-publication availability sentence. `--check`
(run by the release-integrity tests) fails on a stale `current` mention and on
any mention the inventory does not list.

## Delivery readback

After an authorized delivery, validate the exact selected tag's offline release
readback with `python3 scripts/check-installation-pointer.py --metadata FILE
--root PROJECT --expected-version TAG --expected-producer COMMIT`. Supply the
release JSON, the peeled tag commit as `producer_commit`, and asset names with
their GitHub SHA-256 digests. The checker requires the published stable state,
a `Source candidate tag` policy line naming the selected tag, one visible README
install command in the latest-release form, and the exact inventory of
three-platform binaries, archives and checksum files plus `install.sh` and
`install.sh.sha256`. Verify attestations as part of delivery before recording
its result. Obtain and authenticate that readback through the delivery process;
the checker performs no network or `latest` lookup and does not authenticate its
input file. Missing readback files return `verification unavailable`,
separately from mismatches.

## Series and compatibility

Future stable releases in this series use 0.31.10 through 0.31.99, increasing
only when an immutable release is delivered. Versions 0.31.1 through 0.31.9 are
intentionally skipped. Core numbers have no leading zeroes: 0.31.01 is invalid.
The updater compares numbers, so 0.31.10 follows both 0.31.0 and 0.31.9. Passing
99 or needing a new compatibility epoch requires a deliberate series decision,
such as 0.32.10.

This is a pre-1.0 product convention. Compatible new features share a series
with fixes and are described as features. Breaking CLI, configuration, privacy
or persisted-format changes require explicit migration and version review.
Package versions never replace ledger format identities.

Capabilities are opt-in. Configurations without `project.capabilities` retain
the previous path. Strict older readers refuse that new configuration field;
retain the pre-registration configuration for rollback. Existing Work remains
bound to its original configuration and producer. New optional child-intent
metadata does not grant an older host permission. Compatible readers prepare
and validate current approved content.

Publication requires exact source review, native checks, hosted checks on the
selected publication ref, actual WSL validation, tag checks, release attestations
and installer proof. Official release and operator installation remain separate decisions.
Published tags and assets stay immutable.

Routine branch and paired-integration CI uses Linux only. Two explicitly selected
completed task branches can produce one Linux-only prerelease such as
`0.31.17-rc.1`; the candidate number is selected once for the pair, not incremented
for each commit. Prereleases never become the latest stable release. Before a
separately selected stable promotion, run all-platform stable readiness on the
exact integration candidate, then verify that result before changing main.
See [paired prereleases](prereleases.md). Existing tags and assets stay immutable.
