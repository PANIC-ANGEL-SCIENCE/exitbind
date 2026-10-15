# Release series and compatibility

Published stable tag: v0.31.16
Source candidate tag: v0.31.16

The published stable tag and current source version are 0.31.16.
README install links name the available stable assets. The published installation
pointer has been verified against the exact release tag and asset inventory. A source version or
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

Routine branch and paired-integration CI uses Linux only. Two explicitly selected
completed task branches can produce one Linux-only prerelease such as
`0.31.17-rc.1`; the candidate number is selected once for the pair, not incremented
for each commit. Prereleases never become the latest stable release. Before a
separately selected stable promotion, run all-platform stable readiness on the
exact integration candidate, then verify that result before changing main.
See [paired prereleases](prereleases.md). Existing tags and assets stay immutable.
