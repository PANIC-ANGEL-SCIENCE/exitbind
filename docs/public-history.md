# Public release history

## The organization import baseline

The organization repository begins at the single root commit
[`cde4c003ad885b99ec8e405c8bc80e89935bb050`](https://github.com/PANIC-ANGEL-SCIENCE/exitbind/commit/cde4c003ad885b99ec8e405c8bc80e89935bb050).
It imports the source tree at the transition from Soulmate to Exitbind. Earlier
development history is privately retained. Soulmate names, compatibility
artifacts and historical release notes remain in the source where relevant.

Subsequent Exitbind development commits remain individual commits. Their source
trees, messages, and author and committer dates and timezones are retained.
Maintainer attribution uses the current public GitHub noreply identity.
Rewriting parentage and attribution changes commit and tag object IDs and
invalidates earlier commit signatures. Original signed release archives are
not evidence that a rewritten source commit produced those bytes.

Contributor validation pins the import baseline and requires it to remain an
ancestor of each checked candidate. Pull-request CI checks the original PR head
as well as the tested merge commit. Replacement objects, grafts or a main-side
merge parent cannot hide lost PR ancestry. Missing Git evidence and shallow
checkouts fail the check.

## Releases and installed readers

Historical tags identify retained source snapshots. Historical release notes
do not establish that binary assets are published in this repository. Newly
published archives carry provenance for their actual organization repository,
source commit and release tag; old attestations are never relabeled.

For older record formats, consult the
[tag and reader map](../CHANGELOG.md#public-tags-and-format-readers). Keep the
compatible reader and configuration pinned for each active Work. See
[upgrade and rollback](upgrade-and-rollback.md) for the installation cutover.

## Verify a specific observation

Compare documentation, Cargo.toml and install.sh at the same named commit or
tag. A moving branch URL, search result or cached rendering may show another
snapshot. The release-reference gate checks source consistency, while the
history gate checks actual local ancestry. Provider protection is separate;
an administrator who can change repository rules remains outside these checks.

Version selection is defined in
[REFERENCE.md](../REFERENCE.md#versioning-and-release-contract).
