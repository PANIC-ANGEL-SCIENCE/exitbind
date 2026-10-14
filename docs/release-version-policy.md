# Release series and compatibility

Published stable tag: v0.31.14
Source candidate tag: v0.31.14

The selected published stable release is 0.31.14.
The current source candidate is 0.31.14.
The published release is available; README install links name its published assets. A source version or
passing CI alone does not establish an official release. The release-reference
check validates published and candidate declarations separately; source metadata,
schemas and installation defaults must match the current source version.

After an authorized delivery, validate the exact selected tag's offline release
readback with `python3 scripts/check-installation-pointer.py --metadata FILE
--root PROJECT --expected-version TAG --expected-producer COMMIT`. Supply the
release JSON, the peeled tag commit as `producer_commit`, and asset names with
their GitHub SHA-256 digests. The checker requires the published stable state,
the matching `Published stable tag` policy line, one visible README install
command for that tag, and the exact three-platform inventory of binaries,
archives and checksum files. An available stable tag may differ from a newer
source candidate when release assets have not yet been published. Verify
attestations as part of delivery before recording its result. Obtain and
authenticate that readback through the delivery process; the checker performs
no network or `latest` lookup and does not authenticate its input file. Missing
readback files return `verification unavailable`, separately from mismatches.

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

## Preparing documentation references

`Cargo.toml` remains the source-candidate authority. After editing release
metadata, run `sh tooling/releases/prepare_docs.sh`, then the shared native CI
sequence. The command renders inventoried document references from the saved
stable selection and candidate version. Review and commit the changes with the
version preparation. Ordinary preparation leaves changelog history and
Unreleased headings unchanged; a newer candidate does not promote install links.

After an authorized publication, the existing release actor explicitly selects
the verified delivery readback described above:

```sh
sh tooling/releases/prepare_docs.sh --published --metadata /path/to/readback.json \
  --expected-version SELECTED_TAG --expected-producer PEELED_COMMIT
./scripts/ci-local.sh
```

The renderer stages the prospective README and policy in a temporary directory
and invokes `scripts/check-installation-pointer.py` unchanged, before writing
any document. It then promotes only the single exact selected version's
`## Unreleased — VERSION source candidate` heading to `## VERSION`. Other
headings, newer candidates, entry bodies and historical bytes are preserved.
An already-published matching heading is a no-op; duplicate or missing matching
headings require review. Missing readback remains `verification unavailable`.
An explicitly selected older stable is valid; this command never looks up latest.

The actor must inspect the resulting diff and commit it through normal review.
This manual publication checkpoint creates no commit, PR, tag or release and
needs no workflow write permission. The saved documentation may intentionally
continue to select an older stable during ongoing preparation.

`tooling/releases/doc-versions.json` classifies version-bearing lines as stable
or candidate templates, or pinned historical/tool references with reasons.
Markdown, MDX, reStructuredText and AsciiDoc documents are checked, including
language variants and files outside `docs`. Unknown lines and missing or
duplicated current references fail the offline checker. Add an exact template
or a justified historical pin for new references; do not relabel old evidence.
Changelog entry bodies remain archived, with only the verified heading promotion
above allowed. A saved stable selection still marked Unreleased fails the check
and requires the verified publication command. CI runs `python3 tooling/releases/sync_docs.py` without writing;
it checks the saved selection, including the README-only path, and does not
claim the saved selection is the newest published release.
