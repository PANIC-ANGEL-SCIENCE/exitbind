# Release series and compatibility

Published stable tag: v0.31.11
Source candidate tag: v0.31.11

The published stable release and current source version are both 0.31.11.
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
