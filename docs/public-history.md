# Public release history

It verifies what you asked an agent to do and what came back, in the same
record.

For an older ledger, start with the
[public tag and format reader map](../CHANGELOG.md#public-tags-and-format-readers).
It distinguishes tagged source support from historical release notes and
identifies the reader required for checked runs.

## The retained history boundary

The current public repository begins at commit
[`a0b8be321d67cf81041d8cc8d49b13d86cbab4c0`](https://github.com/envy404/soulmate/commit/a0b8be321d67cf81041d8cc8d49b13d86cbab4c0),
which carries package version 0.10.0 and has no parent.
Earlier entries in CHANGELOG.md are historical release notes; they do not
establish Git ancestry available in this repository. This boundary is disclosed
rather than reconstructed from those notes.

The 0.11.0 release commit
[`7f1d0146694d6b049f60e6bfba968e2a8c3a104d`](https://github.com/envy404/soulmate/commit/7f1d0146694d6b049f60e6bfba968e2a8c3a104d)
retains that root and adds two commits. Contributor history validation pins this
published checkpoint and requires it to remain an ancestor of each checked
candidate. Pull-request CI checks the original PR head as well as the tested
merge commit, so a main-side parent cannot hide lost PR ancestry. Missing Git
evidence fails the check, including an incomplete checkout that cannot establish
ancestry.

## Verify a specific observation

Compare a reported README with a named commit or tag and record its blob ID.
A moving branch URL, search result or cached rendering can refer to a different
snapshot. Read the GitHub contents API for the selected ref and compare the
README install command with Cargo.toml and install.sh in that same snapshot.
The release-reference gate checks these source identities before publication;
it cannot invalidate an external search cache.

CI's ancestry check is local evidence. Provider rules must be verified
separately before reporting them enforced. An administrator able to change
rules or delete a repository remains outside these checks; neither a passing
CI run nor these instructions proves universal history retention.

Version selection remains defined in
[REFERENCE.md](../REFERENCE.md#versioning-and-release-contract).
