# Upgrade and rollback

An update selects the executable and managed host guidance used for future
work. It does not move project files or change what an active Work means.
Before switching versions, keep the executable and configuration that govern
each active Work available.

## What a normal update changes

The supported installer replaces the selected `exitbind` binary. The update
command also refreshes Exitbind-managed host guidance. Neither path is meant to
replace your custom role profiles, project configuration, unrelated host
files, or project data. Setup previews show the proposed changes; apply only
after reviewing the paths and ownership. Repeating an already-applied setup
should report managed files as unchanged and leave custom files alone.

Before changing a working selector, retain each active Work's pinned binary at
a stable path you manage, outside the installer's `.exitbind-previous-*`
directories. The installer keeps only the most recently displaced binary and
removes older aside directories during a later install; those temporary copies
are not a place to store active Work governors.

Portable projects keep their reviewed configuration and profiles in the
project. Local projects keep them in the configured control directory. The
installer does not migrate one storage mode into the other. See
[onboarding](onboarding.md#choose-the-storage-mode) for the storage choices.

## Keep active work with its original reader

Each active Work keeps its original governor executable, configuration,
ledger and accepted decisions. Installing a new default does not change that
Work, renew its grants, reset its accounting or reinterpret its earlier
acceptance. Continue or inspect it with the supported reader and state that
belong to it.

The 0.29.0 line writes a marked v8 `carryProtocol` version 1 accounting seed
when `run supersede` creates a same-goal successor. It preserves the governor's
phase, spent count, no-information streak, post-re-plan count, re-plan count,
budget and defaults, seen-evidence fingerprints, and observation keys. The
successor receives a fresh run identity, assignment and authority; old
mutation pointers, grants and check evidence do not carry. Its current result
still needs its own check, review and Lead decision. The cumulative spent count
is accounting context, not a lifetime budget cutoff.

A reader that predates this marker deliberately refuses the newer state
without mutating it. That refusal is a compatibility boundary, not transparent
downgrade support. Historical markerless records retain their existing meaning.

The 0.30.1 release also includes named requirements and memory policy-correction
events. Keep a compatible producer for their completion and lifecycle commands.
An older executable's raw goal-status display does not establish support for
named-goal completion. Policy-corrected memory uses v3 events that older memory
readers refuse. Switching the executable selector does not downgrade these
saved formats or restore an earlier authority decision.

Explicit known-ended check recovery in 0.30.1 appends a distinct v8
`check_observation_recovered` event. A reader without that action refuses the
affected ledger. Retain its original executable and configuration, and explicitly
select the compatible recovery reader for that Work's remaining lifecycle.
The recovery records reported failure, preserves accounting and earlier observer
identity, and transfers no check, grant or acceptance. Other Work pins stay as
selected. See [check recovery](check-recovery.md).

## Roll back the selected executable

Rollback switches the executable selector to a version that can read the state
you intend to use. It does not roll back a ledger or transfer acceptance to
changed files. You can install a specific published version by setting
`EXITBIND_VERSION` to its exact release tag when running the installer; for
example, after the version and its assets are available:

```sh
EXITBIND_VERSION="$ROLLBACK_TAG" sh ./install.sh
```

For a rollback to the 0.28.0 line, set `ROLLBACK_TAG` to that published tag.

If the state contains a marked record that the selected older reader rejects,
keep the compatible newer executable for that Work, or keep its state separate
while using the older reader for older records. Do not remove the marker, edit
the ledger, or restore an old snapshot over newer work to get past the refusal.
Keep the existing compatible reader and its records together until the newer
Work is complete.

Historical records retain their original producer and schema meaning. See the
[public format map](../CHANGELOG.md#public-tags-and-format-readers) and
[legacy compatibility](legacy-compatibility.md) before switching readers.
