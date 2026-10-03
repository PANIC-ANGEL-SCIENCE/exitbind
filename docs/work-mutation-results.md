# Work mutation results (current source branch)

## Current effective action

Programmatic Exitbind Work responses add `effectiveAction`, derived from the existing
canonical current state and action forms. Existing fields keep their meanings.
The optional historical Soulmate CLI keeps its existing response shape.
Use its `exists`, `kind`, `state`, `binding`, `readiness` and `effect` to select
the current path. `done` has no action; `blocked` and `unknown` also have no
action but require inspection. A zero process exit alone does not establish
semantic success. Warnings stay separate from action existence.

The bounded response's `requiresDetail` and exact `detail.command` lead to the
current grouped detail. Read that route before dependent execution or review.
Its projection includes complete `choices` (with current-bound command argv,
required input and placeholders), `beforeEditing`, or a mechanical `command`.
The Lead supplies semantic decisions and reasons. The projection does not
execute a choice, grant rights or bypass the pre-edit governor. Binding changes
require reacquisition; old action forms remain stale and are refused.

The legacy section routes in `current.details` also use explicit
`sameExecutableRequired` and `sameConfigRequired` markers for unusually long
paths. Reuse those exact invocation arguments before executing such a route.

Held, refused and uncertain effects expose no advancement action. An inspection
route remains available through the existing recovery fields. A terminal READY
with later memory drift warnings remains done and does not replay governance.

## Recorded mutation

`work check WORK`, `work permit WORK ASSIGNMENT --operation OPERATION`, and
`work return WORK ASSIGNMENT --outcome OUTCOME` emit one compact JSON result by
default. `work return` also accepts `--json` for scripts. The emitted JSON,
including its newline, is at most 8 KiB.

`effect` says whether this invocation recorded an event or held a worker
result. `status` can additionally say `refused` when the recorded event is a
protection refusal. A recorded failed or signalled `work check` exits nonzero
after printing its compact JSON result; the event, checker result, logs, and
read-only recovery command remain available in that JSON. A zero exit means
the recorded check result passed or the mutation was another successful
operation. Read the checker's `result.kind` and `result.code` or
`result.signal` separately. An invalid request or an error before recording
exits nonzero without a recorded check event.

`eventSha256` is the event from this invocation. `reference.headEventSha256`
may be a later head and must not replace `eventSha256`. The `next` object is a
hint with `requiresExpansion: true`; read `work next WORK --full` with the same
config before another mutation. If a compact lookup says
`constraintsOmitted: true`, its packet is absent until that full read.

`nextAction.command` is an argv array for a read-only
`run inspect LEDGER --event SHA --json --config CONFIG`. It selects the named
event even after the ledger advances and never reruns a checker or submits a
held result. The detail response reports `authorizesMutation: false`.
`evidence` reports referenced submission or check-log bytes as `verified`,
`missing`, or `changed`; unsafe or corrupt check-log references cause an
integrity error. A historical detail lookup does not validate the current
subject for reuse.

For an unusually long executable or config path, a mutation detail command
can be an argv suffix marked `nextAction.sameExecutableRequired: true` or end
at `--config` with `nextAction.sameConfigRequired: true`. Compact lookups use
the corresponding `fullCommandSameExecutableRequired` and
`fullCommandSameConfigRequired` markers. Append the exact config value used
for the current invocation. Do not substitute a default config. If
`fullCommandSameExecutableRequired: true`, prepend the
exact executable that produced the current reply to the returned argv suffix;
the product never substitutes a bare command name for an oversized executable
path. `diagnostic.phases` independently names `projection` and
`cleanup` when either failed after recording; the event identity and outcome
remain available.

`work permit` reports `allowed`, the recorded event ID, governor state, and a
short next-action hint. Its `nextAction.commandSuffix` reads the full current
work assignment; `audit.commandSuffix` inspects the named event. Both are
read-only and require the same executable and config used for the permit call.
The full governor history stays in those read surfaces.
