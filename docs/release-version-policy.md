# Release series and compatibility

Published stable tag: v0.31.12
Source candidate tag: v0.31.12

The published stable release is 0.31.12.
The current source version is 0.31.12.
README install links name the available published assets. A source version or
passing CI alone does not establish an official release. The release-reference
check validates published and candidate declarations separately; source metadata,
schemas and installation defaults must match the current source version.

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

## Automatic documentation references

`Published stable tag` above is the checked-in stable checkpoint; `Cargo.toml`
is the candidate authority. Run `python3 tooling/releases/sync_docs.py --write`
(Python 3.11+) to render references, or omit flags to check them. A candidate
bump never promotes stable installation links. `--write --from-github` first
verifies the canonical latest stable release, its published/non-draft status,
and all nine native assets. Both the checker and regression tests run in
`scripts/ci-local.sh`.

`tooling/releases/doc-versions.json` inventories exact version-bearing lines.
A `template` uses `{stable}` or `{candidate}` without the leading `v`; its
`count` catches missing or duplicated references. A `pinned` line and reason
preserve historical evidence, compatibility examples and host versions. The
changelog is the only whole-file archive exception; its source heading remains
checked by the existing release-reference gate. All Markdown, MDX,
reStructuredText and AsciiDoc files are scanned, including language variants
and guides outside `docs`. New version-bearing lines must join the inventory,
even when their values equal today's stable release. Add the exact template
or justified pinned line, render, then check. New formats need scanner tests.

## Release preparation and publication checkpoint

When updating the source version, keep the existing preparation sequence:
update `Cargo.toml`, its lockfile, installation defaults, plugin metadata and
validator, WSL expectation, and the source-candidate changelog entry. Then run:

```sh
sh tooling/releases/prepare_docs.sh
./scripts/ci-local.sh
```

The preparation command renders all inventoried document references and runs
the release-reference gate. It accepts the in-progress version edits and uses
the checked-in stable checkpoint, so a candidate bump leaves published install
links unchanged. Review and commit the metadata and document changes together.
CI checks the rendered documents without writing them and rejects stale or
unclassified references. It checks the saved stable checkpoint offline; it
does not poll GitHub for a newer release.

After the release workflow publishes the assets and the existing release actor
verifies publication, that actor must run this on a clean, current review branch:

```sh
sh tooling/releases/prepare_docs.sh --published
./scripts/ci-local.sh
```

This reads the canonical latest stable release with the actor's existing `gh`
access and verifies published status and all nine native asset entries before
advancing the stable checkpoint and documentation. Asset metadata validation
is not archive checksum, attestation, or installed-binary proof; the existing
release gates retain those responsibilities. An API error, draft, prerelease,
missing asset or backward version stops synchronization.

The actor must review and commit this post-publication update through the normal
review process. Until that commit is integrated, documents continue to name the
previous published stable release. This remaining step is manual; neither the
release workflow nor these commands create a branch, commit, PR, or merge.
No Actions PR-creation permission or documentation write permission is needed.
Keep published tags and their documents immutable.

This implementation remains a draft on top of `fix/fr45-residuals`; integration
into main still requires review.
