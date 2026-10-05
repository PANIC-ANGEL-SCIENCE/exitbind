# Architecture Contract

An optional Architecture Contract gives the Lead, workers and reviewers the
project's currently selected responsibilities, dependency directions,
interfaces and deterministic checks. A small direct task needs no contract or
governed run. This capability expresses the project's architecture; it does
not prescribe a style or relocate application code.

Use the existing project decision process to review the intended source, then
preview its selection:

```sh
exitbind project architecture select docs/architecture.json \
  --decision reviewed --reason "PROJECT_REVIEW_DECISION" --json --config CONFIG
```

The preview validates the source/schema, computes its exact revision and digest,
and shows the current/proposed selection and responsibility/check summary. It
writes nothing and launches no checker. Execute its returned `apply.command`
argv to select those bytes. The command already includes the exact executable,
custom configuration and current binding; no hash copying is needed. The
`reviewed` declaration and reason describe the existing project decision. They
are caller statements, not proof that a human reviewed the source.

Apply revalidates source and configuration, refuses stale/cross-project bindings
or competing selection, and preserves other custom configuration fields and
file permissions. A successful result returns a refreshed apply command that
is a byte-preserving no-op while that selection/configuration stays current.
If another selection/configuration/source change wins before the response,
`applied_current_changed` preserves the confirmed write effect and returns only
a read-only inspection command. It never rebinds the old selection intent to
that competing configuration. The consumed preview is stale after a write;
refresh preview after any change.
Unsafe, missing, oversized or invalid sources are refused. Apply uses POSIX
file locking on supported native hosts. Selection grants no execution rights
and cannot change the frozen configuration of an active Work; approved changes
use its existing supersession path.

The existing project configuration selects one source under ProductRoot:

```json
"architectureContract": {
  "sourcePath": "docs/architecture.json",
  "sourceSha256": "LOWERCASE_SHA256_OF_THE_EXACT_SOURCE_BYTES",
  "revision": "approved-revision-1"
}
```

Put this optional object inside `project` in `exitbind.json`. Selection is an
owner-managed configuration decision through the project's existing review
and approval process. Exitbind validates the selection and exact source bytes;
it cannot establish that a human approved them. No separate architecture
approval ledger is created. The source must be a regular, safely readable
project-relative JSON file, at most 64 KiB, with schema version `1` and the
same revision as the configuration pin.

The [representative contract](../tests/fixtures/architecture-contract.json)
shows the complete format. All five collections/fields are explicit:

| Field | Meaning |
| --- | --- |
| `responsibilities` | Unique IDs, summaries and nonoverlapping project-relative file/directory paths. Descendants of a directory belong to that responsibility. |
| `dependencies` | Directed `from`/`to` responsibility pairs with `direction` equal to `allowed` or `forbidden`, and an optional interface ID. Unmentioned directions are unspecified. |
| `interfaces` | Unique IDs, owning responsibility, exact file path and summary. The file belongs to its declared owner. |
| `checks` | Unique IDs, owning responsibility, exact file path, `contains` or `excludes` assertion and nonempty literal. A check may reference a declared dependency or an interface at the same path. |
| `revision` | Project-chosen nonempty revision label; distinct from schema version and the exact source digest. |

There are at most 64 responsibilities, interfaces and checks, and 128 dependency
pairs. Responsibility paths are normalized and nonoverlapping. Invalid owners,
contradictory directions for the same pair and unknown fields are refused.

## Read and check

```sh
exitbind project architecture --json
exitbind project architecture check --json
```

The first command presents the current contract with source path, SHA256,
revision, schema version and selecting configuration SHA256. `project context`
also exposes its current provenance or an unavailable reason; it copies no
full contract into setup or session discovery.

The second command reads only the exact named UTF-8 files, up to 64 KiB each,
and evaluates literal assertions. It records input digests in its output,
rereads inputs and the contract before returning, and exits nonzero when an
assertion fails or an input is unsafe, missing, oversized or changed. An
absent contract returns `skipped`. An empty check list proves no file behavior.
These assertions do not parse imports or infer a complete language dependency
graph. Associate an `excludes` assertion with a forbidden dependency, or a
`contains` assertion with an interface, when that exact literal discriminates
the behavior the project needs to preserve.

When governed evidence is needed, explicitly choose this command through the
existing Work check/preservation owners. For an already authorized external
linter or dependency test, compose commands with `&&`, for example
`APPROVED_PROJECT_CHECK && exitbind project architecture check --json --config CONFIG`.
An earlier failing exit stops the compound check; never replace it with a
semicolon that could mask failure. Repository command suggestions remain
proposals until authorized. Separate preservation requirements keep their own
explicit frozen check where they discriminate a different obligation.

Its stdout alone is not
acceptance; the existing check, review and Lead acceptance owners remain the
[authority boundary](../REFERENCE.md#authority-boundary). Contract data contains
no executable command field.

## Current architecture, proposals and delivery

The Lead remains free to propose architectural changes. Put a proposed source
beside the current one or return it through the existing task/review process.
An unselected file is never discovered or promoted. A proposal becomes current
only when the authorized project process selects its exact bytes and revision
in configuration. A changed source with an unchanged pin is refused; an
existing governed Work cannot adopt a new selection in place of its frozen
configuration. Use the existing run supersede path for approved changes that
must affect that Work. Historical records are preserved.
New marked check observations refuse configuration or selected-source drift
before admission or execution on both Work and direct observation routes;
drift during execution records a non-check observation failure.

`work detail`, `work act` native launch context, the bound SubagentStart hook
and explicit `work child context` use the same derived slice. Legacy `away`
refuses preparation when current configuration selects a contract or the
assignment carries a frozen contract identity: this adapter does not support
contract delivery. Use the current Work delivery routes. Absent-contract away
work keeps its existing behavior. Workers and reviewers receive the
responsibilities intersecting their assignment's observe/write paths, incident
dependency directions, associated interfaces and the selected owners' checks.
They receive no unrelated responsibility summaries or checks. Empty paths
produce an empty slice; project-wide paths can select the whole contract.
Narrow broad profiles with an existing run boundary manifest when the task is
smaller. Advisers also receive scoped context. The Lead can inspect the whole
current contract. Provenance identifies the complete selected source even when
only a slice is delivered.

This is read-only context delivery under the existing authority boundary; it
grants no command, path, memory or host permission. Checks are explicit and
are not executed by reading context. Source changes are rechecked before
native launch and during governed transitions. A delivered slice does not
prove that a model followed it, and it is not an OS enforcement mechanism.
